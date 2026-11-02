//! Isolated mip generation for alpha-tested foliage atlas rectangles.
//! No adjacent atlas tile is read, so distant leaves do not bleed other trees.

/// Box-filter a power-of-two RGBA tile. For cutout foliage, filter RGB weighted
/// by alpha to avoid dark transparent fringes. `raw` must be used as input to
/// the next level; coverage adjustment is applied only to the uploaded copy.
pub fn downsample(raw: &[u8], width: usize, height: usize, cutout: bool) -> Vec<u8> {
    assert!(width >= 2 && height >= 2 && raw.len() == width * height * 4);
    let (nw, nh) = (width / 2, height / 2);
    let mut out = vec![0; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let i = (2 * y * width + 2 * x) * 4;
            let idx = [i, i + 4, i + width * 4, i + width * 4 + 4];
            let a: u32 = idx.iter().map(|&p| u32::from(raw[p + 3])).sum();
            let o = (y * nw + x) * 4;
            for c in 0..3 {
                out[o + c] = if cutout && a != 0 {
                    ((idx
                        .iter()
                        .map(|&p| u32::from(raw[p + c]) * u32::from(raw[p + 3]))
                        .sum::<u32>()
                        + a / 2)
                        / a) as u8
                } else {
                    ((idx.iter().map(|&p| u32::from(raw[p + c])).sum::<u32>() + 2) / 4) as u8
                };
            }
            out[o + 3] = ((a + 2) / 4) as u8;
        }
    }
    out
}

/// Approximate preservation of the original fraction above the 0.5 alpha test.
/// At tiny levels exact coverage may be impossible; cap the correction.
pub fn preserve_coverage(raw: &[u8], source_coverage: f32) -> Vec<u8> {
    preserve_coverage_at(raw, source_coverage, 128)
}

/// Preserve the coverage of a specific material alpha-reference value.
pub fn preserve_coverage_at(raw: &[u8], source_coverage: f32, alpha_ref: u8) -> Vec<u8> {
    let mut result = raw.to_vec();
    if alpha_ref == 0 {
        return result;
    }
    let pixels = raw.len() / 4;
    let wanted = (source_coverage.clamp(0., 1.) * pixels as f32).round() as usize;
    if wanted == 0 || pixels == 0 {
        return result;
    }
    let mut histogram = [0usize; 256];
    for px in raw.chunks_exact(4) {
        histogram[px[3] as usize] += 1;
    }
    let mut count = 0;
    let mut threshold = 0usize;
    for a in (1..=255).rev() {
        count += histogram[a];
        if count >= wanted {
            threshold = a;
            break;
        }
    }
    if threshold == 0 {
        return result;
    }
    let scale = (alpha_ref as f32 / threshold as f32).clamp(0.5, 4.0);
    for px in result.chunks_exact_mut(4) {
        px[3] = (px[3] as f32 * scale).round().clamp(0., 255.) as u8;
    }
    result
}

pub fn coverage(rgba: &[u8]) -> f32 {
    coverage_at(rgba, 128)
}

pub fn coverage_at(rgba: &[u8], alpha_ref: u8) -> f32 {
    let n = rgba.len() / 4;
    if n == 0 {
        return 0.;
    }
    rgba.chunks_exact(4).filter(|p| p[3] >= alpha_ref).count() as f32 / n as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_black_does_not_darken_leaf_color() {
        let input = [255, 100, 20, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(downsample(&input, 2, 2, true), vec![255, 100, 20, 64]);
        assert_eq!(downsample(&input, 2, 2, false), vec![64, 25, 5, 64]);
    }
    #[test]
    fn preserves_cutout_coverage_when_resolvable() {
        let raw = [10, 10, 10, 64, 10, 10, 10, 64, 10, 10, 10, 0, 10, 10, 10, 0];
        let adjusted = preserve_coverage(&raw, 0.5);
        assert_eq!(coverage(&adjusted), 0.5);
        assert_eq!(coverage(&raw), 0.0);
    }
    #[test]
    fn supports_client_specific_alpha_references() {
        let raw = [10, 20, 30, 40, 10, 20, 30, 40, 0, 0, 0, 0, 0, 0, 0, 0];
        let corrected = preserve_coverage_at(&raw, 0.5, 80);
        assert_eq!(coverage_at(&corrected, 80), 0.5);
        assert_eq!(coverage_at(&raw, 80), 0.0);
        assert_eq!(preserve_coverage_at(&raw, 0.5, 0), raw);
    }
    #[test]
    fn empty_alpha_stays_empty() {
        let raw = [1, 2, 3, 0, 1, 2, 3, 0];
        assert_eq!(preserve_coverage(&raw, 0.5), raw);
    }
}
