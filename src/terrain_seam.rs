//! Stitch local high-resolution terrain to the viewer's 4-grid distant mesh.
//! This is a crack-reduction approximation, not the client's LOD algorithm.
const STEP: usize = 4;
pub fn stitch(
    heights: &[f32],
    normals: &[[i8; 3]],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    bounds: [usize; 4],
) -> (f32, [f32; 3]) {
    let idx = y * width + x;
    let original = heights[idx];
    let n = normals[idx].map(|v| v as f32 / 127.);
    let [x0, x1, y0, y1] = bounds;
    let d = x
        .saturating_sub(x0)
        .min(x1.saturating_sub(x))
        .min(y.saturating_sub(y0))
        .min(y1.saturating_sub(y));
    if d >= STEP || x < x0 || x > x1 || y < y0 || y > y1 {
        return (original, n);
    }
    let left = (x / STEP) * STEP;
    let top = (y / STEP) * STEP;
    let right = (left + STEP).min(width - 1);
    let bottom = (top + STEP).min(height - 1);
    let fx = if right == left {
        0.
    } else {
        (x - left) as f32 / (right - left) as f32
    };
    let fy = if bottom == top {
        0.
    } else {
        (y - top) as f32 / (bottom - top) as f32
    };
    let points = [
        top * width + left,
        top * width + right,
        bottom * width + left,
        bottom * width + right,
    ];
    let sample = |a: f32, b: f32, c: f32, d: f32| {
        if fx + fy <= 1. {
            a + (b - a) * fx + (c - a) * fy
        } else {
            d + (c - d) * (1. - fx) + (b - d) * (1. - fy)
        }
    };
    let coarse = sample(
        heights[points[0]],
        heights[points[1]],
        heights[points[2]],
        heights[points[3]],
    );
    let coarse_n: [f32; 3] = std::array::from_fn(|i| {
        sample(
            normals[points[0]][i] as f32 / 127.,
            normals[points[1]][i] as f32 / 127.,
            normals[points[2]][i] as f32 / 127.,
            normals[points[3]][i] as f32 / 127.,
        )
    });
    let t = d as f32 / STEP as f32;
    (
        coarse + (original - coarse) * t,
        std::array::from_fn(|i| coarse_n[i] + (n[i] - coarse_n[i]) * t),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borders_equal_far_triangles_and_interior_stays_original() {
        let w = 17;
        let h = 17;
        let heights = (0..w * h)
            .map(|i| ((i % w) * (i / w) % 7) as f32)
            .collect::<Vec<_>>();
        let normals = vec![[0, 127, 0]; w * h];
        let bounds = [4, 12, 4, 12];
        for y in 4..=12 {
            let (z, _) = stitch(&heights, &normals, w, h, 4, y, bounds);
            let a = y / 4 * 4;
            let b = (a + 4).min(16);
            let expected = heights[a * w + 4]
                + (heights[b * w + 4] - heights[a * w + 4]) * (y - a) as f32 / 4.;
            assert!((z - expected).abs() < 1e-6);
        }
        let (z, n) = stitch(&heights, &normals, w, h, 8, 8, bounds);
        assert_eq!(z, heights[8 * w + 8]);
        assert_eq!(n, [0., 1., 0.]);
    }
}
#[cfg(test)]
mod more_tests {
    use super::*;
    #[test]
    fn smooth_interpolation_preserves_near_interior_and_matches_all_edges() {
        let (w, h) = (25, 25);
        let values = (0..w * h)
            .map(|i| ((i * 17) % 53) as f32)
            .collect::<Vec<_>>();
        let normals = (0..w * h)
            .map(|i| [((i * 3) % 120) as i8, 80, ((i * 5) % 120) as i8])
            .collect::<Vec<_>>();
        let b = [4, 20, 4, 20];
        for i in 4..=20 {
            for (x, y, horizontal) in [(i, 4, true), (i, 20, true), (4, i, false), (20, i, false)] {
                let (got, n) = stitch(&values, &normals, w, h, x, y, b);
                let axis = if horizontal { x } else { y };
                let lo = axis / 4 * 4;
                let hi = (lo + 4).min(24);
                let t = (axis - lo) as f32 / 4.;
                let (a, c) = if horizontal {
                    (y * w + lo, y * w + hi)
                } else {
                    (lo * w + x, hi * w + x)
                };
                assert!((got - (values[a] + (values[c] - values[a]) * t)).abs() < 0.0001);
                for k in 0..3 {
                    let expect = (normals[a][k] as f32
                        + (normals[c][k] as f32 - normals[a][k] as f32) * t)
                        / 127.;
                    assert!((n[k] - expect).abs() < 0.0001);
                }
            }
        }
        for y in 8..=16 {
            for x in 8..=16 {
                assert_eq!(
                    stitch(&values, &normals, w, h, x, y, b).0,
                    values[y * w + x]
                );
            }
        }
    }
}
