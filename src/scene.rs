//! ECWLD v12 scene blocks from the Perfect World client.
//! Layout informed by https://forum.ragezone.com/threads/map-editor.1252114/
use std::{collections::HashSet, fs, path::Path};
#[derive(Clone, Debug)]
pub struct TreeType {
    pub tree: String,
    pub composite: String,
}
#[derive(Clone, Debug)]
pub struct GrassType {
    pub path: String,
    // Raw 4-byte serialized slot: only its low byte is the bool; padding
    // contains nonzero legacy bytes. Retail grass initialization forces false.
    pub alpha_blend: i32,
    pub alpha_ref: i32,
    pub size: f32,
    pub size_variation: f32,
    pub softness: f32,
    pub sight_range: f32,
}
#[derive(Clone, Debug)]
pub struct Tree {
    pub tree_type: u32,
    pub position: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Reference {
    pub export_id: u32,
    pub offset: u32,
}
#[derive(Clone, Debug)]
pub struct SceneModel {
    pub export_id: u32,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub up: [f32; 3],
    pub path: String,
    pub action: String,
}
#[derive(Clone, Debug)]
pub struct Effect {
    pub export_id: u32,
    pub position: [f32; 3],
    pub path: String,
}
#[derive(Clone, Debug)]
pub struct Sound {
    pub export_id: u32,
    pub position: [f32; 3],
    pub path: String,
}
#[derive(Clone, Debug)]
pub struct Block {
    pub row: u16,
    pub col: u16,
    pub trees: Vec<Tree>,
    pub water: Vec<Reference>,
    pub grass: Vec<Reference>,
    pub ornaments: Vec<Reference>,
    pub boxes: Vec<Reference>,
    pub effects: Vec<Effect>,
    pub models: Vec<SceneModel>,
    pub critters: Vec<Reference>,
    pub beziers: Vec<Reference>,
    pub sounds: Vec<Sound>,
}
pub struct Scene {
    pub width: f32,
    pub length: f32,
    pub block_size: f32,
    pub rows: usize,
    pub cols: usize,
    pub tree_types: Vec<TreeType>,
    pub grass_types: Vec<GrassType>,
    pub blocks: Vec<Block>,
}
fn u32at(b: &[u8], p: usize) -> Result<u32, String> {
    let end = p.checked_add(4).ok_or("overflow")?;
    Ok(u32::from_le_bytes(
        b.get(p..end).ok_or("truncated ECWLD")?.try_into().unwrap(),
    ))
}
fn f32at(b: &[u8], p: usize) -> Result<f32, String> {
    let f = f32::from_bits(u32at(b, p)?);
    if !f.is_finite() {
        return Err("non-finite ECWLD float".into());
    }
    Ok(f)
}
fn string(b: &[u8], p: &mut usize) -> Result<String, String> {
    let n = u32at(b, *p)? as usize;
    *p += 4;
    if n > 4096 {
        return Err("oversized ECWLD string".into());
    }
    let e = p.checked_add(n).ok_or("string overflow")?;
    let bytes = b.get(*p..e).ok_or("truncated ECWLD string")?;
    *p = e;
    let (s, _, bad) = encoding_rs::GBK.decode(bytes);
    if bad {
        return Err("invalid GBK scene string".into());
    }
    Ok(s.replace('\\', "/"))
}
fn vec3(b: &[u8], p: usize) -> Result<[f32; 3], String> {
    Ok([f32at(b, p)?, f32at(b, p + 4)?, f32at(b, p + 8)?])
}
fn refs(b: &[u8], p: &mut usize, n: usize) -> Result<Vec<Reference>, String> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(Reference {
            export_id: u32at(b, *p)?,
            offset: u32at(b, *p + 4)?,
        });
        *p += 8;
    }
    Ok(out)
}
impl Scene {
    pub fn load(path: &Path) -> Result<Self, String> {
        let b = fs::read(path).map_err(|e| e.to_string())?;
        Self::parse(&b)
    }
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        if u32at(b, 0)? != 0x45435744 || u32at(b, 4)? != 12 {
            return Err("only ECWLD version 12 is supported".into());
        }
        let width = f32at(b, 8)?;
        let length = f32at(b, 12)?;
        let block_size = f32at(b, 16)?;
        let count = u32at(b, 24)? as usize;
        let rows = u32at(b, 28)? as usize;
        let cols = u32at(b, 32)? as usize;
        let tree_off = u32at(b, 36)? as usize;
        let grass_off = u32at(b, 40)? as usize;
        let tree_count = u32at(b, 44)? as usize;
        let grass_count = u32at(b, 48)? as usize;
        if count == 0
            || rows.checked_mul(cols) != Some(count)
            || count > 1_000_000
            || tree_count > 10000
            || grass_count > 10000
            || tree_off > grass_off
            || grass_off > b.len()
        {
            return Err("invalid ECWLD header".into());
        }
        let offsets: Vec<usize> = (0..count)
            .map(|i| u32at(b, 116 + i * 4).map(|v| v as usize))
            .collect::<Result<_, _>>()?;
        let mut order = offsets.clone();
        order.sort_unstable();
        if order.iter().any(|&x| x < 116 + 4 * count || x >= tree_off)
            || order.windows(2).any(|w| w[0] == w[1])
        {
            return Err("invalid ECWLD block offsets".into());
        }
        let mut boundaries = std::collections::HashMap::new();
        for i in 0..count {
            boundaries.insert(
                order[i],
                if i + 1 < count {
                    order[i + 1]
                } else {
                    tree_off
                },
            );
        }
        let mut tp = tree_off;
        let mut tree_types = Vec::with_capacity(tree_count);
        for _ in 0..tree_count {
            tree_types.push(TreeType {
                tree: string(b, &mut tp)?,
                composite: string(b, &mut tp)?,
            });
        }
        if tp != grass_off {
            return Err("tree type table does not end at grass table".into());
        }
        let mut gp = grass_off;
        let mut grass_types = Vec::with_capacity(grass_count);
        for _ in 0..grass_count {
            let g = GrassType {
                alpha_blend: u32at(b, gp)? as i32,
                alpha_ref: u32at(b, gp + 4)? as i32,
                size: f32at(b, gp + 8)?,
                size_variation: f32at(b, gp + 12)?,
                softness: f32at(b, gp + 16)?,
                sight_range: f32at(b, gp + 20)?,
                path: {
                    gp += 24;
                    string(b, &mut gp)?
                },
            };
            grass_types.push(g)
        }
        if gp != b.len() {
            return Err(format!(
                "unparsed bytes after grass type table: {}",
                b.len() - gp
            ));
        }
        let mut blocks = Vec::with_capacity(count);
        let mut coords = HashSet::new();
        for off in offsets {
            let end = boundaries[&off];
            let data = &b[off..end];
            let row = u16::from_le_bytes(data.get(0..2).ok_or("short block")?.try_into().unwrap());
            let col = u16::from_le_bytes(data.get(2..4).ok_or("short block")?.try_into().unwrap());
            if row as usize >= rows || col as usize >= cols || !coords.insert((row, col)) {
                return Err("invalid or duplicate block coordinates".into());
            }
            let mut n = [0usize; 10];
            for i in 0..10 {
                n[i] = u32at(data, 4 + i * 4)? as usize;
                if n[i] > 100_000 {
                    return Err("invalid block object count".into());
                }
            }
            let mut p = 44;
            let mut trees = Vec::with_capacity(n[0]);
            for _ in 0..n[0] {
                let tree_type = u32at(data, p)?;
                if tree_type as usize >= tree_count {
                    return Err("invalid tree type".into());
                }
                trees.push(Tree {
                    tree_type,
                    position: vec3(data, p + 4)?,
                });
                p += 16;
            }
            let water = refs(data, &mut p, n[1])?;
            let ornaments = refs(data, &mut p, n[3])?;
            let boxes = refs(data, &mut p, n[4])?;
            let grass = refs(data, &mut p, n[2])?;
            let mut effects = Vec::with_capacity(n[5]);
            for _ in 0..n[5] {
                let export_id = u32at(data, p)?;
                let position = vec3(data, p + 8)?;
                p += 60;
                let path = string(data, &mut p)?;
                effects.push(Effect {
                    export_id,
                    position,
                    path,
                })
            }
            let mut models = Vec::with_capacity(n[6]);
            for _ in 0..n[6] {
                let export_id = u32at(data, p)?;
                let position = vec3(data, p + 4)?;
                let direction = vec3(data, p + 16)?;
                let up = vec3(data, p + 28)?;
                p += 40;
                let path = string(data, &mut p)?;
                let action = string(data, &mut p)?;
                models.push(SceneModel {
                    export_id,
                    position,
                    direction,
                    up,
                    path,
                    action,
                })
            }
            let critters = refs(data, &mut p, n[7])?;
            let beziers = refs(data, &mut p, n[8])?;
            let mut sounds = Vec::with_capacity(n[9]);
            for _ in 0..n[9] {
                let export_id = u32at(data, p)?;
                let position = vec3(data, p + 4)?;
                p += 32;
                let path = string(data, &mut p)?;
                sounds.push(Sound {
                    export_id,
                    position,
                    path,
                })
            }
            if p != data.len() {
                return Err(format!(
                    "block ({row},{col}) length mismatch: consumed {p}, actual {}",
                    data.len()
                ));
            }
            blocks.push(Block {
                row,
                col,
                trees,
                water,
                grass,
                ornaments,
                boxes,
                effects,
                models,
                critters,
                beziers,
                sounds,
            });
        }
        Ok(Self {
            width,
            length,
            block_size,
            rows,
            cols,
            tree_types,
            grass_types,
            blocks,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Ornament {
    pub export_id: u32,
    pub offset: u32,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub up: [f32; 3],
    pub extra: u32,
    pub path: String,
}
impl Scene {
    /// Read placed lit-model references in the ECWLD's associated EC-BSD file.
    /// Blocks can refer to the same export ID, so each instance is returned once.
    pub fn load_ornaments(&self, path: &Path) -> Result<Vec<Ornament>, String> {
        let b = fs::read(path).map_err(|e| e.to_string())?;
        if u32at(&b, 0)? != 0x42584f4d || u32at(&b, 4)? != 0x42534446 || u32at(&b, 8)? != 12 {
            return Err("unsupported EC-BSD header".into());
        }
        let mut seen = std::collections::HashMap::<u32, u32>::new();
        let mut result = Vec::new();
        for block in &self.blocks {
            for r in &block.ornaments {
                if let Some(old) = seen.insert(r.export_id, r.offset) {
                    if old != r.offset {
                        return Err(format!("ornament {} has inconsistent offsets", r.export_id));
                    }
                    continue;
                }
                let p = r.offset as usize;
                let position = vec3(&b, p)?;
                let direction = vec3(&b, p + 12)?;
                let up = vec3(&b, p + 24)?;
                let extra = u32at(&b, p + 36)?;
                let mut pos = p + 40;
                let path = string(&b, &mut pos)?;
                if path.is_empty() || !path.to_lowercase().ends_with(".bmd") {
                    return Err(format!(
                        "ornament {} has unexpected BMD path {path:?}",
                        r.export_id
                    ));
                }
                result.push(Ornament {
                    export_id: r.export_id,
                    offset: r.offset,
                    position,
                    direction,
                    up,
                    extra,
                    path,
                });
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_header() {
        assert!(Scene::parse(&[0; 116]).is_err());
    }
    #[test]
    fn rejects_short_string() {
        let mut p = 0;
        assert!(string(&[9, 0, 0, 0, 1], &mut p).is_err());
    }
    #[test]
    fn decodes_chinese_path() {
        let mut p = 0;
        let path = string(&[4, 0, 0, 0, 0xd1, 0xa9, 0xb5, 0xd8], &mut p).unwrap();
        assert_eq!(path, "雪地");
    }
}
