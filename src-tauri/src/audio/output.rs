//! Audio output: the device stream and the real-time renderer that feeds it from the ring buffer.
//!
//! The renderer is shared by every backend. It applies volume with smooth gliding (so dragging the slider
//! never causes zipper noise), short 12 ms micro-fades (so pause, seek and skip never click or pop),
//! meters peak level, and keeps DACs awake with a ±1-LSB signal during gaps in exclusive mode.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use ringbuf::traits::{Consumer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

/// Length of the fade on pause/resume, seek and skip.
const FADE_MS: f32 = 12.0;
/// How long a volume change takes to glide to its new level (20ms pop suppression).
const VOLUME_RAMP_MS: f32 = 20.0;

pub struct Shared {
    pub playing: AtomicBool,
    pub volume: AtomicU32,
    pub fade_gain: AtomicU32,
    pub consumed: AtomicU64,
    pub flush: AtomicBool,
    pub failed: AtomicBool,
    pub peak: AtomicU32,
    pub underrun: AtomicU64,
    pub clipped: AtomicU64,
}

impl Shared {
    pub fn new(volume: f32) -> Self {
        Self {
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(volume.to_bits()),
            fade_gain: AtomicU32::new(1f32.to_bits()),
            consumed: AtomicU64::new(0),
            flush: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            peak: AtomicU32::new(0),
            underrun: AtomicU64::new(0),
            clipped: AtomicU64::new(0),
        }
    }

    pub fn set_fade(&self, gain: f32) {
        self.fade_gain.store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::AcqRel))
    }
}

/// Real-time side: pulls samples from the ring and writes device buffers.
pub struct Renderer {
    cons: HeapCons<f32>,
    shared: Arc<Shared>,
    channels: usize,
    fade: f32,
    fade_step: f32,
    gain: f32,
    gain_target: f32,
    gain_step: f32,
    gain_frames: f32,
    gain_set: bool,
    clamp: bool,
    lsb: f32,
    settle: usize,
}

impl Renderer {
    pub fn new(
        cons: HeapCons<f32>,
        shared: Arc<Shared>,
        channels: usize,
        rate: u32,
        clamp: bool,
    ) -> Self {
        let fade_step = 1.0 / (rate as f32 * FADE_MS / 1000.0).max(1.0);
        Self {
            cons,
            shared,
            channels,
            fade: 0.0,
            fade_step,
            gain: 1.0,
            gain_target: 1.0,
            gain_step: 0.0,
            gain_frames: (rate as f32 * VOLUME_RAMP_MS / 1000.0).max(1.0),
            gain_set: false,
            clamp,
            lsb: 1.0 / 8_388_608.0,
            settle: rate as usize / 5,
        }
    }

    pub fn with_bits(mut self, bits: u32) -> Self {
        self.lsb = 1.0 / (1u32 << (bits.clamp(8, 24) - 1)) as f32;
        self
    }

    /// Fills `out` (interleaved f32) with the next audio.
    pub fn render(&mut self, out: &mut [f32]) {
        let music = self.render_music(out);
        if self.clamp {
            let lsb = self.lsb;
            for (i, s) in out[music..].iter_mut().enumerate() {
                if *s == 0.0 {
                    *s = if (i / self.channels) % 2 == 0 { lsb } else { -lsb };
                }
            }
        }
    }

    fn render_music(&mut self, out: &mut [f32]) -> usize {
        if self.shared.flush.load(Ordering::Acquire) {
            let n = self.pop_into(out);
            self.ramp(&mut out[..n], 0.0);
            self.apply_volume(&mut out[..n]);
            out[n..].fill(0.0);
            if self.fade == 0.0 || n < out.len() {
                use ringbuf::traits::Consumer;
                self.cons.clear();
                self.fade = 0.0;
                self.settle = self.settle.max(self.fade_frames() * 16);
                self.shared.flush.store(false, Ordering::Release);
            }
            self.finish(out, n);
            return if self.fade == 0.0 { 0 } else { n };
        }
        let playing = self.shared.playing.load(Ordering::Relaxed);
        if !playing && self.fade == 0.0 {
            out.fill(0.0);
            self.gain = self.target_gain();
            self.gain_target = self.gain;
            return 0;
        }
        let n = self.pop_into(out);
        let frames = out.len() / self.channels;
        if self.settle > 0 {
            self.settle = self.settle.saturating_sub(frames);
        } else if playing && n < out.len() {
            self.shared
                .underrun
                .fetch_add(((out.len() - n) / self.channels) as u64, Ordering::Relaxed);
        }
        out[n..].fill(0.0);
        let target = if playing { 1.0 } else { 0.0 };
        self.ramp(&mut out[..n], target);
        self.apply_volume(&mut out[..n]);
        self.finish(out, n);
        n
    }

    fn target_gain(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
            * f32::from_bits(self.shared.fade_gain.load(Ordering::Relaxed))
    }

    fn apply_volume(&mut self, samples: &mut [f32]) {
        let target = self.target_gain();
        if !self.gain_set {
            self.gain_set = true;
            self.gain = 0.0;
            self.gain_target = target;
            self.gain_step = target / self.gain_frames;
        }
        if target != self.gain_target {
            self.gain_target = target;
            self.gain_step = (target - self.gain) / self.gain_frames;
        }
        if self.gain == self.gain_target {
            let g = self.gain;
            if g != 1.0 {
                samples.iter_mut().for_each(|s| *s *= g);
            }
            return;
        }
        for frame in samples.chunks_exact_mut(self.channels) {
            if self.gain != self.gain_target {
                self.gain += self.gain_step;
                let past = if self.gain_step > 0.0 {
                    self.gain >= self.gain_target
                } else {
                    self.gain <= self.gain_target
                };
                if past {
                    self.gain = self.gain_target;
                }
            }
            let g = self.gain;
            frame.iter_mut().for_each(|s| *s *= g);
        }
    }

    fn fade_frames(&self) -> usize {
        (1.0 / self.fade_step) as usize
    }

    pub fn buffered_frames(&self) -> usize {
        use ringbuf::traits::Observer;
        self.cons.occupied_len() / self.channels.max(1)
    }

    fn pop_into(&mut self, out: &mut [f32]) -> usize {
        let n = self.cons.pop_slice(out);
        (n / self.channels) * self.channels
    }

    fn ramp(&mut self, samples: &mut [f32], target: f32) {
        if self.fade == target {
            if target == 0.0 {
                samples.fill(0.0);
            }
            return;
        }
        for frame in samples.chunks_exact_mut(self.channels) {
            self.fade = if target > self.fade {
                (self.fade + self.fade_step).min(target)
            } else {
                (self.fade - self.fade_step).max(target)
            };
            frame.iter_mut().for_each(|s| *s *= self.fade);
        }
    }

    fn finish(&mut self, out: &mut [f32], popped: usize) {
        let peak = out[..popped].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.shared.peak.fetch_max(peak.to_bits(), Ordering::AcqRel);
        if self.clamp && peak > 1.0 {
            let over = out.iter().filter(|s| s.abs() > 1.0).count();
            self.shared.clipped.fetch_add(over as u64, Ordering::Relaxed);
            out.iter_mut().for_each(|s| *s = s.clamp(-1.0, 1.0));
        }
        self.shared
            .consumed
            .fetch_add((popped / self.channels) as u64, Ordering::AcqRel);
    }
}

pub enum Backend {
    Shared(#[allow(dead_code)] cpal::Stream),
    #[cfg(windows)]
    Exclusive(#[allow(dead_code)] super::exclusive::ExclusiveStream),
}

pub struct Output {
    pub backend: Backend,
    pub prod: HeapProd<f32>,
    pub rate: u32,
    pub channels: usize,
    pub device: String,
    pub bits: Option<u32>,
}

impl Output {
    pub fn exclusive(&self) -> bool {
        #[cfg(windows)]
        return matches!(self.backend, Backend::Exclusive(_));
        #[cfg(not(windows))]
        false
    }
}

fn rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I24 => 2,
        SampleFormat::F64 => 3,
        SampleFormat::I16 => 4,
        SampleFormat::U16 => 5,
        _ => 9,
    }
}

fn best_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    let default = device.default_output_config()?;
    let supported = device
        .supported_output_configs()?
        .filter(|c| c.channels() == default.channels() && c.min_sample_rate() <= default.sample_rate() && default.sample_rate() <= c.max_sample_rate())
        .filter(|c| rank(c.sample_format()) < 9)
        .min_by_key(|c| rank(c.sample_format()))
        .map(|c| c.with_sample_rate(default.sample_rate()));

    Ok(match supported {
        Some(b) if rank(b.sample_format()) < rank(default.sample_format()) => b,
        _ => default,
    })
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Default Audio Device".into())
}

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if let Ok(desc) = d.description() {
                let name = desc.name().to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

pub fn default_output_device_name() -> Option<String> {
    let host = cpal::default_host();
    host.default_output_device()
        .and_then(|d| d.description().map(|desc| desc.name().to_string()).ok())
}

pub fn open_shared(shared: Arc<Shared>, device_name_opt: Option<&str>) -> Result<Output> {
    let host = cpal::default_host();
    let device = match device_name_opt {
        Some(name) => host
            .output_devices()?
            .find(|d| d.description().map(|desc| desc.name() == name).unwrap_or(false))
            .ok_or_else(|| anyhow!("Device '{}' not found", name))?,
        None => host.default_output_device().ok_or_else(|| anyhow!("No default output device"))?,
    };
    let supported = best_config(&device)?;
    log::info!("Opened CPAL shared output: {} at {} Hz", supported.sample_format(), supported.sample_rate());
    let dev_name = device_name(&device);
    let (stream, prod, rate, channels) = start(&device, supported, shared)?;
    Ok(Output {
        backend: Backend::Shared(stream),
        prod,
        rate,
        channels,
        device: dev_name,
        bits: None,
    })
}

fn start(
    device: &cpal::Device,
    supported: cpal::SupportedStreamConfig,
    shared: Arc<Shared>,
) -> Result<(cpal::Stream, HeapProd<f32>, u32, usize)> {
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let (rate, channels) = (config.sample_rate, config.channels as usize);
    let float = matches!(format, SampleFormat::F32 | SampleFormat::F64);
    let bits = match format {
        SampleFormat::I16 | SampleFormat::U16 => Some(16),
        SampleFormat::I24 => Some(24),
        _ => None,
    };
    let (prod, cons) = HeapRb::<f32>::new((rate as usize / 2) * channels).split();
    let mut renderer = Renderer::new(cons, shared.clone(), channels, rate, !float);
    if let Some(b) = bits {
        renderer = renderer.with_bits(b);
    }
    let stream = match format {
        SampleFormat::F32 => build_stream::<f32>(device, config, renderer, shared, bits),
        SampleFormat::F64 => build_stream::<f64>(device, config, renderer, shared, bits),
        SampleFormat::I16 => build_stream::<i16>(device, config, renderer, shared, bits),
        SampleFormat::I32 => build_stream::<i32>(device, config, renderer, shared, bits),
        SampleFormat::I24 => build_stream::<cpal::I24>(device, config, renderer, shared, bits),
        SampleFormat::U16 => build_stream::<u16>(device, config, renderer, shared, bits),
        other => return Err(anyhow!("Unsupported device sample format: {other}")),
    }?;
    stream.play()?;
    Ok((stream, prod, rate, channels))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
    shared: Arc<Shared>,
    bits: Option<u32>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let mut scratch = vec![0f32; 16_384];
    let mut dither = super::pcm::Dither::default();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            scratch.resize(data.len(), 0.0);
            renderer.render(&mut scratch);
            if let Some(b) = bits {
                super::pcm::requantize(&mut scratch, b, &mut dither);
            }
            for (dst, &src) in data.iter_mut().zip(&scratch) {
                *dst = T::from_sample(src);
            }
        },
        move |err| {
            log::warn!("Audio stream error: {err}");
            shared.failed.store(true, Ordering::Release);
        },
        None,
    )?;
    Ok(stream)
}
