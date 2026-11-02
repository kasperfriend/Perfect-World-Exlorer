//! Binary A3DLitModel reader for world BMDs from this client.
//! Layout informed by GameForge's GPL-2.0-or-later Angelica importer.
#[derive(Clone)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub diffuse: u32,
    pub day_diffuse: u32,
    pub night_diffuse: u32,
}
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub emissive: [f32; 4],
    pub specular: [f32; 4],
    pub power: f32,
    pub two_sided: bool,
}
pub struct Mesh {
    pub name: String,
    pub version: u32,
    pub material: Option<Material>,
    pub texture: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}
pub struct Model {
    pub scale: [f32; 3],
    pub direction: [f32; 3],
    pub up: [f32; 3],
    pub position: [f32; 3],
    pub meshes: Vec<Mesh>,
}
fn u(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p.checked_add(4).ok_or("overflow")?)
            .ok_or("truncated BMD")?
            .try_into()
            .unwrap(),
    ))
}
fn f(b: &[u8], p: usize) -> Result<f32, String> {
    let v = f32::from_bits(u(b, p)?);
    if !v.is_finite() {
        return Err("non-finite BMD float".into());
    }
    Ok(v)
}
fn v3(b: &[u8], p: usize) -> Result<[f32; 3], String> {
    Ok([f(b, p)?, f(b, p + 4)?, f(b, p + 8)?])
}
fn fixed(b: &[u8], p: usize, n: usize) -> Result<String, String> {
    let s = b
        .get(p..p.checked_add(n).ok_or("overflow")?)
        .ok_or("truncated string")?;
    let end = s.iter().position(|&c| c == 0).unwrap_or(n);
    let (t, _, bad) = encoding_rs::GBK.decode(&s[..end]);
    if bad {
        return Err("invalid GBK in BMD".into());
    }
    Ok(t.replace('\\', "/"))
}
impl Model {
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        if u(b, 0)? != 0x42584f4d {
            return Err("not a binary AFile".into());
        }
        let mut p = 4;
        let mut version = u(b, p)?;
        if version == 0x80000001 {
            p += 5;
            version = u(b, p)?
        }
        if version != 0x10000002 {
            return Err(format!("unsupported BMD version {version:#x}"));
        }
        p += 4;
        let scale = v3(b, p)?;
        let direction = v3(b, p + 12)?;
        let up = v3(b, p + 24)?;
        let position = v3(b, p + 36)?;
        p += 48;
        let count = u(b, p)? as usize;
        p += 4;
        if count > 4096 {
            return Err("too many meshes".into());
        }
        let mut meshes = Vec::with_capacity(count);
        for _ in 0..count {
            let ver = u(b, p)?;
            p += 4;
            if ver != 0x10000004 && ver != 0x10000005 {
                return Err(format!("unsupported mesh version {ver:#x}"));
            }
            let name = fixed(b, p, 64)?;
            let texture = fixed(b, p + 64, 256)?;
            p += 320;
            let n = u(b, p)? as usize;
            let faces = u(b, p + 4)? as usize;
            p += 8;
            if n > 1_000_000 || faces > 2_000_000 {
                return Err("invalid BMD mesh size".into());
            }
            let needed = n
                .checked_mul(24)
                .and_then(|v| v.checked_add(faces * 6))
                .and_then(|v| v.checked_add(n * 20))
                .and_then(|v| v.checked_add(48))
                .ok_or("BMD size overflow")?;
            if p.checked_add(needed).is_none_or(|e| e > b.len()) {
                return Err("truncated BMD mesh data".into());
            }
            let mut vertices = Vec::with_capacity(n);
            for i in 0..n {
                let q = p + i * 24;
                vertices.push(Vertex {
                    position: v3(b, q)?,
                    diffuse: u(b, q + 12)?,
                    day_diffuse: 0,
                    night_diffuse: 0,
                    uv: [f(b, q + 16)?, f(b, q + 20)?],
                    normal: [0.; 3],
                });
            }
            p += n * 24;
            let mut indices = Vec::with_capacity(faces * 3);
            for _ in 0..faces * 3 {
                let s = b.get(p..p + 2).ok_or("truncated BMD indices")?;
                let i = u16::from_le_bytes([s[0], s[1]]);
                if i as usize >= n {
                    return Err("BMD vertex index out of range".into());
                }
                indices.push(i);
                p += 2;
            }
            for v in &mut vertices {
                v.normal = v3(b, p)?;
                p += 12;
            }
            // The client loads two packed vertex-light arrays into mesh
            // members +0x284/+0x288 and blends them at 0x7b9bc0.
            for v in &mut vertices {
                v.day_diffuse = u(b, p)?;
                p += 4;
            }
            for v in &mut vertices {
                v.night_diffuse = u(b, p)?;
                p += 4;
            }
            p += 48;
            let material = if ver >= 0x10000005 {
                let tail = b.get(p..).ok_or("truncated material")?;
                let len = tail
                    .iter()
                    .position(|&c| c == 0)
                    .ok_or("unterminated material name")?;
                if len > 4096 {
                    return Err("oversized BMD material name".into());
                }
                let name = fixed(b, p, len)?;
                p = p.checked_add(len + 1).ok_or("material overflow")?;
                let color = |off| -> Result<[f32; 4], String> {
                    Ok([f(b, off)?, f(b, off + 4)?, f(b, off + 8)?, f(b, off + 12)?])
                };
                let m = Material {
                    name,
                    ambient: color(p)?,
                    diffuse: color(p + 16)?,
                    emissive: color(p + 32)?,
                    specular: color(p + 48)?,
                    power: f(b, p + 64)?,
                    two_sided: *b.get(p + 68).ok_or("truncated material")? != 0,
                };
                p = p.checked_add(69).ok_or("material overflow")?;
                Some(m)
            } else {
                None
            };
            meshes.push(Mesh {
                name,
                version: ver,
                material,
                texture,
                vertices,
                indices,
            });
        }
        Ok(Self {
            scale,
            direction,
            up,
            position,
            meshes,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_short_file() {
        assert!(Model::parse(&[0; 8]).is_err());
    }
    #[test]
    fn reads_both_baked_light_arrays() {
        let mut b = Vec::new();
        let u = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        u(&mut b, 0x42584f4d);
        u(&mut b, 0x10000002);
        b.extend_from_slice(&[0; 48]);
        u(&mut b, 1);
        u(&mut b, 0x10000004);
        b.extend_from_slice(&[0; 320]);
        u(&mut b, 2);
        u(&mut b, 0);
        b.extend_from_slice(&[0; 48]); // two vertex records
        b.extend_from_slice(&[0; 24]); // two normals
        u(&mut b, 0xff123456);
        u(&mut b, 0xff789abc);
        u(&mut b, 0xfffedcba);
        u(&mut b, 0xff654321);
        b.extend_from_slice(&[0; 48]);
        let m = Model::parse(&b).unwrap();
        let v = &m.meshes[0].vertices;
        assert_eq!(
            (v[0].day_diffuse, v[0].night_diffuse),
            (0xff123456, 0xfffedcba)
        );
        assert_eq!(
            (v[1].day_diffuse, v[1].night_diffuse),
            (0xff789abc, 0xff654321)
        );
    }
    #[test]
    fn reads_version_five_material_and_rejects_truncation() {
        let mut b = Vec::new();
        let put = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        put(&mut b, 0x42584f4d);
        put(&mut b, 0x10000002);
        b.extend_from_slice(&[0; 48]);
        put(&mut b, 1);
        put(&mut b, 0x10000005);
        b.extend_from_slice(&[0; 320]);
        put(&mut b, 0);
        put(&mut b, 0);
        b.extend_from_slice(&[0; 48]);
        b.extend_from_slice(b"MATERIAL: \0");
        for x in 0..17 {
            put(&mut b, (x as f32 / 16.0).to_bits());
        }
        b.push(1);
        let model = Model::parse(&b).unwrap();
        let m = model.meshes[0].material.as_ref().unwrap();
        assert_eq!(m.name, "MATERIAL: ");
        assert_eq!(m.diffuse, [0.25, 0.3125, 0.375, 0.4375]);
        assert_eq!(m.power, 1.0);
        assert!(m.two_sided);
        b.pop();
        assert!(Model::parse(&b).is_err());
    }
    #[test]
    fn rejects_nan_geometry() {
        assert!(f(&f32::NAN.to_le_bytes(), 0).is_err());
    }
}
