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
#[derive(Debug, Clone, Copy, PartialEq)]
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
/// Constant-space traversal of packed MSB-first placement bits. Only the
/// current 16x16 patch owns RNG state; no full-world transforms or cell IDs.
pub struct PlacementIter<'a> {
    width: usize,
    height: usize,
    bits: std::borrow::Cow<'a, [u8]>,
    base: f32,
    variation: f32,
    patch: usize,
    cell: usize,
    last_cell: usize,
    rng: Option<Ranmar>,
}
impl<'a> PlacementIter<'a> {
    pub fn new(width: usize, height: usize, bits: &'a [u8], base: f32, variation: f32) -> Self {
        Self {
            width,
            height,
            bits: std::borrow::Cow::Borrowed(bits),
            base,
            variation,
            patch: 0,
            cell: 0,
            last_cell: 0,
            rng: None,
        }
    }
    fn active(&self, col: usize, row: usize) -> bool {
        let bit = row * self.width + col;
        self.bits
            .get(bit / 8)
            .is_some_and(|byte| byte & (0x80 >> (bit % 8)) != 0)
    }
}
impl PlacementIter<'static> {
    pub fn owned(width: usize, height: usize, bits: Vec<u8>, base: f32, variation: f32) -> Self {
        Self {
            width,
            height,
            bits: std::borrow::Cow::Owned(bits),
            base,
            variation,
            patch: 0,
            cell: 0,
            last_cell: 0,
            rng: None,
        }
    }
}
impl Iterator for PlacementIter<'_> {
    type Item = Placement;
    fn next(&mut self) -> Option<Placement> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let patches_x = self.width.div_ceil(16);
        let patches_y = self.height.div_ceil(16);
        while self.patch < patches_x * patches_y {
            let px = self.patch % patches_x * 16;
            let py = self.patch / patches_x * 16;
            let w = (self.width - px).min(16);
            let h = (self.height - py).min(16);
            if self.rng.is_none() {
                let Some(last) = (0..w * h)
                    .rev()
                    .find(|&cell| self.active(px + cell % w, py + cell / w))
                else {
                    self.patch += 1;
                    continue;
                };
                self.last_cell = last + 1;
                let seed = (px + py) as u32;
                self.rng = Some(Ranmar::new(seed % 31328, seed % 30081));
                self.cell = 0;
            }
            while self.cell < self.last_cell {
                let col = px + self.cell % w;
                let row = py + self.cell / w;
                self.cell += 1;
                let active = self.active(col, row);
                let rng = self.rng.as_mut().unwrap();
                // Empty cells still consume exactly the native five draws.
                let jitter_x = rng.next() as f32 - 0.5;
                let jitter_z = rng.next() as f32 - 0.5;
                let yaw = rng.next() as f32 * 6.28;
                let scale =
                    self.base * (1. - self.variation + 2. * self.variation * rng.next() as f32);
                let wind_selector = rng.next() as f32 * 10.;
                if active {
                    return Some(Placement {
                        col: col as u32,
                        row: row as u32,
                        jitter_x,
                        jitter_z,
                        yaw,
                        scale,
                        wind_selector,
                    });
                }
            }
            self.patch += 1;
            self.rng = None;
        }
        None
    }
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
    fn packed_iterator_is_bit_exact_to_eager_patch_transforms() {
        for (width, height) in [(8usize, 3usize), (16, 16), (24, 33), (48, 32), (64, 65)] {
            for pattern in 0..4 {
                let active: Vec<u32> = (0..width * height)
                    .filter(|i| match pattern {
                        0 => true,
                        1 => i % 17 == 0,
                        2 => i % 7 < 2,
                        _ => false,
                    })
                    .map(|i| i as u32)
                    .collect();
                let mut bits = vec![0; (width * height).div_ceil(8)];
                for &bit in &active {
                    bits[bit as usize / 8] |= 0x80 >> (bit % 8);
                }
                assert_eq!(
                    PlacementIter::new(width, height, &bits, 2., 0.3).collect::<Vec<_>>(),
                    placements(width, height, &active, 2., 0.3),
                    "{width}x{height}, pattern {pattern}"
                );
            }
        }
    }

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
