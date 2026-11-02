//! Retail 1.4.2 GFX controller 108, not the later floating interpolation path.
//! 0x6ec5dd builds 128 packed entries/segment; 0x6ec190 indexes that table.
//! Start/end are seconds; segment lengths are milliseconds. Callers preserve
//! their last keypoint color when sample returns None (controller inactive).
#[derive(Debug, Clone)]
pub struct ColorTrack {
    start: f32,
    end: f32,
    segments: Vec<(f32, [u32; 128])>,
}
impl ColorTrack {
    pub fn new(
        start: f32,
        end: f32,
        original: u32,
        destinations: &[(u32, f32)],
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
        let mut segments = Vec::with_capacity(destinations.len());
        for &(to, duration) in destinations {
            if !duration.is_finite() || duration <= 0. {
                return Err("invalid GFX color segment duration".into());
            }
            let mut table = [0; 128];
            table[0] = from;
            table[127] = to;
            for (i, value) in table.iter_mut().enumerate().take(127).skip(1) {
                // Exact value of the native stored f32 reciprocal at 0x8ebdcc,
                // 0x3c010204 = 2113665/268435456. The x87 interpolation has
                // enough precision for these positive dyadic products. Using
                // i/127 or f32 lerp changes authored packed bytes.
                const DEN: u64 = 268435456;
                let n = i as u64 * 2113665;
                for shift in [0, 8, 16, 24] {
                    let a = ((from >> shift) & 255) as u64;
                    let b = ((to >> shift) & 255) as u64;
                    *value |= (((a * (DEN - n) + b * n) / DEN) as u32) << shift;
                }
            }
            segments.push((duration, table));
            from = to;
        }
        Ok(Self {
            start,
            end,
            segments,
        })
    }
    pub fn sample(&self, keypoint_seconds: f32) -> Option<u32> {
        if !keypoint_seconds.is_finite()
            || keypoint_seconds < self.start
            || (self.end >= 0. && keypoint_seconds > self.end)
        {
            return None;
        }
        // 0x6ec1ce..0x6ec20b runs on the rendering/update thread. D3D8
        // selects PC24: x87 registers do not imply f64 arithmetic. This
        // sampling context is separate from the table's load-thread context.
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
        assert_eq!(t.segments[0].1[1], 0); // stored reciprocal is below 1/127
        assert_eq!(t.segments[0].1[64], 0x3f3f3f3f);
        assert_eq!(t.segments[0].1[127], 0x7f7f7f7f); // exact authored endpoint
        assert_eq!(t.sample(0.5), Some(0x3e3e3e3e)); // index floor(.5*127)=63
        assert_eq!(t.sample(1.), Some(0x7f7f7f7f));
    }
    #[test]
    fn all_packed_channels_match_independent_double_interpolation_table() {
        // f64 represents every dyadic product here exactly; independent oracle
        // follows native FLD/FMUL/FADD/ftol rather than the integer formula.
        let reciprocal = f32::from_bits(0x3c010204) as f64;
        for from in 0..=255u32 {
            for to in 0..=255u32 {
                let a = from * 0x01010101;
                let b = to * 0x01010101;
                let track = ColorTrack::new(0., -1., a, &[(b, 1000.)]).unwrap();
                for i in 1..127 {
                    let f = i as f64 * reciprocal;
                    let byte = (from as f64 * (1. - f) + to as f64 * f) as u32;
                    assert_eq!(track.segments[0].1[i], byte * 0x01010101);
                }
            }
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
    }
}
