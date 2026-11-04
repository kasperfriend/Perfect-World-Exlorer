//! Retail grass patch placement: 16x16 patches use the client's RANMAR
//! generator and consume five random values for every cell, even empty cells.
#[derive(Clone)]
struct Ranmar {
    u: [f64; 98],
    c: f64,
    i: usize,
    j: usize,
}
impl Ranmar {
    fn new(ij: u32, kl: u32) -> Self {
        let (ij, kl) = if ij > 31328 || kl > 30081 {
            (1802, 9373)
        } else {
            (ij, kl)
        };
        let mut i = (ij / 177 % 177 + 2) as i32;
        let mut j = (ij % 177 + 2) as i32;
        let mut k = (kl / 169 % 178 + 1) as i32;
        let mut l = (kl % 169) as i32;
        let mut u = [0.; 98];
        for v in &mut u[1..] {
            let mut s = 0.;
            let mut t = 0.5;
            for _ in 0..24 {
                let m = (((i * j) % 179) * k) % 179;
                i = j;
                j = k;
                k = m;
                l = (53 * l + 1) % 169;
                if (l * m) % 64 >= 32 {
                    s += t;
                }
                t *= 0.5;
            }
            *v = s;
        }
        Self {
            u,
            c: 362436. / 16777216.,
            i: 97,
            j: 33,
        }
    }
    fn next(&mut self) -> f64 {
        let mut x = self.u[self.i] - self.u[self.j];
        if x < 0. {
            x += 1.;
        }
        self.u[self.i] = x;
        self.i = if self.i == 1 { 97 } else { self.i - 1 };
        self.j = if self.j == 1 { 97 } else { self.j - 1 };
        self.c -= 7654321. / 16777216.;
        if self.c < 0. {
            self.c += 16777213. / 16777216.;
        }
        x -= self.c;
        if x < 0. {
            x += 1.;
        }
        x
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub col: u32,
    pub row: u32,
    pub jitter_x: f32,
    pub jitter_z: f32,
    pub yaw: f32,
    pub scale: f32,
    /// The client's fifth random value, `rand(0, 10)` at `0x653ea1`. The
    /// emitter adds it to `2*|y - 10000|` and takes the remainder by 80
    /// (`0x653ea6`-`0x653ec1`), so this is the wind-history slot of the whole
    /// placement rather than a phase offset.
    pub wind_selector: f32,
}
/// Generates retail patch randomization for every masked cell. The client's
/// `step = 5 - ftol(colour*4 + 0.5)` grid walk (`0x653b6e`) is applied when the
/// placements are drawn, so the selector stream stays identical at any density.
/// Original terrain height query still differs.
pub fn placements(
    width: usize,
    height: usize,
    active: &[u32],
    base: f32,
    variation: f32,
) -> Vec<Placement> {
    use std::collections::BTreeMap;
    let mut patches: BTreeMap<(usize, usize), Vec<(usize, usize, usize)>> = BTreeMap::new();
    for &bit in active {
        let row = bit as usize / width;
        let col = bit as usize % width;
        if row >= height {
            continue;
        }
        let py = row / 16 * 16;
        let px = col / 16 * 16;
        let patch_width = (width - px).min(16);
        let ordinal = (row - py) * patch_width + (col - px);
        patches
            .entry((py, px))
            .or_default()
            .push((ordinal, col, row));
    }
    let mut result = Vec::with_capacity(active.len());
    for ((py, px), cells) in patches {
        let seed = (px + py) as u32;
        let mut rng = Ranmar::new(seed % 31328, seed % 30081);
        let mut ordinal = 0;
        for (target, col, row) in cells {
            while ordinal < target {
                for _ in 0..5 {
                    rng.next();
                }
                ordinal += 1;
            }
            let x = rng.next() as f32 - 0.5;
            let z = rng.next() as f32 - 0.5;
            let yaw = rng.next() as f32 * 6.28;
            let scale = base * (1. - variation + 2. * variation * rng.next() as f32);
            let selector = rng.next() as f32 * 10.;
            ordinal += 1;
            result.push(Placement {
                col: col as u32,
                row: row as u32,
                jitter_x: x,
                jitter_z: z,
                yaw,
                scale,
                wind_selector: selector,
            });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranmar_reference() {
        let mut r = Ranmar::new(1802, 9373);
        for _ in 0..20000 {
            r.next();
        }
        let got: Vec<_> = (0..6).map(|_| (r.next() * 16777216.) as u32).collect();
        assert_eq!(
            got,
            vec![6533892, 14220222, 7275067, 6172232, 8354498, 10633180]
        );
    }
    #[test]
    fn empty_cells_still_consume_random_and_patch_is_repeatable() {
        let all = placements(32, 16, &(0..512).collect::<Vec<_>>(), 2., 0.3);
        let sparse = placements(32, 16, &[0, 17, 255, 256, 511], 2., 0.3);
        for a in &sparse {
            let b = all
                .iter()
                .find(|b| b.col == a.col && b.row == a.row)
                .unwrap();
            assert_eq!(a.yaw, b.yaw);
            assert_eq!(a.scale, b.scale);
            assert!((1.4..=2.6).contains(&a.scale));
            assert!((-0.5..0.5).contains(&a.jitter_x));
        }
    }
}
