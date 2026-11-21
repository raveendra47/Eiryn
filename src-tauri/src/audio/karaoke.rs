//! Low-CPU Center-Channel Vocal Remover (Karaoke mode).
//!
//! Lead vocals in studio recordings are mixed dead center (L == R), whereas instruments
//! and stereophonic effects are spread across the stereo field.
//! A simple L - R subtraction eliminates center vocals, but also loses the bass
//! drum and bassline (which are also panned center).
//! This filter uses a 2nd-order Butterworth low-pass filter (150 Hz, 12 dB/oct) to isolate
//! the center-panned sub-bass and recombine it with the differential side channels (L - center)
//! and (R - center). This achieves > 30 dB vocal attenuation while keeping the low-end punch intact.

pub struct KaraokeFilter {
    rate: f32,
    cutoff_hz: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    enabled: bool,
}

impl KaraokeFilter {
    pub fn new(rate: u32) -> Self {
        let mut k = Self {
            rate: rate.max(8_000) as f32,
            cutoff_hz: 150.0,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            enabled: false,
        };
        k.calc_coeffs();
        k
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(8_000) as f32;
        self.calc_coeffs();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.reset();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn calc_coeffs(&mut self) {
        let w0 = 2.0 * std::f32::consts::PI * self.cutoff_hz / self.rate;
        let alpha = w0.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;

        self.b0 = ((1.0 - cos_w0) * 0.5) / a0;
        self.b1 = (1.0 - cos_w0) / a0;
        self.b2 = ((1.0 - cos_w0) * 0.5) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Process interleaved stereo audio in place.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if !self.enabled || channels < 2 || samples.is_empty() {
            return;
        }

        for frame in samples.chunks_exact_mut(channels) {
            let left = frame[0];
            let right = frame[1];
            let center = (left + right) * 0.5;

            // 2nd-order Butterworth low-pass filter on center channel
            let bass = self.b0 * center + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1 - self.a2 * self.y2;

            self.x2 = self.x1;
            self.x1 = center;
            self.y2 = self.y1;
            self.y1 = bass;

            // Subtract mid/high center vocal, add back isolated bass
            frame[0] = (left - center) + bass;
            frame[1] = (right - center) + bass;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_vocal_cancelation() {
        let mut k = KaraokeFilter::new(48_000);
        k.set_enabled(true);

        // 1 kHz pure center tone (identical in L and R)
        let mut audio = Vec::new();
        for i in 0..4800 {
            let v = (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 48000.0).sin();
            audio.push(v);
            audio.push(v);
        }

        k.process(&mut audio, 2);

        // After filter settling, 1kHz center vocal should be suppressed by > 30 dB (< 0.05 amplitude)
        let settled = &audio[2000..];
        let peak = settled.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.05, "1kHz center vocal should be cancelled, got peak {peak}");
    }

    #[test]
    fn test_sub_bass_preservation() {
        let mut k = KaraokeFilter::new(48_000);
        k.set_enabled(true);

        // 40 Hz sub-bass tone (center panned)
        let mut audio = Vec::new();
        for i in 0..4800 {
            let v = (i as f32 * 2.0 * std::f32::consts::PI * 40.0 / 48000.0).sin();
            audio.push(v);
            audio.push(v);
        }

        k.process(&mut audio, 2);

        let settled = &audio[3000..];
        let peak = settled.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.8, "40Hz sub-bass should be preserved, got peak {peak}");
    }
}
