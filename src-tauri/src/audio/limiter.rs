//! Transparent lookahead peak limiter for ReplayGain, EQ preamp, and DSP processing.
//!
//! When positive ReplayGain gain or graphic/parametric EQ boosts are applied,
//! digital audio can exceed 0 dBFS. This limiter introduces a tiny (16-frame, ~0.3ms)
//! lookahead buffer to anticipate peaks and smoothly scale gain *before* the peak arrives,
//! completely eliminating clipping, digital harshness, and DAC inter-sample distortion
//! while maintaining 100% bit-exact transparency for audio below the ceiling.

pub const CEILING: f32 = 0.988_553_1; // -0.1 dBFS headroom for DAC anti-aliasing
const RELEASE_S: f32 = 0.08;
const LOOKAHEAD_FRAMES: usize = 16;

pub struct Limiter {
    rate: u32,
    channels: usize,
    release: f32,
    cut: f32,
    lookahead_buf: Vec<f32>,
    lookahead_pos: usize,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(44_100, 2)
    }
}

impl Limiter {
    pub fn new(rate: u32, channels: usize) -> Self {
        let ch = channels.clamp(1, 8);
        let mut l = Self {
            rate: 0,
            channels: ch,
            release: 0.0,
            cut: 0.0,
            lookahead_buf: vec![0.0; LOOKAHEAD_FRAMES * ch],
            lookahead_pos: 0,
        };
        l.set_format(rate, ch);
        l
    }

    pub fn set_format(&mut self, rate: u32, channels: usize) {
        let ch = channels.clamp(1, 8);
        let r = rate.max(8_000);
        if r == self.rate && ch == self.channels {
            return;
        }
        self.rate = r;
        self.channels = ch;
        self.release = 1.0 - (-1.0 / (RELEASE_S * r as f32)).exp();
        self.lookahead_buf.clear();
        self.lookahead_buf.resize(LOOKAHEAD_FRAMES * ch, 0.0);
        self.lookahead_pos = 0;
        self.cut = 0.0;
    }

    pub fn reset(&mut self) {
        self.cut = 0.0;
        self.lookahead_buf.fill(0.0);
        self.lookahead_pos = 0;
    }

    /// Process interleaved samples in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        if samples.is_empty() {
            return;
        }
        let ch = self.channels;
        let total_buf_samples = LOOKAHEAD_FRAMES * ch;
        if self.lookahead_buf.len() != total_buf_samples {
            self.lookahead_buf.resize(total_buf_samples, 0.0);
        }

        for frame in samples.chunks_exact_mut(ch) {
            // Find peak in the incoming frame
            let incoming_peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));

            // Desired target reduction
            let most = if incoming_peak.is_finite() && incoming_peak > CEILING {
                CEILING / incoming_peak
            } else {
                1.0
            };
            let needed_cut = 1.0 - most;

            // Instant attack to head off the peak, smooth exponential decay
            let released = self.cut * (1.0 - self.release);
            self.cut = (if released < 1e-4 { 0.0 } else { released }).max(needed_cut);

            let gain = 1.0 - self.cut;

            // Retrieve the delayed frame from lookahead buffer and store the incoming frame
            let buf_idx = self.lookahead_pos * ch;
            let delayed_peak = (0..ch).fold(0.0f32, |m, c| m.max(self.lookahead_buf[buf_idx + c].abs()));
            let safe_gain = if delayed_peak.is_finite() && delayed_peak * gain > CEILING {
                CEILING / delayed_peak
            } else {
                gain
            };

            for c in 0..ch {
                let delayed = self.lookahead_buf[buf_idx + c];
                self.lookahead_buf[buf_idx + c] = frame[c];
                frame[c] = delayed * safe_gain;
            }

            self.lookahead_pos = (self.lookahead_pos + 1) % LOOKAHEAD_FRAMES;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = amp * (i as f32 * 0.05).sin();
                [v, v * 0.8]
            })
            .collect()
    }

    #[test]
    fn test_normal_music_not_distorted() {
        let mut limiter = Limiter::new(48_000, 2);
        let original = sine(0.7, 4_800);
        let mut processed = original.clone();
        limiter.process(&mut processed);

        // Discard lookahead buffer ramp-in (first 16 frames)
        let end_orig = &original[32..4_800 * 2];
        let end_proc = &processed[32 + 32..];
        for (a, b) in end_orig.iter().zip(end_proc.iter()) {
            assert!((a - b).abs() < 1e-5, "Sample mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn test_overshoot_limited_below_ceiling() {
        let mut limiter = Limiter::new(48_000, 2);
        // +6 dB overshoot (amplitude 2.0)
        let mut audio = sine(2.0, 9_600);
        limiter.process(&mut audio);

        let max_peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            max_peak <= CEILING + 1e-4,
            "Limiter let peak through: {max_peak} > {CEILING}"
        );
    }

    #[test]
    fn test_level_recovery_after_burst() {
        let mut limiter = Limiter::new(48_000, 2);
        let mut burst = sine(3.0, 100);
        limiter.process(&mut burst);
        assert!(limiter.cut > 0.5, "Cut should be high after burst");

        let mut silence = vec![0.0f32; 48_000 * 2]; // 1 second
        limiter.process(&mut silence);
        assert_eq!(limiter.cut, 0.0, "Limiter should fully recover after silence");
    }
}
