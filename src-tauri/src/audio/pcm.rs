//! Float samples to the integer formats DACs take, without changing a bit of the music (used by the
//! direct outputs: WASAPI exclusive on Windows, and integer devices through cpal).
//!
//! Dither is added only where a sample isn't already a whole step of the output format: music that
//! nothing changed (volume at full, no processing) comes out bit-for-bit, and music that was changed
//! (volume, EQ, normalization, resampling) is rounded with TPDF dither instead of plain truncation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// Bits per sample in the stream (16, 24 or 32).
    pub store_bits: usize,
    /// Bits that carry audio (24 in a 32-bit container: the low byte is zero).
    pub valid_bits: usize,
}

/// Triangular (TPDF) dither noise source.
#[derive(Default)]
pub struct Dither {
    state: u32,
}

impl Dither {
    /// A uniform random number in [0, 1) (xorshift: cheap and good enough for dither noise).
    fn uniform(&mut self) -> f32 {
        if self.state == 0 {
            self.state = 0x9E37_79B9;
        }
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state as f32 / u32::MAX as f32
    }

    /// Triangular noise in (-1, 1) LSB: the difference of two uniform values.
    pub fn next(&mut self) -> f32 {
        self.uniform() - self.uniform()
    }
}

/// `x` (in steps of the output format) rounded to a whole step: TPDF dither (±1 step) first when
/// it isn't one already.
fn round_dithered(x: f64, dither: &mut Dither) -> f64 {
    if x.fract() != 0.0 {
        (x + f64::from(dither.next())).round()
    } else {
        x
    }
}

/// Rounds float samples (in place) to whole steps of a `bits`-bit integer format, dithered where
/// needed and kept within range, for a device that converts floats by truncating (cpal).
pub fn requantize(samples: &mut [f32], bits: u32, dither: &mut Dither) {
    let scale = f64::from(1u32 << (bits.clamp(8, 24) - 1));
    for s in samples {
        let x = round_dithered(f64::from(*s) * scale, dither).clamp(-scale, scale - 1.0);
        *s = (x / scale) as f32;
    }
}

/// Converts float samples to the device's little-endian integer format.
///
/// Symphonia decodes N-bit audio as `value / 2^(N-1)`, so multiplying back gives the exact original
/// integers: a 24-bit file at full volume comes out bit-for-bit identical. Dither is added (16- and
/// 24-bit output) only when a value isn't already a whole number at the output's depth, so untouched
/// 16- and 24-bit music stays exact. 32-bit output holds every bit an f32 carries: no dither.
pub fn encode(input: &[f32], out: &mut [u8], f: Format, dither: &mut Dither) {
    match (f.store_bits, f.valid_bits) {
        (32, 24) => {
            for (s, o) in input.iter().zip(out.chunks_exact_mut(4)) {
                let v = round_dithered(*s as f64 * 8_388_608.0, dither)
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                o.copy_from_slice(&(v << 8).to_le_bytes());
            }
        }
        (32, _) => {
            for (s, o) in input.iter().zip(out.chunks_exact_mut(4)) {
                let v = (*s as f64 * 2_147_483_648.0)
                    .round()
                    .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                o.copy_from_slice(&v.to_le_bytes());
            }
        }
        (24, _) => {
            for (s, o) in input.iter().zip(out.chunks_exact_mut(3)) {
                let v = round_dithered(*s as f64 * 8_388_608.0, dither)
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                o.copy_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        _ => {
            for (s, o) in input.iter().zip(out.chunks_exact_mut(2)) {
                let v = round_dithered(*s as f64 * 32_768.0, dither).clamp(-32_768.0, 32_767.0) as i16;
                o.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    pub fn dither_is_added_for_fractional_values_only() {
        let mut dither = Dither::default();
        let exact = round_dithered(100.0, &mut dither);
        assert_eq!(exact, 100.0);
    }

    #[test]
    pub fn sixteen_bit_source_gets_no_dither() {
        let mut dither = Dither::default();
        let mut out = [0u8; 2];
        let sample = 16384.0 / 32768.0;
        encode(&[sample], &mut out, Format { store_bits: 16, valid_bits: 16 }, &mut dither);
        assert_eq!(i16::from_le_bytes(out), 16384);
    }

    #[test]
    pub fn twenty_four_bit_is_exact() {
        let mut dither = Dither::default();
        let mut out = [0u8; 4];
        let sample = 4194304.0 / 8388608.0;
        encode(&[sample], &mut out, Format { store_bits: 32, valid_bits: 24 }, &mut dither);
        let val = i32::from_le_bytes(out) >> 8;
        assert_eq!(val, 4194304);
    }

    #[test]
    pub fn thirty_two_bit_keeps_every_bit() {
        let mut dither = Dither::default();
        let mut out = [0u8; 4];
        let sample = 0.5f32;
        encode(&[sample], &mut out, Format { store_bits: 32, valid_bits: 32 }, &mut dither);
        assert_eq!(i32::from_le_bytes(out), 1073741824);
    }
}
