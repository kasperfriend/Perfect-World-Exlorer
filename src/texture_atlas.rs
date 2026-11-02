//! Rectangle packing for native-size scenery textures in 4096² GPU pages.
//! The 128-pixel grid avoids reducing large source textures to fixed 128² slots.
pub const SIDE: usize = 4096;
pub const UNIT: usize = 128;
const GRID: usize = SIDE / UNIT;
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}
pub struct Atlas {
    used: [[bool; GRID]; GRID],
    free: usize,
}
impl Atlas {
    pub fn new() -> Self {
        Self {
            used: [[false; GRID]; GRID],
            free: GRID * GRID,
        }
    }
    /// Allocate a native resolution rectangle if possible. `reserve` leaves at
    /// least one minimum slot per other texture; when needed, reduce this one.
    pub fn allocate(
        &mut self,
        width: usize,
        height: usize,
        reserve: usize,
        allow_reduce: bool,
    ) -> Option<Slot> {
        let mut w = width.max(1).next_power_of_two().clamp(UNIT, SIDE) / UNIT;
        let mut h = height.max(1).next_power_of_two().clamp(UNIT, SIDE) / UNIT;
        loop {
            if self.free >= w * h + reserve {
                for y in 0..=GRID - h {
                    for x in 0..=GRID - w {
                        if (y..y + h).all(|yy| (x..x + w).all(|xx| !self.used[yy][xx])) {
                            for row in &mut self.used[y..y + h] {
                                for item in &mut row[x..x + w] {
                                    *item = true;
                                }
                            }
                            self.free -= w * h;
                            return Some(Slot {
                                x: x * UNIT,
                                y: y * UNIT,
                                w: w * UNIT,
                                h: h * UNIT,
                            });
                        }
                    }
                }
            }
            if !allow_reduce || (w == 1 && h == 1) {
                return None;
            }
            if w >= h && w > 1 {
                w /= 2
            } else if h > 1 {
                h /= 2
            } else {
                w /= 2
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packs_rectangles_without_overlap() {
        let mut a = Atlas::new();
        let p = a.allocate(512, 256, 0, false).unwrap();
        let q = a.allocate(128, 512, 0, false).unwrap();
        assert!(p.x + p.w <= q.x || q.x + q.w <= p.x || p.y + p.h <= q.y || q.y + q.h <= p.y);
        assert_eq!((p.w, p.h), (512, 256));
    }
    #[test]
    fn reserve_prevents_starving_other_textures() {
        let mut a = Atlas::new();
        let p = a.allocate(4096, 4096, 1023, true).unwrap();
        assert_eq!((p.w, p.h), (128, 128));
        for _ in 0..1023 {
            assert!(a.allocate(128, 128, 0, false).is_some());
        }
        assert!(a.allocate(128, 128, 0, false).is_none());
    }
}
