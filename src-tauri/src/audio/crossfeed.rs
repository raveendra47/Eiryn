//! Headphone crossfeed (Bauer stereophonic-to-binaural, after libbs2b, MIT).
//!
//! On headphones each ear hears only its own channel, which makes hard-panned mixes tiring.
//! Crossfeed feeds a low-passed, slightly delayed copy of each channel to the other ear, like
//! speakers in a room do. It's a preference, not a correction, so it's off by default.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrossfeedLevel {
    /// 700 Hz, 4.5 dB — subtle (bs2b default).
    #[default]
    Default,
    /// 700 Hz, 6 dB — Chu Moy's headphone amplifier.
    Cmoy,
    /// 650 Hz, 9.5 dB — Jan Meier's, strongest.
    Meier,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossfeedSettings {
    pub enabled: bool,
    pub level: CrossfeedLevel,
}

pub struct Crossfeed {
    settings: CrossfeedSettings,
    rate: u32,
    a0_lo: f64,
    b1_lo: f64,
    a0_hi: f64,
    a1_hi: f64,
    b1_hi: f64,
    gain: f64,
    /// Levels of the crossfed low-pass and of what the direct path takes out.
    g_lo: f64,
    g_hi: f64,
    lo: [f64; 2],
    hi: [f64; 2],
    last_in: [f64; 2],
}

impl Crossfeed {
    pub fn new(settings: CrossfeedSettings, rate: u32) -> Self {
        let mut c = Self {
            settings,
            rate,
            a0_lo: 0.0,
            b1_lo: 0.0,
            a0_hi: 0.0,
            a1_hi: 0.0,
            b1_hi: 0.0,
            gain: 1.0,
            g_lo: 0.0,
            g_hi: 0.0,
            lo: [0.0; 2],
            hi: [0.0; 2],
            last_in: [0.0; 2],
        };
        c.init();
        c
    }

    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn set_settings(&mut self, settings: CrossfeedSettings) {
        if settings != self.settings {
            let switched_on = settings.enabled && !self.settings.enabled;
            let (g_lo, g_hi) = (self.g_lo, self.g_hi);
            self.settings = settings;
            self.init();
            if switched_on {
                self.reset();
            } else {
                for c in 0..2 {
                    if g_lo > 0.0 {
                        self.lo[c] *= self.g_lo / g_lo;
                    }
                    if g_hi > 0.0 {
                        let x = self.last_in[c];
                        self.hi[c] = x - (x - self.hi[c]) * self.g_hi / g_hi;
                    }
                }
            }
        }
    }

    pub fn set_rate(&mut self, rate: u32) {
        if rate != self.rate {
            self.rate = rate;
            self.init();
            self.reset();
        }
    }

    fn init(&mut self) {
        let (fc_lo, level) = match self.settings.level {
            CrossfeedLevel::Default => (700.0, 4.5),
            CrossfeedLevel::Cmoy => (700.0, 6.0),
            CrossfeedLevel::Meier => (650.0, 9.5),
        };
        let gb_lo = level * -5.0 / 6.0 - 3.0;
        let gb_hi = level / 6.0 - 3.0;
        let g_lo = 10f64.powf(gb_lo / 20.0);
        let g_hi = 1.0 - 10f64.powf(gb_hi / 20.0);
        let fc_hi = fc_lo * 2f64.powf((gb_lo - 20.0 * g_hi.log10()) / 12.0);
        let rate = if self.rate > 0 { self.rate as f64 } else { 44100.0 };
        let x = (-2.0 * std::f64::consts::PI * fc_lo / rate).exp();
        self.b1_lo = x;
        self.a0_lo = g_lo * (1.0 - x);
        let x = (-2.0 * std::f64::consts::PI * fc_hi / rate).exp();
        self.b1_hi = x;
        self.a0_hi = 1.0 - g_hi * (1.0 - x);
        self.a1_hi = -x;
        self.gain = 1.0 / (1.0 - g_hi + g_lo);
        self.g_lo = g_lo;
        self.g_hi = g_hi;
    }

    pub fn reset(&mut self) {
        self.lo = [0.0; 2];
        self.hi = [0.0; 2];
        self.last_in = [0.0; 2];
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if !self.settings.enabled || channels != 2 {
            return;
        }
        for frame in samples.chunks_exact_mut(2) {
            let input = [frame[0] as f64, frame[1] as f64];
            for c in 0..2 {
                self.lo[c] = self.a0_lo * input[c] + self.b1_lo * self.lo[c];
                self.hi[c] =
                    self.a0_hi * input[c] + self.a1_hi * self.last_in[c] + self.b1_hi * self.hi[c];
            }
            self.last_in = input;
            frame[0] = ((self.hi[0] + self.lo[1]) * self.gain) as f32;
            frame[1] = ((self.hi[1] + self.lo[0]) * self.gain) as f32;
        }
    }
}
