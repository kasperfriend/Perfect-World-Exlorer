//! Bounded T2LM v2 DDS records and subterrain-to-mask-area placement.
//! The offset table is indexed by mask area; payload order is not guaranteed.
pub const TILES: usize = 16;
pub struct Tiles<'a> {
    bytes: &'a [u8],
    ranges: [(usize, usize); TILES],
}
impl<'a> Tiles<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        const HEADER: usize = 24 + TILES * 4;
        let u = |p| -> Result<usize, String> {
            Ok(u32::from_le_bytes(
                bytes
                    .get(p..p + 4)
                    .ok_or("truncated T2LM header")?
                    .try_into()
                    .unwrap(),
            ) as usize)
        };
        if u(0)? != 0x54324c4d || u(4)? != 2 || u(8)? != TILES || u(12)? != 256 || u(16)? != 1 {
            return Err("unsupported T2LM version, tile geometry or DDS flags".into());
        }
        let size = u(20)?;
        if size == 0 {
            return Err("empty T2LM tile".into());
        }
        let mut ranges = [(0, 0); TILES];
        for i in 0..TILES {
            let start = u(24 + i * 4)?;
            let end = start.checked_add(size).ok_or("T2LM tile size overflow")?;
            if start < HEADER || end > bytes.len() {
                return Err("T2LM tile outside payload".into());
            }
            if ranges[..i].iter().any(|&(a, b)| start < b && a < end) {
                return Err("overlapping T2LM tiles".into());
            }
            ranges[i] = (start, end);
        }
        Ok(Self { bytes, ranges })
    }
    pub fn tile(&self, i: usize) -> Option<&'a [u8]> {
        self.ranges.get(i).map(|&(a, b)| &self.bytes[a..b])
    }
}
#[derive(Debug)]
pub struct Layout {
    areas_x: usize,
    subs_x: usize,
    sub_count: usize,
}
impl Layout {
    pub fn new(areas_x: usize, area_count: usize, sub_count: usize) -> Result<Self, String> {
        if areas_x == 0
            || areas_x % 4 != 0
            || area_count == 0
            || area_count % areas_x != 0
            || (area_count / areas_x) % 4 != 0
            || sub_count.checked_mul(TILES) != Some(area_count)
        {
            return Err("inconsistent lightmap subterrain/mask-area dimensions".into());
        }
        Ok(Self {
            areas_x,
            subs_x: areas_x / 4,
            sub_count,
        })
    }
    pub fn area(&self, sub: usize, tile: usize) -> Option<usize> {
        if sub >= self.sub_count || tile >= TILES {
            return None;
        }
        let x = (sub % self.subs_x) * 4 + tile % 4;
        let y = (sub / self.subs_x) * 4 + tile / 4;
        Some(y * self.areas_x + x)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 88 + TILES * 4];
        for (i, v) in [0x54324c4du32, 2, 16, 256, 1, 4].into_iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        for i in 0..TILES {
            // Reverse order deliberately. A next-offset size heuristic fails.
            let p = 88 + (TILES - 1 - i) * 4;
            b[24 + i * 4..28 + i * 4].copy_from_slice(&(p as u32).to_le_bytes());
            b[p..p + 4].fill(i as u8);
        }
        b
    }
    #[test]
    fn independent_offsets_preserve_mask_indices() {
        let b = fixture();
        let t = Tiles::parse(&b).unwrap();
        for i in 0..TILES {
            assert_eq!(t.tile(i).unwrap(), [i as u8; 4]);
        }
        assert!(t.tile(TILES).is_none());
    }
    #[test]
    fn rejects_invalid_headers_ranges_and_overlap() {
        let original = fixture();
        for n in 0..original.len() {
            assert!(Tiles::parse(&original[..n]).is_err());
        }
        for (p, v) in [
            (0, 0),
            (4, 3),
            (8, 15),
            (12, 128),
            (16, 0),
            (16, 3),
            (20, 0),
            (20, u32::MAX),
            (24, 0),
            (24, 88),
            (24, 89),
        ] {
            let mut b = original.clone();
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
            assert!(Tiles::parse(&b).is_err(), "{p}/{v}");
        }
    }
    #[test]
    fn rectangular_world_layout_is_bijective() {
        for (x, y) in [(44, 32), (32, 44), (4, 4), (8, 12)] {
            let l = Layout::new(x, x * y, x * y / TILES).unwrap();
            let mut ids = vec![false; x * y];
            for sub in 0..x * y / TILES {
                for tile in 0..TILES {
                    let a = l.area(sub, tile).unwrap();
                    assert!(!ids[a]);
                    ids[a] = true;
                }
            }
            assert!(ids.into_iter().all(|x| x));
            assert_eq!(l.area(0, 0), Some(0));
            assert_eq!(l.area(0, 4), Some(x));
            if y > 4 {
                assert_eq!(l.area(x / 4, 0), Some(x * 4));
            }
            assert!(l.area(x * y / TILES, 0).is_none());
            assert!(l.area(0, TILES).is_none());
        }
    }
    #[test]
    fn rejects_inconsistent_layout() {
        for (x, n, s) in [
            (0, 0, 0),
            (3, 48, 3),
            (4, 0, 0),
            (4, 15, 1),
            (4, 20, 1),
            (8, 16, 1),
            (4, 16, 0),
            (4, 16, usize::MAX),
        ] {
            assert!(Layout::new(x, n, s).is_err());
        }
    }
}
