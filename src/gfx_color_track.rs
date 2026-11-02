//! Retail 1.4.2 GFX controller 108, not the later floating interpolation path.
//! 0x6ec5dd builds 128 packed entries/segment; 0x6ec190 indexes that table.
//! Start/end are seconds; segment lengths are milliseconds. Callers preserve
//! their last keypoint color when sample returns None (controller inactive).
//!
//! Construction and sampling precision
//!
//! Both run in the same FPU context in the supplied 1.4.2 executable:
//! * the executable never creates a thread. No thread-creation import is
//!   referenced anywhere in `.text`: there is no call site (direct or through
//!   the IAT slot) for `_beginthreadex`/`_beginthread` (`0x4d54ec`/`0x4d54f0`)
//!   and `CreateThread` is not imported at all. Loading is frame-budgeted on the
//!   update thread instead - `0x5b1a00` returns after one pending root - so
//!   GFX table construction happens on the same thread that samples the table.
//! * that thread runs with the device's FPU control word. `_controlfp` is
//!   imported (`0x4d5564`) and never called, and the only `fldcw`/`fnstcw`
//!   instructions in the image are inside the static CRT math helpers
//!   (`0x7fdcb7`, `0x8001e5`, `0x80c878`, `0x863350`), which save and restore
//!   the caller's word, so the application never changes it. The device context
//!   is therefore the one D3D8 installs: PC24, the same premise the gamma path
//!   already had to adopt (`0x7b9bc0` capture).
//! * `0x6ec5dd` builds the table with x87 `fmul`/`fsub` only, so under PC24 every
//!   intermediate rounds to a 24-bit mantissa - exactly what f32 arithmetic
//!   computes. `Precision::Pc24` is therefore the native construction mode, and
//!   `Precision::Extended` remains only as the independent comparison oracle
//!   that shows the two tables are not interchangeable.
//!
//! Residual exposure: a GFX root loaded before the D3D8 device exists would be
//! built under the startup word (extended). `construction_divergences` bounds
//! exactly which entries that could change, and the survey prints the count for
//! the real corpus.

/// How the 128-entry segment table is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// Exact rational over the stored f32 reciprocal 0x3c010204, i.e. the
    /// table an extended-precision (x87 control word 0x027f) build produces.
    Extended,
    /// Every instruction rounded to f32 (PC24), the table a 24-bit x87 build
    /// produces: `a*(1-f) + b*f` with `f = f32(i * reciprocal)`.
    Pc24,
}

/// Active construction mode: the native PC24 table (see the module notes).
/// `Precision::Extended` stays available for the divergence diagnostic only.
pub const CONSTRUCTION: Precision = Precision::Pc24;

/// 0x3c010204 = 2113665/268435456, the native stored f32 reciprocal.
const RECIPROCAL_BITS: u32 = 0x3c010204;
const RECIPROCAL: f32 = f32::from_bits(RECIPROCAL_BITS);
const DENOMINATOR: u64 = 268_435_456;

/// One packed channel byte for interior table index `i` (1..=126).
fn channel(from: u32, to: u32, i: usize, precision: Precision) -> u32 {
    match precision {
        Precision::Extended => {
            let n = i as u64 * 2_113_665;
            ((from as u64 * (DENOMINATOR - n) + to as u64 * n) / DENOMINATOR) as u32
        }
        Precision::Pc24 => {
            let f = i as f32 * RECIPROCAL;
            ((from as f32) * (1. - f) + (to as f32) * f) as u32
        }
    }
}

fn build(from: u32, to: u32, precision: Precision) -> [u32; 128] {
    let mut table = [0; 128];
    table[0] = from;
    table[127] = to;
    for (i, value) in table.iter_mut().enumerate().take(127).skip(1) {
        let mut packed = 0;
        for shift in [0, 8, 16, 24] {
            packed |= channel((from >> shift) & 255, (to >> shift) & 255, i, precision) << shift;
        }
        *value = packed;
    }
    table
}

#[derive(Debug, Clone)]
pub struct ColorTrack {
    start: f32,
    end: f32,
    ends: Vec<(u32, u32)>,
    segments: Vec<(f32, [u32; 128])>,
}
impl ColorTrack {
    pub fn new(
        start: f32,
        end: f32,
        original: u32,
        destinations: &[(u32, f32)],
    ) -> Result<Self, String> {
        Self::new_with_precision(start, end, original, destinations, CONSTRUCTION)
    }
    /// Explicit construction mode; used by the precision comparison and by any
    /// future load-thread evidence that selects the other mode.
    pub fn new_with_precision(
        start: f32,
        end: f32,
        original: u32,
        destinations: &[(u32, f32)],
        precision: Precision,
    ) -> Result<Self, String> {
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.
            || destinations.is_empty()
            || destinations.len() > 10
        {
            return Err("unsupported GFX color controller bounds/count".into());
        }
        let mut from = original;
        let mut ends = Vec::with_capacity(destinations.len());
        let mut segments = Vec::with_capacity(destinations.len());
        for &(to, duration) in destinations {
            if !duration.is_finite() || duration <= 0. {
                return Err("invalid GFX color segment duration".into());
            }
            segments.push((duration, build(from, to, precision)));
            ends.push((from, to));
            from = to;
        }
        Ok(Self {
            start,
            end,
            ends,
            segments,
        })
    }
    /// Interior entries whose two candidate construction modes disagree. The
    /// survey diagnostic prints this for the actual corpus; a nonzero count is
    /// why the mode may not be chosen silently.
    pub fn construction_divergences(&self) -> usize {
        let mut count = 0;
        for (from, to) in &self.ends {
            // Compare built packed tables, not single channels: one differing
            // channel byte makes the whole entry distinct.
            let extended = build(*from, *to, Precision::Extended);
            let pc24 = build(*from, *to, Precision::Pc24);
            count += (1..127).filter(|i| extended[*i] != pc24[*i]).count();
        }
        count
    }
    pub fn sample(&self, keypoint_seconds: f32) -> Option<u32> {
        if !keypoint_seconds.is_finite()
            || keypoint_seconds < self.start
            || (self.end >= 0. && keypoint_seconds > self.end)
        {
            return None;
        }
        // 0x6ec1ce..0x6ec20b runs on the rendering/update thread with the
        // device's PC24 control word: x87 registers do not imply f64
        // arithmetic. The table was built in that same context, so sampling and
        // construction cannot disagree about precision (see module notes).
        let mut ms = keypoint_seconds * 1000.;
        for (duration, table) in &self.segments {
            if ms < *duration {
                let index = (ms / *duration * 127.) as usize;
                return Some(table[index.min(127)]);
            }
            ms -= *duration;
        }
        Some(self.segments.last().unwrap().1[127])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_lookup_is_not_continuous_or_exact_fraction_lerp() {
        let t = ColorTrack::new(0., -1., 0, &[(0x7f7f7f7f, 1000.)]).unwrap();
        assert_eq!(t.segments[0].1[0], 0);
        // PC24: f32(1 * stored reciprocal) rounds 0.999..*127 up to 1.
        assert_eq!(t.segments[0].1[1], 0x01010101);
        assert_eq!(t.segments[0].1[64], 0x40404040);
        assert_eq!(t.segments[0].1[126], 0x7e7e7e7e);
        assert_eq!(t.segments[0].1[127], 0x7f7f7f7f); // exact authored endpoint
        assert_eq!(t.sample(0.5), Some(0x3f3f3f3f)); // index floor(.5*127)=63
        assert_eq!(t.sample(1.), Some(0x7f7f7f7f));
    }
    #[test]
    fn double_precision_construction_is_not_the_native_table() {
        // The independent f64 oracle follows the same FLD/FMUL/FADD/ftol shape
        // but keeps a 53-bit mantissa at every step. It disagrees with the
        // native PC24 table at interior indices, so the two are not
        // interchangeable and the extended table cannot be the shipped one.
        let reciprocal = f32::from_bits(RECIPROCAL_BITS) as f64;
        let track = ColorTrack::new(0., -1., 0, &[(0x7f7f7f7f, 1000.)]).unwrap();
        let mut differing = 0;
        for i in 1..127 {
            let f = i as f64 * reciprocal;
            let double = (127. * f) as u32; // from == 0, so the a-term vanishes
            let stored = track.segments[0].1[i] & 0xff;
            if double != stored {
                differing += 1;
            }
        }
        assert!(differing > 0, "PC24 and f64 tables must differ");
        // Index 64 is the sharpest witness: 64*reciprocal is just below 1/2 in
        // f64 (127*f truncates to 63) but rounds up to exactly 1/2 in PC24.
        let f = 64. * reciprocal;
        assert_eq!((127. * f) as u32, 63);
        assert_eq!(track.segments[0].1[64] & 0xff, 64);
        // Endpoints are authored values in both modes.
        for precision in [Precision::Extended, Precision::Pc24] {
            let table = build(0x12345678, 0x9abcdef0, precision);
            assert_eq!(table[0], 0x12345678);
            assert_eq!(table[127], 0x9abcdef0);
        }
    }
    #[test]
    fn render_thread_pc24_rounds_multiply_divide_and_segment_subtraction() {
        let t = ColorTrack::new(0., -1., 0, &[(0xffffffff, 1000.)]).unwrap();
        // Native f32(ms/time*127) is exactly 1. PC53 falsely selects index 0.
        assert_eq!(t.sample(f32::from_bits(0x3c010204)), Some(0x02020202));
        let t = ColorTrack::new(0., -1., 0, &[(0xff0000ff, 290.), (0xff00ff00, 1000.)]).unwrap();
        // PC24 rounds stored .29 seconds *1000 to 290, crossing the segment
        // exactly. The authored endpoint is independent of LUT load precision.
        assert_eq!(t.sample(0.29), Some(0xff0000ff));
    }
    #[test]
    fn times_are_keypoint_relative_end_inclusive_and_last_color_held() {
        let t = ColorTrack::new(
            0.25,
            0.75,
            0xff000000,
            &[(0xffffffff, 400.), (0x80000000, 300.)],
        )
        .unwrap();
        assert_eq!(t.sample(0.249), None);
        // Start bounds eligibility, not a subtraction from keypoint time.
        assert!(t.sample(0.25).unwrap() != 0xff000000);
        assert_eq!(t.sample(0.75), Some(0x80000000));
        assert_eq!(t.sample(0.751), None);
        for (s, e, d) in [
            (f32::NAN, -1., 1.),
            (0., f32::INFINITY, 1.),
            (0., -1., 0.),
            (0., -1., f32::NAN),
        ] {
            assert!(ColorTrack::new(s, e, 0, &[(0, d)]).is_err());
        }
        assert!(ColorTrack::new(0., -1., 0, &[]).is_err());
        assert!(ColorTrack::new(0., f32::NAN, 0, &[(0, 1.)]).is_err());
    }
    #[test]
    fn construction_precision_is_material_and_native() {
        // f32(1 * 1/127) rounds to exactly 1.0 for 127*recip in PC24, so the
        // first interior entry is 1 instead of the extended-precision 0.
        let extended = build(0, 0x7f7f7f7f, Precision::Extended);
        let pc24 = build(0, 0x7f7f7f7f, Precision::Pc24);
        assert_eq!(extended[1], 0);
        assert_eq!(pc24[1], 0x01010101);
        // Authored endpoints never depend on the construction mode.
        for precision in [Precision::Extended, Precision::Pc24] {
            let table = build(0x12345678, 0x9abcdef0, precision);
            assert_eq!(table[0], 0x12345678);
            assert_eq!(table[127], 0x9abcdef0);
        }
        let track =
            ColorTrack::new_with_precision(0., -1., 0, &[(0x7f7f7f7f, 1000.)], Precision::Pc24)
                .unwrap();
        assert_eq!(track.segments[0].1[1], 0x01010101);
        // The counter reports packed interior entries whose built value differs
        // between the two construction modes. Compare against the tables the
        // constructor actually stores instead of a magic constant: the count
        // depends on f32 rounding at every interior index.
        let extended = build(0, 0x7f7f7f7f, Precision::Extended);
        let pc24 = build(0, 0x7f7f7f7f, Precision::Pc24);
        let direct = (1..127).filter(|i| extended[*i] != pc24[*i]).count();
        assert_eq!(track.construction_divergences(), direct);
        assert!(direct > 0, "the two modes must not be interchangeable");
        assert!(direct <= 126);
        let shipped = ColorTrack::new(0., -1., 0, &[(0x7f7f7f7f, 1000.)]).unwrap();
        assert!(shipped.construction_divergences() > 0);
        // Same FPU context for construction and sampling: the shipped table is
        // the PC24 one (module notes; no thread creation, no control-word
        // write anywhere in the application code).
        assert_eq!(CONSTRUCTION, Precision::Pc24);
        assert_eq!(shipped.segments[0].1[1], build(0, 0x7f7f7f7f, Precision::Pc24)[1]);
    }
    #[test]
    fn segment_chain_reports_one_end_pair_per_destination() {
        let t = ColorTrack::new(
            0.,
            -1.,
            0xff000000,
            &[(0xffffffff, 400.), (0x00ff00ff, 300.)],
        )
        .unwrap();
        assert_eq!(t.ends, [(0xff000000, 0xffffffff), (0xffffffff, 0x00ff00ff)]);
        assert_eq!(t.sample(0.4), Some(0xffffffff));
    }
}
