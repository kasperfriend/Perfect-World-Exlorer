//! Experimental decoding of client grass placement masks and MOXB geometry.
//! EC-BSD loader and MSB-first bit order traced in elementclient.exe.
#[path = "grass_random.rs"]
pub mod random;
use crate::{pck, scene, water};
use flate2::read::ZlibDecoder;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

#[derive(Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}
#[derive(Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub texture: String,
}
#[derive(Debug)]
pub struct Model {
    pub meshes: Vec<Mesh>,
}
pub struct Area {
    pub type_id: usize,
    pub center: [f32; 3],
    pub grid: f32,
    pub width: usize,
    pub height: usize,
    pub active: Vec<u32>,
    pub placements: Vec<random::Placement>,
}
pub struct Grass {
    pub areas: Vec<Area>,
    pub types: Vec<scene::GrassType>,
    pub models: Vec<Option<Model>>,
    pub textures: HashMap<String, image::RgbaImage>,
    /// Authored lower mip levels keyed by the same texture path as `textures`.
    /// The default world path ignores these; P4.07 opt-in uploads them verbatim.
    pub texture_mips: HashMap<String, Vec<image::RgbaImage>>,
}
fn cstr<'a>(b: &'a [u8], start: usize) -> Result<(&'a [u8], usize), String> {
    let bytes = b.get(start..).ok_or("invalid MOXB string")?;
    let n = bytes
        .iter()
        .position(|x| *x == 0)
        .ok_or("unterminated MOXB string")?;
    if n > 4096 {
        return Err("oversized MOXB string".into());
    }
    Ok((&bytes[..n], start + n + 1))
}
fn find(b: &[u8], p: usize, s: &[u8]) -> Result<usize, String> {
    let data = b.get(p..).ok_or("bad MOXB offset")?;
    data.windows(s.len())
        .position(|w| w == s)
        .map(|n| n + p)
        .ok_or_else(|| "missing MOXB marker".into())
}
fn u(b: &[u8], p: usize) -> Result<u32, String> {
    water::u(b, p)
}
fn f(b: &[u8], p: usize) -> Result<f32, String> {
    water::f(b, p)
}
fn parse_model(b: &[u8], name: &str) -> Result<Model, String> {
    if !b.starts_with(b"MOXB") {
        return Err("unsupported MOXB header".into());
    }
    let mut frame = 4;
    if b.get(frame..).is_some_and(|s| s.starts_with(b"VERSION: ")) {
        let (version, end) = cstr(b, frame)?;
        if version != b"VERSION: 2" {
            return Err("unsupported MOXB version".into());
        }
        frame = end;
    }
    if !b.get(frame..).is_some_and(|s| s.starts_with(b"FRAME: ")) {
        return Err("missing MOXB frame".into());
    }
    let (_, mut q) = cstr(b, frame)?;
    let frames = u(b, q)? as usize;
    q += 4;
    if frames != 1 {
        return Err("animated MOXB frames unsupported".into());
    }
    q += 64;
    let children = u(b, q)?;
    let count = u(b, q + 4)? as usize;
    q += 8;
    if children != 0 || count == 0 || count > 64 {
        return Err("unsupported MOXB hierarchy".into());
    }
    let dir = name.rsplit_once('/').map(|x| x.0).unwrap_or("");
    let mut meshes = Vec::new();
    for _ in 0..count {
        let st = find(b, q, b"MESH: ")?;
        let (_, p) = cstr(b, st)?;
        let nv = u(b, p)? as usize;
        let ni = u(b, p + 4)? as usize;
        let nf = u(b, p + 8)? as usize;
        if nv == 0 || nv > 100_000 || ni == 0 || ni > 300_000 || ni % 3 != 0 || nf != 1 {
            return Err("unsupported MOXB mesh".into());
        }
        let tex = find(b, p, b"TEXTURE: ")?;
        let vertbytes = nv.checked_mul(32).ok_or("MOXB vertex overflow")?;
        let indbytes = ni.checked_mul(2).ok_or("MOXB index overflow")?;
        let inds = tex
            .checked_sub(vertbytes + indbytes)
            .ok_or("short MOXB mesh")?;
        if inds < p + 18 {
            return Err("bad MOXB mesh offsets".into());
        }
        let mut indices = Vec::with_capacity(ni);
        for i in 0..ni {
            let k = inds + i * 2;
            let value = u16::from_le_bytes(
                b.get(k..k + 2)
                    .ok_or("short MOXB index")?
                    .try_into()
                    .unwrap(),
            ) as u32;
            if value as usize >= nv {
                return Err("MOXB index out of range".into());
            }
            indices.push(value)
        }
        let mut vertices = Vec::with_capacity(nv);
        let base = inds + indbytes;
        for i in 0..nv {
            let p = base + i * 32;
            vertices.push(Vertex {
                pos: [f(b, p)?, f(b, p + 4)?, f(b, p + 8)?],
                normal: [f(b, p + 12)?, f(b, p + 16)?, f(b, p + 20)?],
                uv: [f(b, p + 24)?, f(b, p + 28)?],
            })
        }
        let (t, end) = cstr(b, tex + 9)?;
        let (decoded, _, bad) = encoding_rs::GBK.decode(t);
        if bad {
            return Err("invalid MOXB texture path".into());
        }
        let t = decoded.replace('\\', "/").to_lowercase();
        if t.contains("..") || t.starts_with('/') || t.contains(':') {
            return Err("unsafe MOXB texture path".into());
        }
        let texture = format!("{dir}/textures/{t}");
        meshes.push(Mesh {
            vertices,
            indices,
            texture,
        });
        q = end;
    }
    Ok(Model { meshes })
}
fn mask(b: &[u8], expected: usize) -> Result<Vec<u32>, String> {
    if b.len() != expected {
        return Err("bad grass bitmap length".into());
    }
    let mut out = Vec::new();
    for (i, &byte) in b.iter().enumerate() {
        for k in 0..8 {
            if byte & (0x80 >> k) != 0 {
                out.push((i * 8 + k) as u32)
            }
        }
    }
    Ok(out)
}
impl Grass {
    pub fn load(scene: &scene::Scene, dir: &Path, element: &Path) -> Result<Self, String> {
        let b = fs::read(dir.join("world.ecbsd")).map_err(|e| e.to_string())?;
        if u(&b, 0)? != 0x42584f4d || u(&b, 4)? != 0x42534446 || u(&b, 8)? != 12 {
            return Err("unsupported grass EC-BSD".into());
        }
        let n = u(&b, 12)? as usize;
        if n > 10000 {
            return Err("invalid grass BSData count".into());
        }
        let mut p = 0x4c;
        let mut names = Vec::new();
        for _ in 0..n {
            names.push(water::path_string(&b, &mut p)?)
        }
        let mut files: HashMap<usize, Vec<u8>> = HashMap::new();
        let mut seen = HashMap::new();
        let mut areas = Vec::new();
        for block in &scene.blocks {
            for r in &block.grass {
                if let Some(previous) = seen.insert(r.export_id, r.offset) {
                    if previous != r.offset {
                        return Err("inconsistent grass record".into());
                    }
                    continue;
                }
                let off = r.offset as usize;
                let type_id = u(&b, off)? as usize;
                if type_id >= scene.grass_types.len() {
                    return Err("grass type index".into());
                }
                let center = [f(&b, off + 4)?, f(&b, off + 8)?, f(&b, off + 12)?];
                let grid = f(&b, off + 16)?;
                let width = u(&b, off + 20)? as usize;
                let height = u(&b, off + 24)? as usize;
                if width == 0
                    || height == 0
                    || width > 4096
                    || height > 4096
                    || width % 8 != 0
                    || grid <= 0.
                    || grid > 10000.
                {
                    return Err("invalid grass bitmap dimensions".into());
                }
                let bytes = width.checked_mul(height).ok_or("grass mask overflow")? / 8;
                let index = u(&b, off + 28)? as usize;
                let rec = u(&b, off + 32)? as usize;
                if !files.contains_key(&index) {
                    let name = names.get(index).ok_or("grass BSData index")?;
                    let data = fs::read(water::resolve_case_insensitive(dir, name)?)
                        .map_err(|e| e.to_string())?;
                    files.insert(index, data);
                }
                let data = &files[&index];
                if u(data, 0)? != 0x42584f4d || u(data, 4)? != 2 {
                    return Err("unsupported grass BSData".into());
                }
                let tail = data.len().checked_sub(4).ok_or("short grass BSData")?;
                let count = u(data, tail)? as usize;
                let table = tail
                    .checked_sub(count.checked_mul(4).ok_or("grass table overflow")?)
                    .ok_or("bad grass table")?;
                if rec >= count {
                    return Err("grass record index".into());
                }
                let start = u(data, table + rec * 4)? as usize;
                let stored = u(data, start)? as usize;
                let raw = u(data, start + 4)? as usize;
                if raw != bytes || stored > bytes * 2 + 1024 {
                    return Err("invalid grass compressed size".into());
                }
                let src = data
                    .get(start + 8..start + 8 + stored)
                    .ok_or("short grass compressed bitmap")?;
                let mut output = Vec::new();
                ZlibDecoder::new(src)
                    .take((bytes + 1) as u64)
                    .read_to_end(&mut output)
                    .map_err(|e| e.to_string())?;
                let active = mask(&output, bytes)?;
                let typ = &scene.grass_types[type_id];
                let placements =
                    random::placements(width, height, &active, typ.size, typ.size_variation);
                areas.push(Area {
                    type_id,
                    center,
                    grid,
                    width,
                    height,
                    active,
                    placements,
                });
            }
        }
        let mut archive = File::open(element.join("grasses.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<_, _> = pck::directory(&mut archive)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        let mut models = Vec::new();
        let mut textures = HashMap::new();
        let mut texture_mips = HashMap::new();
        let mut warnings = HashSet::new();
        for t in &scene.grass_types {
            let name = t.path.to_lowercase();
            let model = match entries.get(&name) {
                Some(e) => match pck::read_entry(&mut archive, e)
                    .and_then(|bytes| parse_model(&bytes, &name))
                {
                    Ok(model) => Some(model),
                    Err(error) => {
                        eprintln!("Unsupported grass model {name}: {error}");
                        None
                    }
                },
                None => {
                    eprintln!("Missing grass model {name}");
                    None
                }
            };
            if let Some(m) = &model {
                for mesh in &m.meshes {
                    if !textures.contains_key(&mesh.texture) {
                        let alt = mesh.texture.replace("/textures/", "/");
                        // One retail asset names its DDS with a space while the
                        // matching archive member uses an underscore.
                        let spaced = mesh.texture.replace(' ', "_");
                        let entry = entries
                            .get(&mesh.texture)
                            .or_else(|| entries.get(&alt))
                            .or_else(|| entries.get(&spaced));
                        if let Some(entry) = entry {
                            if let Ok(bytes) = pck::read_entry(&mut archive, entry) {
                                if let Ok(img) = image::load_from_memory(&bytes) {
                                    textures.insert(mesh.texture.clone(), img.to_rgba8());
                                    match crate::vegetation_textures::authored_lower_mips(&bytes) {
                                        Ok(mips) => {
                                            texture_mips.insert(mesh.texture.clone(), mips);
                                        }
                                        Err(e) => eprintln!(
                                            "Grass texture {} authored mips unavailable: {e}",
                                            mesh.texture
                                        ),
                                    }
                                    continue;
                                }
                            }
                        }
                        warnings.insert(mesh.texture.clone());
                    }
                }
            }
            models.push(model)
        }
        for path in &warnings {
            eprintln!("Missing or unsupported grass texture: {path}");
        }
        eprintln!("Loaded {} grass areas, {} active bits, {}/{} MOXB types, {} textures ({} texture paths unresolved)",areas.len(),areas.iter().map(|a|a.active.len()).sum::<usize>(),models.iter().filter(|x|x.is_some()).count(),models.len(),textures.len(),warnings.len());
        Ok(Self {
            areas,
            types: scene.grass_types.clone(),
            models,
            textures,
            texture_mips,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bits_are_most_significant_first() {
        assert_eq!(mask(&[0x81, 0x40], 2).unwrap(), vec![0, 7, 9]);
        assert!(mask(&[0], 2).is_err())
    }
    #[test]
    fn truncated_mox_fails() {
        assert!(parse_model(b"MOXBFRAME: \0", "a/b").is_err());
    }
    #[test]
    fn parses_versioned_triangle_and_rejects_bad_index() {
        let mut b = b"MOXBVERSION: 2\0FRAME: \0".to_vec();
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 64]);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(b"VERSION: 2\0MESH: \0");
        for v in [3u32, 3, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[0u8; 10]);
        let index = b.len();
        for v in [0u16, 1, 2] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for i in 0..3 {
            for v in [i as f32, 0., 0., 0., 1., 0., 0., 0.] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b.extend_from_slice(b"TEXTURE: x.dds\0");
        let m = parse_model(&b, "grasses/a/x.mox").unwrap();
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2]);
        assert_eq!(m.meshes[0].vertices[2].pos[0], 2.);
        assert_eq!(m.meshes[0].texture, "grasses/a/textures/x.dds");
        b[index + 4..index + 6].copy_from_slice(&3u16.to_le_bytes());
        assert!(parse_model(&b, "grasses/a/x.mox").is_err());
    }
}
