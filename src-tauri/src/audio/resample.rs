//! Streaming sample-rate conversion (FFT-based, from `rubato`) for when a song's rate
//! differs from the output device's rate. Only used when needed: matching rates pass through untouched.

use anyhow::Result;
use rubato::{Fft, FixedSync, Resampler as _, WindowFunction};

const BLOCK: usize = 1024;

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

pub struct Resampler {
    inner: Fft<f32>,
    channels: usize,
    pub rate_in: u32,
    pub rate_out: u32,
    pending: Vec<f32>,
    out: Vec<f32>,
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl Resampler {
    pub fn new(rate_in: u32, rate_out: u32, channels: usize) -> Result<Self> {
        let min_in = rate_in as usize / gcd(rate_in as usize, rate_out as usize).max(1);
        let chunk = (BLOCK.div_ceil(min_in.max(1)) * min_in.max(1)).max(min_in);
        Self::with_blocks(rate_in, rate_out, channels, chunk, 1)
    }

    fn with_blocks(rate_in: u32, rate_out: u32, channels: usize, chunk: usize, sub_chunks: usize) -> Result<Self> {
        let inner = Fft::<f32>::new_custom(
            rate_in as usize,
            rate_out as usize,
            chunk,
            sub_chunks,
            channels,
            WindowFunction::BlackmanHarris2,
            FixedSync::Input,
        )?;
        let skip = inner.output_delay();
        Ok(Self {
            inner,
            channels,
            rate_in,
            rate_out,
            pending: Vec::new(),
            out: Vec::new(),
            skip,
            frames_in: 0,
            frames_out: 0,
        })
    }

    pub fn matches(&self, rate_in: u32, rate_out: u32, channels: usize) -> bool {
        self.rate_in == rate_in && self.rate_out == rate_out && self.channels == channels
    }

    /// Feeds interleaved input; appends whatever output is ready to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<()> {
        self.pending.extend_from_slice(input);
        self.frames_in += (input.len() / self.channels) as u64;
        loop {
            let need = self.inner.input_frames_next();
            if self.pending.len() / self.channels < need {
                return Ok(());
            }
            self.run(need, out)?;
        }
    }

    /// Pushes out the remaining input and the filter tail (end of a stream).
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<()> {
        let expected =
            (self.frames_in as f64 * self.rate_out as f64 / self.rate_in as f64).round() as u64;
        let mut guard = 0;
        while self.frames_out < expected && guard < 16 {
            let have = (self.pending.len() / self.channels).min(self.inner.input_frames_next());
            self.run(have, out)?;
            guard += 1;
        }
        let extra = self.frames_out.saturating_sub(expected) as usize * self.channels;
        out.truncate(out.len().saturating_sub(extra));
        self.reset();
        Ok(())
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.pending.clear();
        self.skip = self.inner.output_delay();
        self.frames_in = 0;
        self.frames_out = 0;
    }

    fn run(&mut self, frames: usize, out: &mut Vec<f32>) -> Result<()> {
        use audioadapter_buffers::direct::InterleavedSlice;
        let need = self.inner.input_frames_next().max(frames);
        let mut buf = vec![0.0f32; need * self.channels];
        let copy_len = self.pending.len().min(buf.len());
        buf[..copy_len].copy_from_slice(&self.pending[..copy_len]);
        self.pending.drain(..copy_len);

        let in_adapter = InterleavedSlice::new(&buf, self.channels, need)?;
        let max_out = self.inner.output_frames_max();
        self.out.resize(max_out * self.channels, 0.0);
        let mut out_adapter = InterleavedSlice::new_mut(&mut self.out, self.channels, max_out)?;

        let (_, produced) = self.inner.process_into_buffer(&in_adapter, &mut out_adapter, None)?;
        let keep = produced * self.channels;
        let samples = &self.out[..keep];
        let drop_samples = (self.skip * self.channels).min(samples.len());
        self.skip -= drop_samples / self.channels;
        let valid = &samples[drop_samples..];
        out.extend_from_slice(valid);
        self.frames_out += (valid.len() / self.channels) as u64;
        Ok(())
    }
}
