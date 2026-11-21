//! Decodes one audio file or stream into interleaved f32 frames with Symphonia.
//!
//! 16- and 24-bit PCM fit exactly in f32's 24-bit mantissa, so the conversion itself is lossless.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, Timestamp};

pub struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    pub rate: u32,
    pub channels: usize,
    pub bits: Option<u32>,
    pub codec: String,
    /// The length the file's header promises, in frames (when it says).
    pub total_frames: Option<u64>,
    /// Packets that couldn't be decoded and were skipped.
    pub errors: u32,
    scratch: Vec<f32>,
    /// Frames still to drop from the start of the decoded audio: a seek lands on the packet that
    /// holds the wanted frame, up to a whole packet early.
    skip: u64,
    /// Where in `scratch` the next chunk starts (after a skip, or what `give_back` returned).
    start: usize,
    /// The rest of the last chunk is to be returned again (see `give_back`).
    replay: bool,
    last_format: (u32, usize),
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).with_context(|| format!("can't open {}", path.display()))?;
        let ext = path.extension().and_then(|e| e.to_str());
        Self::open_media_with(Box::new(f), ext, false)
    }

    /// Opens a file with checksum verification enabled (FLAC STREAMINFO MD5).
    pub fn open_verified(path: &Path) -> Result<Self> {
        let f = File::open(path).with_context(|| format!("can't open {}", path.display()))?;
        let ext = path.extension().and_then(|e| e.to_str());
        Self::open_media_with(Box::new(f), ext, true)
    }

    pub fn open_reader(source: Box<dyn MediaSource>, hint_ext: Option<&str>) -> Result<Self> {
        Self::open_media_with(source, hint_ext, false)
    }

    fn open_media_with(source: Box<dyn MediaSource>, hint_ext: Option<&str>, verify: bool) -> Result<Self> {
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = hint_ext {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .context("unsupported audio format")?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| anyhow!("no audio track found"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| anyhow!("no audio codec parameters"))?
            .clone();
        let mut opts = AudioDecoderOptions::default();
        opts.verify = verify;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &opts)
            .context("unsupported audio codec")?;
        let codec = symphonia::default::get_codecs()
            .get_audio_decoder(params.codec)
            .map(|d| d.codec.info.short_name.to_uppercase())
            .unwrap_or_else(|| "AUDIO".into());

        let rate = params.sample_rate.unwrap_or(44_100);
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2);

        Ok(Self {
            track_id: track.id,
            rate,
            channels,
            bits: params.bits_per_sample,
            codec,
            total_frames: track.num_frames,
            errors: 0,
            format,
            decoder,
            scratch: Vec::new(),
            skip: 0,
            start: 0,
            replay: false,
            last_format: (rate, channels),
        })
    }

    /// Decodes the next packet. Returns `None` at the end of the stream.
    /// The rate/channels of a decoded buffer can differ from the header, so they're returned with the samples.
    pub fn next_chunk(&mut self) -> Result<Option<(&[f32], u32, usize)>> {
        if self.replay {
            self.replay = false;
            if self.start < self.scratch.len() {
                let (rate, channels) = self.last_format;
                return Ok(Some((&self.scratch[self.start..], rate, channels)));
            }
        }
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(SymError::ResetRequired) => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    let spec = buf.spec();
                    let (rate, channels) = (spec.rate(), spec.channels().count());
                    self.scratch.resize(buf.samples_interleaved(), 0.0);
                    buf.copy_to_slice_interleaved(&mut self.scratch[..]);
                    // Frames before the seek target (the start of the packet it landed in).
                    let frames = (self.scratch.len() / channels.max(1)) as u64;
                    let dropped = self.skip.min(frames);
                    self.skip -= dropped;
                    self.start = dropped as usize * channels.max(1);
                    if self.start >= self.scratch.len() {
                        continue;
                    }
                    self.last_format = (rate, channels);
                    return Ok(Some((&self.scratch[self.start..], rate, channels)));
                }
                // Corrupt packet: skip it rather than stopping playback
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                    self.errors += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Only the first `frames` of the last chunk were used: the next `next_chunk` returns the rest
    /// of it (for CUE tracks or crossfade boundaries).
    pub fn give_back(&mut self, frames: usize) {
        let ch = self.last_format.1.max(1);
        self.start = (self.start + frames * ch).min(self.scratch.len());
        self.replay = self.start < self.scratch.len();
    }

    /// Seeks to `secs` and returns the position actually reached.
    pub fn seek(&mut self, secs: f64) -> Result<f64> {
        let frame = (secs.max(0.0) * f64::from(self.rate)).round() as u64;
        let reached = self.seek_frame(frame)?;
        Ok(reached as f64 / f64::from(self.rate))
    }

    /// Seeks to exactly `frame` (counted at the file's own rate) and returns the frame reached.
    pub fn seek_frame(&mut self, frame: u64) -> Result<u64> {
        let rate = u64::from(self.rate.max(1));
        let time_base = self
            .format
            .tracks()
            .iter()
            .find(|t| t.id == self.track_id)
            .and_then(|t| t.time_base);

        let exact = time_base.is_some_and(|tb| tb.numer.get() == 1 && u64::from(tb.denom.get()) == rate);
        let to = if exact {
            SeekTo::Timestamp {
                ts: Timestamp::new(frame as i64),
                track_id: self.track_id,
            }
        } else {
            SeekTo::Time {
                time: Time::try_from_secs_f64(frame as f64 / rate as f64).unwrap_or(Time::ZERO),
                track_id: Some(self.track_id),
            }
        };
        let seeked = self.format.seek(SeekMode::Accurate, to).context("seek failed")?;
        self.decoder.reset();
        self.replay = false;
        self.start = 0;
        let to_frame = |ts: Timestamp| -> Option<u64> {
            if exact {
                return u64::try_from(ts.get()).ok();
            }
            let t = time_base?.calc_time(ts)?;
            Some((t.as_secs_f64() * rate as f64).round().max(0.0) as u64)
        };
        let actual = to_frame(seeked.actual_ts).unwrap_or(frame);
        let required = to_frame(seeked.required_ts).unwrap_or(frame);
        self.skip = required.saturating_sub(actual).min(rate * 10);
        Ok(actual + self.skip)
    }

    /// Returns whether the audio data matched the checksum stored in the file (FLAC).
    /// Returns None if the format or file does not provide a checksum.
    pub fn finalize(&mut self) -> Option<bool> {
        self.decoder.finalize().verify_ok
    }
}
