//! Equalizer built from RBJ "Audio EQ Cookbook" biquads, processed in f64.
//!
//! Two modes:
//! - **Graphic**: 10 ISO bands (32 Hz – 16 kHz), peaking filters at Q 1.41.
//! - **Parametric**: arbitrary peaking / shelf / pass filters with individual Q values.
//!
//! The preamp is automatic: the combined response of all filters is measured and the level
//! lowered by exactly the largest boost, so EQ never pushes audio past full scale.
//! A manual preamp is applied on top of that.

use serde::{Deserialize, Serialize};

pub const EQ_BANDS: [f64; 10] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
const GRAPHIC_Q: f64 = 1.41;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub kind: FilterKind,
    pub freq: f64,
    pub gain: f64,
    pub q: f64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EqSettings {
    pub enabled: bool,
    #[serde(default = "default_mode")]
    pub mode: String,
    pub preamp: f64,
    pub gains: [f64; 10],
    pub filters: Vec<Filter>,
}

fn default_mode() -> String {
    "graphic".to_string()
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "graphic".to_string(),
            preamp: 0.0,
            gains: [0.0; 10],
            filters: Vec::new(),
        }
    }
}

impl EqSettings {
    pub fn is_transparent(&self) -> bool {
        !self.enabled || (self.preamp == 0.0 && self.gains.iter().all(|&g| g == 0.0) && self.filters.iter().all(|f| !f.enabled || f.gain == 0.0))
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct Coeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coeffs {
    fn design(f: &Filter, rate: f64) -> Self {
        let a = 10f64.powf(f.gain / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f.freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * f.q.max(0.01));
        let (b0, b1, b2, a0, a1, a2) = match f.kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            FilterKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn magnitude_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / rate;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let (nr, ni) = (
            self.b0 + self.b1 * c1 + self.b2 * c2,
            self.b1 * s1 + self.b2 * s2,
        );
        let (dr, di) = (
            1.0 + self.a1 * c1 + self.a2 * c2,
            self.a1 * s1 + self.a2 * s2,
        );
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

pub struct Equalizer {
    settings: EqSettings,
    channels: usize,
    rate: f64,
    preamp: f64,
    preamp_now: f64,
    preamp_step: f64,
    filters: Vec<Coeffs>,
    states: Vec<State>,
}

impl Equalizer {
    pub fn new(channels: usize, rate: u32) -> Self {
        let mut eq = Self {
            settings: EqSettings::default(),
            channels,
            rate: rate as f64,
            preamp: 1.0,
            preamp_now: 1.0,
            preamp_step: 0.0,
            filters: Vec::new(),
            states: Vec::new(),
        };
        eq.rebuild();
        eq.preamp_now = eq.preamp;
        eq
    }

    pub fn set_format(&mut self, channels: usize, rate: u32) {
        if channels != self.channels || rate as f64 != self.rate {
            self.channels = channels;
            self.rate = rate as f64;
            self.rebuild();
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.settings.enabled != enabled {
            self.settings.enabled = enabled;
            self.rebuild();
        }
    }

    pub fn set_parametric_filters(&mut self, filters: Vec<Filter>) {
        self.settings.filters = filters;
        self.settings.mode = "parametric".to_string();
        self.rebuild();
    }

    pub fn set_mode(&mut self, mode: &str) {
        if self.settings.mode != mode {
            self.settings.mode = mode.to_string();
            self.rebuild();
        }
    }

    pub fn set_settings(&mut self, settings: EqSettings) {
        self.settings = settings;
        self.rebuild();
    }

    pub fn set_band(&mut self, index: u8, gain_db: f32) {
        if let Some(slot) = self.settings.gains.get_mut(index as usize) {
            *slot = gain_db as f64;
            self.settings.mode = "graphic".to_string();
            self.rebuild();
        }
    }

    pub fn set_preamp(&mut self, gain_db: f32) {
        self.settings.preamp = gain_db as f64;
        self.rebuild();
    }

    pub fn reset(&mut self) {
        self.states.iter_mut().for_each(|s| *s = State::default());
        self.preamp_now = self.preamp;
    }

    fn rebuild(&mut self) {
        let mut active: Vec<Filter> = Vec::new();
        if self.settings.mode == "parametric" {
            // Parametric mode: evaluate custom filters
            for f in &self.settings.filters {
                if f.enabled && f.gain != 0.0 {
                    active.push(*f);
                }
            }
        } else {
            // Graphic 10-band
            for (i, &f) in EQ_BANDS.iter().enumerate() {
                let g = self.settings.gains[i];
                if g != 0.0 {
                    active.push(Filter {
                        kind: FilterKind::Peak,
                        freq: f,
                        gain: g,
                        q: GRAPHIC_Q,
                        enabled: true,
                    });
                }
            }
        }

        let nyquist_ok = self.rate * 0.45;
        let valid_filters: Vec<Filter> = active
            .into_iter()
            .filter(|f| f.freq > 0.0 && f.freq < nyquist_ok)
            .collect();

        self.filters = valid_filters.iter().map(|f| Coeffs::design(f, self.rate)).collect();

        // Calculate automatic headroom to prevent digital clipping
        let nyquist = self.rate / 2.0 * 0.999;
        let top = if self.filters.is_empty() {
            0.0
        } else {
            (0..=600)
                .map(|i| 10.0 * (nyquist / 10.0).powf(i as f64 / 600.0))
                .map(|f| self.filters.iter().map(|c| c.magnitude_db(f, self.rate)).sum::<f64>())
                .fold(f64::MIN, f64::max)
        };

        let total = (self.settings.preamp - top.max(0.0)).min(-top);
        let target = 10f64.powf(total / 20.0);
        if target != self.preamp {
            self.preamp = target;
            self.preamp_step = (target - self.preamp_now) / (self.rate * 0.01).max(1.0);
        }

        self.states.resize(self.filters.len() * self.channels, State::default());
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if self.settings.is_transparent() || self.channels == 0 || self.filters.is_empty() {
            self.preamp_now = self.preamp;
            return;
        }
        let ch = self.channels;
        for frame in samples.chunks_exact_mut(ch) {
            if self.preamp_now != self.preamp {
                self.preamp_now += self.preamp_step;
                let past = if self.preamp_step > 0.0 {
                    self.preamp_now >= self.preamp
                } else {
                    self.preamp_now <= self.preamp
                };
                if past || self.preamp_step == 0.0 {
                    self.preamp_now = self.preamp;
                }
            }
            let pre = self.preamp_now;
            for (c, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample as f64 * pre;
                for (b, f) in self.filters.iter().enumerate() {
                    let s = &mut self.states[b * ch + c];
                    let y = f.b0 * x + f.b1 * s.x1 + f.b2 * s.x2 - f.a1 * s.y1 - f.a2 * s.y2;
                    s.x2 = s.x1;
                    s.x1 = x;
                    s.y2 = s.y1;
                    s.y1 = y;
                    x = y;
                }
                *sample = x as f32;
            }
        }
    }
}
