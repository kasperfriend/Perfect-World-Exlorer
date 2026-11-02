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
/// Client map tiles are named ROW then COLUMN: 000..107, 11 rows x 8 columns.
/// Downsample each 1024px image for a practical single atlas in the map UI.
pub fn original_map(element: &Path) -> Result<image::RgbaImage, String> {
    let path = element.join("surfaces.pck");
    let mut f = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let entries = pck::directory(&mut f)?;
    let side = 256u32;
    let mut map = image::RgbaImage::new(8 * side, 11 * side);
    for row in 0..11 {
        for col in 0..8 {
            let name = format!("surfaces/maps/{row:02}{col}.dds");
            let entry = entries
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(&name))
                .ok_or_else(|| format!("missing map tile {name}"))?;
            let data = pck::read_entry(&mut f, entry)?;
            let tile = image::load_from_memory_with_format(&data, image::ImageFormat::Dds)
                .map_err(|e| format!("{name}: {e}"))?
                .to_rgba8();
            let tile =
                image::imageops::resize(&tile, side, side, image::imageops::FilterType::Triangle);
            image::imageops::replace(&mut map, &tile, (col * side) as i64, (row * side) as i64);
        }
    }
    Ok(map)
}
#[cfg(test)]
mod tests {
    use super::*;
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
