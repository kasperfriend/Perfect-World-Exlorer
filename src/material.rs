//! Main-world T2MK v2 material layers and DDS texture access.
//! Format informed by the GPL-2.0-or-later GameForge Angelica importer.
use crate::{f32at, pck, u32at};
use flate2::read::ZlibDecoder;
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};
pub(crate) struct Texture {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) pixels: Vec<u8>,
}
pub(crate) struct Layer {
    pub(crate) texture: usize,
    pub(crate) axis: u8,
    pub(crate) u: f32,
    pub(crate) v: f32,
    pub(crate) mask_size: usize,
    pub(crate) mask: Vec<u8>,
}
pub(crate) struct Area {
    pub(crate) layers: Vec<Layer>,
}
pub struct Materials {
    pub(crate) textures: Vec<Texture>,
    pub(crate) areas: Vec<Area>,
    pub(crate) areas_x: usize,
    pub(crate) mask_grid: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) spacing: f32,
}
fn gbk_string(b: &[u8], p: &mut usize) -> Result<String, String> {
    let len = u32at(b, *p)? as usize;
    *p += 4;
    if len > 4096 {
        return Err("oversized name".into());
    }
    let bytes = b.get(*p..*p + len).ok_or("truncated name")?;
    *p += len;
    let (s, _, bad) = encoding_rs::GBK.decode(bytes);
    if bad {
        return Err("invalid GBK path".into());
    }
    Ok(s.replace('\\', "/"))
}
impl Materials {
    pub fn load(
        dir: &Path,
        trn: &[u8],
        names: &[String],
        width: usize,
        height: usize,
        spacing: f32,
    ) -> Result<Self, String> {
        let mask_grid = u32at(trn, 48)? as usize;
        let subgrid = u32at(trn, 64)? as usize;
        if mask_grid == 0
            || subgrid % mask_grid != 0
            || width % mask_grid != 0
            || height % mask_grid != 0
        {
            return Err("unsupported mask grid".into());
        }
        let areas_x = width / mask_grid;
        let areas_y = height / mask_grid;
        let sub_areas = subgrid / mask_grid;
        let subs_x = width / subgrid;
        let mut pos = u32at(trn, 72)? as usize;
        let count = u32at(trn, 36)? as usize;
        if count > 4096 {
            return Err("too many textures".into());
        }
        let mut paths = Vec::new();
        for _ in 0..count {
            paths.push(gbk_string(trn, &mut pos)?)
        }
        let element = dir.parent().and_then(Path::parent).ok_or("bad map path")?;
        let mut file =
            File::open(element.join("textures.pck")).map_err(|e| format!("textures.pck: {e}"))?;
        let entries = pck::directory(&mut file)?;
        let mut textures = Vec::new();
        for path in &paths {
            let entry = entries
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(path))
                .ok_or_else(|| format!("missing texture {path}"))?;
            let raw = pck::read_entry(&mut file, entry)?;
            // Terrain sources must be one of the compressed formats the client
            // can create; anything else is an explicit error naming the file
            // instead of an `image` crate fallback that silently decodes a
            // format the native device would have rejected.
            let info = crate::terrain_sampling::parse_terrain_dds(&raw)
                .map_err(|e| format!("texture {path}: {e}"))?;
            let img = image::load_from_memory_with_format(&raw, image::ImageFormat::Dds)
                .map_err(|e| format!("texture {path}: {e}"))?
                .to_rgb8();
            if (info.width, info.height) != (img.width(), img.height()) {
                return Err(format!(
                    "texture {path}: header says {}x{} but decoded {}x{}",
                    info.width,
                    info.height,
                    img.width(),
                    img.height()
                ));
            }
            let (w, h) = (info.width as usize, info.height as usize);
            textures.push(Texture {
                w,
                h,
                pixels: img.into_raw(),
            });
        }
        let mut areas: Vec<Area> = (0..areas_x * areas_y)
            .map(|_| Area { layers: Vec::new() })
            .collect();
        for (si, name) in names.iter().enumerate() {
            let data = fs::read(dir.join(format!("{name}.t2mk"))).map_err(|e| e.to_string())?;
            if u32at(&data, 0)? != 0x544d4b42 || u32at(&data, 4)? != 2 {
                return Err(format!("{name}: unsupported T2MK"));
            }
            let area_count = u32at(&data, 8)? as usize;
            let layer_count = u32at(&data, 12)? as usize;
            if area_count != sub_areas * sub_areas || layer_count > 4096 {
                return Err(format!("{name}: invalid layer count"));
            }
            let layerstart = 20 + area_count * 4;
            for ai in 0..area_count {
                let p = u32at(&data, 20 + ai * 4)? as usize;
                let n = u32at(&data, p)? as usize;
                if n > 256 {
                    return Err("too many layers".into());
                }
                let dst = ((si / subs_x) * sub_areas + ai / sub_areas) * areas_x
                    + (si % subs_x) * sub_areas
                    + ai % sub_areas;
                let area = areas.get_mut(dst).ok_or("mask area outside world")?;
                for li in 0..n {
                    let r = p + 4 + li * 12;
                    let idx = u32at(&data, r)? as usize;
                    if idx >= layer_count {
                        return Err("layer index out of bounds".into());
                    }
                    let lp = layerstart + idx * 28;
                    let texture = u32at(&data, lp)? as usize;
                    if texture >= textures.len() {
                        return Err("texture index out of bounds".into());
                    }
                    let mut size = if li == 0 {
                        0
                    } else {
                        u32at(&data, lp + 8)? as usize
                    };
                    if size > 8192 || (li > 0 && size == 0) {
                        return Err("invalid mask size".into());
                    }
                    let mut mask = Vec::new();
                    if li > 0 {
                        let off = u32at(&data, r + 4)? as usize;
                        let len = u32at(&data, r + 8)? as usize;
                        let bytes = data
                            .get(off..off.checked_add(len).ok_or("mask overflow")?)
                            .ok_or("truncated mask")?;
                        if u32at(&data, 16)? != 0 {
                            ZlibDecoder::new(bytes)
                                .take((size * size + 1) as u64)
                                .read_to_end(&mut mask)
                                .map_err(|e| e.to_string())?;
                        } else {
                            mask.extend_from_slice(bytes)
                        }
                        if mask.len() != size * size {
                            return Err("invalid decoded mask size".into());
                        }
                        // Only downsample in the explicitly selected low-memory mode.
                        if size == 256 && std::env::var_os("PW_SMALL_MASK_ATLAS").is_some() {
                            let mut half = vec![0u8; 128 * 128];
                            for y in 0..128 {
                                for x in 0..128 {
                                    let i = 2 * y * 256 + 2 * x;
                                    half[y * 128 + x] = ((mask[i] as u32
                                        + mask[i + 1] as u32
                                        + mask[i + 256] as u32
                                        + mask[i + 257] as u32
                                        + 2)
                                        / 4)
                                        as u8;
                                }
                            }
                            mask = half;
                            size = 128;
                        }
                    }
                    area.layers.push(Layer {
                        texture,
                        axis: *data.get(lp + 12).ok_or("truncated layer")?,
                        u: f32at(&data, lp + 16)?,
                        v: f32at(&data, lp + 20)?,
                        mask_size: size,
                        mask,
                    });
                }
            }
        }
        Ok(Self {
            textures,
            areas,
            areas_x,
            mask_grid,
            width,
            height,
            spacing,
        })
    }
    /// The desktop GPU owns uploaded copies and no longer CPU-bakes terrain.
    pub fn release_gpu_source(&mut self) {
        self.areas.clear();
        self.textures.clear();
        self.areas.shrink_to_fit();
        self.textures.shrink_to_fit();
        #[cfg(target_os = "linux")]
        unsafe {
            unsafe extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            malloc_trim(0);
        }
    }
    pub fn sample(&self, x: f32, y: f32, h: f32) -> [f32; 3] {
        let gx = x.clamp(0., self.width as f32 - 1.);
        let gy = y.clamp(0., self.height as f32 - 1.);
        let ax = gx as usize / self.mask_grid;
        let ay = gy as usize / self.mask_grid;
        let area = &self.areas[ay * self.areas_x + ax];
        let mut color = [0.; 3];
        for (i, l) in area.layers.iter().enumerate() {
            let tex = &self.textures[l.texture];
            let world_x = (gx - self.width as f32 * 0.5) * self.spacing;
            let world_z = (self.height as f32 * 0.5 - gy) * self.spacing;
            let (u, v) = match l.axis {
                0 => (world_z, -h),
                2 => (world_x, -h),
                _ => (world_x, -world_z),
            };
            let ux = (u * l.u).rem_euclid(1.) * tex.w as f32 - 0.5;
            let vy = (v * l.v).rem_euclid(1.) * tex.h as f32 - 0.5;
            let x0 = ux.floor() as isize;
            let y0 = vy.floor() as isize;
            let fx = ux - ux.floor();
            let fy = vy - vy.floor();
            let texel = |x: isize, y: isize, k: usize| -> f32 {
                let xx = x.rem_euclid(tex.w as isize) as usize;
                let yy = y.rem_euclid(tex.h as isize) as usize;
                tex.pixels[(yy * tex.w + xx) * 3 + k] as f32
            };
            let alpha = if i == 0 {
                1.
            } else {
                let mx = ((gx % self.mask_grid as f32) / self.mask_grid as f32)
                    * l.mask_size as f32
                    - 0.5;
                let my = ((gy % self.mask_grid as f32) / self.mask_grid as f32)
                    * l.mask_size as f32
                    - 0.5;
                let ix = mx.floor() as isize;
                let iy = my.floor() as isize;
                let dx = mx - mx.floor();
                let dy = my - my.floor();
                let mask = |x: isize, y: isize| -> f32 {
                    let x = x.clamp(0, l.mask_size as isize - 1) as usize;
                    let y = y.clamp(0, l.mask_size as isize - 1) as usize;
                    l.mask[y * l.mask_size + x] as f32 / 255.
                };
                let top = mask(ix, iy) * (1. - dx) + mask(ix + 1, iy) * dx;
                let bottom = mask(ix, iy + 1) * (1. - dx) + mask(ix + 1, iy + 1) * dx;
                top * (1. - dy) + bottom * dy
            };
            for k in 0..3 {
                let top = texel(x0, y0, k) * (1. - fx) + texel(x0 + 1, y0, k) * fx;
                let bottom = texel(x0, y0 + 1, k) * (1. - fx) + texel(x0 + 1, y0 + 1, k) * fx;
                let value = top * (1. - fy) + bottom * fy;
                color[k] = color[k] * (1. - alpha) + value * alpha;
            }
        }
        color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blends_real_mask_weight() {
        let m = Materials {
            textures: vec![
                Texture {
                    w: 1,
                    h: 1,
                    pixels: vec![200, 0, 0],
                },
                Texture {
                    w: 1,
                    h: 1,
                    pixels: vec![0, 0, 200],
                },
            ],
            areas: vec![Area {
                layers: vec![
                    Layer {
                        texture: 0,
                        axis: 1,
                        u: 1.,
                        v: 1.,
                        mask_size: 0,
                        mask: vec![],
                    },
                    Layer {
                        texture: 1,
                        axis: 1,
                        u: 1.,
                        v: 1.,
                        mask_size: 1,
                        mask: vec![128],
                    },
                ],
            }],
            areas_x: 1,
            mask_grid: 4,
            width: 4,
            height: 4,
            spacing: 2.,
        };
        let v = m.sample(1., 1., 0.);
        assert!((v[0] - 99.6).abs() < 1.0 && v[1] == 0. && (v[2] - 100.4).abs() < 1.0);
    }
}
