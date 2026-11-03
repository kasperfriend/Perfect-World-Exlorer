//! Main-world map coordinates and destinations from the client.
use crate::pck;
use std::{fs, path::Path, sync::Arc};
#[derive(Debug, Clone, PartialEq)]
pub struct Destination {
    pub id: u32,
    pub name: String,
    pub position: [f32; 3],
}
pub fn destinations(path: &Path) -> Result<Vec<Destination>, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() % 2 != 0 {
        return Err("odd UTF-16 destination file".into());
    }
    let words = bytes
        .chunks_exact(2)
        .map(|x| u16::from_le_bytes([x[0], x[1]]))
        .collect::<Vec<_>>();
    let text = String::from_utf16(&words).map_err(|e| e.to_string())?;
    parse_destinations(text.trim_start_matches('\u{feff}'))
}
fn parse_destinations(text: &str) -> Result<Vec<Destination>, String> {
    let mut result = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').map(str::trim).collect::<Vec<_>>();
        if fields.len() < 5 {
            return Err(format!("invalid destination line {}", number + 1));
        }
        let id = fields[0]
            .parse()
            .map_err(|_| format!("invalid destination ID on {}", number + 1))?;
        let name = fields[1]
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .ok_or("invalid destination name")?
            .to_string();
        let coords = fields[3]
            .trim_matches('"')
            .split(',')
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid destination coordinates")?;
        if name.is_empty() || coords.len() != 3 || coords.iter().any(|v| !v.is_finite()) {
            return Err(format!("invalid destination at {}", number + 1));
        }
        // Only main world targets; do not silently teleport into another map.
        if fields[2] == "1" {
            result.push(Destination {
                id,
                name,
                position: [coords[0], coords[1], coords[2]],
            });
        }
    }
    result.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    Ok(result)
}
#[derive(Clone)]
pub struct MapSpace {
    pub width: usize,
    pub height: usize,
    pub spacing: f32,
    pub heights: Arc<[f32]>,
}
impl MapSpace {
    pub fn world_size(&self) -> [f32; 2] {
        [
            self.width as f32 * self.spacing,
            self.height as f32 * self.spacing,
        ]
    }
    pub fn from_uv(&self, u: f32, v: f32) -> [f32; 2] {
        let [w, h] = self.world_size();
        [(u.clamp(0., 1.) - 0.5) * w, (0.5 - v.clamp(0., 1.)) * h]
    }
    pub fn to_uv(&self, x: f32, z: f32) -> [f32; 2] {
        let [w, h] = self.world_size();
        [x / w + 0.5, 0.5 - z / h]
    }
    pub fn ground(&self, x: f32, z: f32) -> f32 {
        let gx = (x / self.spacing + self.width as f32 * 0.5).clamp(0., (self.width - 1) as f32);
        let gy = (self.height as f32 * 0.5 - z / self.spacing).clamp(0., (self.height - 1) as f32);
        let ix = (gx as usize).min(self.width - 2);
        let iy = (gy as usize).min(self.height - 2);
        let fx = gx - ix as f32;
        let fy = gy - iy as f32;
        let a = self.heights[iy * self.width + ix];
        let b = self.heights[iy * self.width + ix + 1];
        let c = self.heights[(iy + 1) * self.width + ix];
        let d = self.heights[(iy + 1) * self.width + ix + 1];
        if fx + fy <= 1. {
            a + (b - a) * fx + (c - a) * fy
        } else {
            d + (c - d) * (1. - fx) + (b - d) * (1. - fy)
        }
    }
}
/// Client world-map tile name: ROW then COLUMN, `%03d` of `row * 10 + col`,
/// which is why the retail archive runs 000..107 for 11 rows x 8 columns. The
/// two-digit names in the same folder (74, 84, ...) are duplicates of 074, 084
/// and are not needed.
pub fn map_tile_name(row: u32, col: u32) -> String {
    format!("surfaces/maps/{:03}.dds", row * 10 + col)
}
/// Client minimap tile name: `%02d%02d` of ROW then COLUMN, the format string at
/// `0x565220` after `Surfaces\\MiniMaps\\`. Each tile is 256x256 DXT1.
pub fn minimap_tile_name(zone: &str, row: u32, col: u32) -> String {
    format!("surfaces/minimaps/{zone}/{row:02}{col:02}.dds")
}

/// The world is 8192x11264 units: 8x11 regions of 1024. A minimap row covers
/// 512 units, so the world spans exactly 16 x 22 tiles. The archive ships 23
/// rows (368 tiles); the 23rd lies past the southern edge and is not part of the
/// world, so it is not composited.
const MAP_ROWS: u32 = 11;
const MAP_COLS: u32 = 8;
const MINIMAP_ROWS: u32 = 22;
const MINIMAP_COLS: u32 = 16;
/// The main world's own minimap set; other zones (a01, b03, ...) are separate
/// instances with their own coordinate space.
pub const WORLD_MINIMAP_ZONE: &str = "world";

/// Composite a row-major tile grid from `surfaces.pck` into one image.
///
/// `side` is the per-tile output size: the client ships 1024px map tiles and
/// 256px minimap tiles, which at full size would be a 368 MB texture.
fn compose(
    element: &Path,
    rows: u32,
    cols: u32,
    side: u32,
    name: impl Fn(u32, u32) -> String,
) -> Result<image::RgbaImage, String> {
    let path = element.join("surfaces.pck");
    let mut f = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let entries = pck::directory(&mut f)?;
    let mut map = image::RgbaImage::new(cols * side, rows * side);
    for row in 0..rows {
        for col in 0..cols {
            let tile_name = name(row, col);
            let entry = entries
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(&tile_name))
                .ok_or_else(|| format!("missing map tile {tile_name}"))?;
            let data = pck::read_entry(&mut f, entry)?;
            let tile = image::load_from_memory_with_format(&data, image::ImageFormat::Dds)
                .map_err(|e| format!("{tile_name}: {e}"))?
                .to_rgba8();
            let tile =
                image::imageops::resize(&tile, side, side, image::imageops::FilterType::Triangle);
            image::imageops::replace(&mut map, &tile, (col * side) as i64, (row * side) as i64);
        }
    }
    Ok(map)
}

/// The client's own full-world map, 11 rows x 8 columns of 1024px tiles
/// downsampled to 256px: 2048x2816, covering exactly the world's 8192x11264.
pub fn original_map(element: &Path) -> Result<image::RgbaImage, String> {
    compose(element, MAP_ROWS, MAP_COLS, 256, map_tile_name)
}

/// The client's own minimap for the main world: 16 x 22 tiles of 256px
/// downsampled to 128px: 2048x2816, the same extent as `original_map`, so one
/// world-to-uv transform positions both.
pub fn original_minimap(element: &Path) -> Result<image::RgbaImage, String> {
    compose(element, MINIMAP_ROWS, MINIMAP_COLS, 128, |row, col| {
        minimap_tile_name(WORLD_MINIMAP_ZONE, row, col)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_map_tile_names_follow_the_archive() {
        // Row then column, `%03d` of row*10+col: the retail folder holds
        // 000..107 for 11 rows x 8 columns plus a duplicate two-digit set.
        assert_eq!(map_tile_name(0, 0), "surfaces/maps/000.dds");
        assert_eq!(map_tile_name(0, 7), "surfaces/maps/007.dds");
        assert_eq!(map_tile_name(1, 0), "surfaces/maps/010.dds");
        assert_eq!(map_tile_name(6, 4), "surfaces/maps/064.dds");
        assert_eq!(map_tile_name(10, 7), "surfaces/maps/107.dds");
        // The minimap is `%02d%02d`, matching the `Surfaces\MiniMaps\%02d%02d`
        // format string at 0x565220.
        assert_eq!(
            minimap_tile_name("world", 0, 0),
            "surfaces/minimaps/world/0000.dds"
        );
        assert_eq!(
            minimap_tile_name("world", 21, 15),
            "surfaces/minimaps/world/2115.dds"
        );
    }

    #[test]
    fn minimap_grid_covers_the_world_exactly() {
        // 16 tiles of 512 units across 8192, 22 rows of 512 down 11264.
        assert_eq!(MINIMAP_COLS * 512, 8 * 1024);
        assert_eq!(MINIMAP_ROWS * 512, 11 * 1024);
        // The shipped 23rd row is outside the world and must not be composited.
        assert_eq!(MINIMAP_ROWS + 1, 23);
    }
    #[test]
    fn client_targets() {
        let d=parse_destinations("3801\t\"Archosaur, North District\"\t1\t\"1470.490,219.608,1180.864\"\t23\n1\t\"Other\"\t2\t\"1,2,3\"\t0").unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].position, [1470.490, 219.608, 1180.864]);
    }
    #[test]
    fn orientation_roundtrip_and_terrain() {
        let m = MapSpace {
            width: 4,
            height: 4,
            spacing: 1.,
            heights: Arc::from(vec![
                0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12., 13., 14., 15.,
            ]),
        };
        assert_eq!(m.from_uv(0., 0.), [-2., 2.]);
        assert_eq!(m.from_uv(1., 1.), [2., -2.]);
        let p = m.from_uv(0.7, 0.2);
        let uv = m.to_uv(p[0], p[1]);
        assert!((uv[0] - 0.7).abs() < 0.00001 && (uv[1] - 0.2).abs() < 0.00001);
        assert!((m.ground(-0.75, 0.75) - 6.25).abs() < 0.0001);
    }
}
