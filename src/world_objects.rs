//! Incremental whole-world BMD preload. Each chunk has its own texture atlas;
//! geometry is retained on the GPU rather than rebuilt with nearby terrain.
use crate::{gpu::Vertex, objects::Objects, scene::Ornament};
use glam::Vec3;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc,
    },
};
const SIDE: usize = 4096;
const MAX_VERTICES: usize = 1_000_000;
pub struct Page {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub atlas: Vec<u8>,
    pub processed: usize,
    pub used_height: usize,
    pub mips: Vec<Vec<u8>>,
}
impl Page {
    pub fn finish(mut self) -> Self {
        // Save unused rows in the last page. Keep all seven mip reductions valid.
        let height = self.used_height.max(crate::texture_atlas::UNIT).min(SIDE);
        let factor = SIDE as f32 / height as f32;
        for v in &mut self.vertices {
            v.uv[1] *= factor;
            v.color[1] *= factor;
            v.color[3] *= factor;
        }
        self.atlas.truncate(SIDE * height * 4);
        self.atlas.shrink_to_fit();
        self.used_height = height;
        // Mip generation is done by the background scene worker, not the UI thread.
        let (mut w, mut h) = (SIDE, height);
        for level in 1..8 {
            let source = if level == 1 {
                &self.atlas
            } else {
                &self.mips[level - 2]
            };
            let (nw, nh) = (w / 2, h / 2);
            let mut out = vec![0u8; nw * nh * 4];
            for y in 0..nh {
                for x in 0..nw {
                    let a = (2 * y * w + 2 * x) * 4;
                    let b = a + w * 4;
                    let dst = (y * nw + x) * 4;
                    for c in 0..4 {
                        out[dst + c] = ((source[a + c] as u32
                            + source[a + 4 + c] as u32
                            + source[b + c] as u32
                            + source[b + 4 + c] as u32
                            + 2)
                            / 4) as u8;
                    }
                }
            }
            self.mips.push(out);
            w = nw;
            h = nh;
        }
        self
    }
}
pub enum Message {
    Page(Page),
    Done { processed: usize, skipped: usize },
    Error(String),
}
fn basis(a: [f32; 3]) -> Vec3 {
    let v = Vec3::from_array(a);
    if v.length_squared() < 1e-8 {
        Vec3::Y
    } else {
        v.normalize()
    }
}
struct Builder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    atlas: Vec<u8>,
    slots: HashMap<String, crate::texture_atlas::Slot>,
    packing: crate::texture_atlas::Atlas,
    used_height: usize,
}
impl Builder {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            atlas: vec![0; SIDE * SIDE * 4],
            slots: HashMap::new(),
            packing: crate::texture_atlas::Atlas::new(),
            used_height: 0,
        }
    }
    fn page(self, processed: usize) -> Page {
        Page {
            vertices: self.vertices,
            indices: self.indices,
            atlas: self.atlas,
            processed,
            used_height: self.used_height,
            mips: Vec::new(),
        }
        .finish()
    }
}
pub fn build(
    element: PathBuf,
    ornaments: Vec<Ornament>,
    tx: SyncSender<Message>,
    cancel: Arc<AtomicBool>,
) {
    let result = run(element, ornaments, &tx, &cancel);
    if let Err(e) = result {
        let _ = tx.send(Message::Error(e));
    }
}
fn run(
    element: PathBuf,
    ornaments: Vec<Ornament>,
    tx: &SyncSender<Message>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut ornaments = ornaments;
    // Group related models to reuse textures within a page. The supplied client
    // fell from 101 to 66 native-resolution pages in the full-world survey.
    if std::env::var_os("PW_WORLD_KEEP_ORDER_TEST").is_none() {
        ornaments.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let mut objects = Objects::open(&element, ornaments)?;
    let first = std::env::var("PW_WORLD_TEST_START")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(objects.ornaments.len());
    let count = std::env::var("PW_WORLD_TEST_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(objects.ornaments.len());
    let limit = first.saturating_add(count).min(objects.ornaments.len());
    let mut page = Builder::new();
    let mut skipped = 0usize;
    let fallback = crate::objects::Texture {
        width: 1,
        height: 1,
        rgba: vec![160, 160, 160, 255],
    };
    for start in (first..limit).step_by(32) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let end = (start + 32).min(limit);
        let ids: Vec<_> = (start..end).collect();
        objects.load_ids(&ids)?;
        for id in ids {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let Some(model) = objects.get(id) else {
                continue;
            };
            let z = basis(model.direction);
            let y = basis(model.up);
            let x = y.cross(z).normalize_or_zero();
            for mesh in &model.meshes {
                let mut key = mesh.texture.to_lowercase();
                let tex = if let Some(t) = objects.texture(&key) {
                    t
                } else {
                    skipped += 1;
                    key = String::from("<missing-client-texture>");
                    &fallback
                };
                if page.vertices.len() + mesh.vertices.len() > MAX_VERTICES
                    && !page.indices.is_empty()
                {
                    let finished = std::mem::replace(&mut page, Builder::new());
                    if tx.send(Message::Page(finished.page(id))).is_err() {
                        return Ok(());
                    }
                }
                let slot = if let Some(&s) = page.slots.get(&key) {
                    s
                } else {
                    let mut slot = page.packing.allocate(tex.width, tex.height, 0, false);
                    if slot.is_none() && !page.indices.is_empty() {
                        let finished = std::mem::replace(&mut page, Builder::new());
                        if tx.send(Message::Page(finished.page(id))).is_err() {
                            return Ok(());
                        }
                        slot = page.packing.allocate(tex.width, tex.height, 0, false);
                    }
                    let s = slot.ok_or("scenery texture exceeds atlas capacity")?;
                    for py in 0..s.h {
                        for px in 0..s.w {
                            let sample = tex.sample_rgba(
                                (px as f32 + 0.5) / s.w as f32,
                                (py as f32 + 0.5) / s.h as f32,
                            );
                            let dst = ((s.y + py) * SIDE + s.x + px) * 4;
                            page.atlas[dst..dst + 4].copy_from_slice(&sample);
                        }
                    }
                    page.used_height = page.used_height.max(s.y + s.h);
                    page.slots.insert(key, s);
                    s
                };
                let base = page.vertices.len() as u32;
                let ox = slot.x as f32 / SIDE as f32;
                let oy = slot.y as f32 / SIDE as f32;
                let sx = slot.w as f32 / SIDE as f32;
                let sy = slot.h as f32 / SIDE as f32;
                for v in &mesh.vertices {
                    let pos = Vec3::from_array(model.position)
                        + x * (v.position[0] * model.scale[0])
                        + y * (v.position[1] * model.scale[1])
                        + z * (v.position[2] * model.scale[2]);
                    let normal =
                        (x * v.normal[0] + y * v.normal[1] + z * v.normal[2]).normalize_or_zero();
                    page.vertices.push(Vertex {
                        pos: pos.to_array(),
                        normal: normal.to_array(),
                        uv: [ox + v.uv[0] * sx, oy + v.uv[1] * sy],
                        color: [ox, oy, sx, sy],
                        kind: if mesh.material.as_ref().is_some_and(|m| m.two_sided) {
                            2.1
                        } else {
                            2.0
                        },
                        diffuse: v.day_diffuse,
                        night_diffuse: v.night_diffuse,
                    });
                }
                page.indices
                    .extend(mesh.indices.iter().map(|&n| base + n as u32));
            }
        }
    }
    if !page.indices.is_empty() && tx.send(Message::Page(page.page(limit))).is_err() {
        return Ok(());
    }
    let _ = tx.send(Message::Done {
        processed: limit,
        skipped,
    });
    Ok(())
}
pub struct GpuPage {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    normal: wgpu::BindGroup,
    reflected: wgpu::BindGroup,
}
impl GpuPage {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        normal: &wgpu::Buffer,
        reflected: &wgpu::Buffer,
        page: Page,
    ) -> Self {
        use wgpu::util::DeviceExt;
        let image = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whole-world object atlas"),
            size: wgpu::Extent3d {
                width: SIDE as u32,
                height: page.used_height as u32,
                depth_or_array_layers: 1,
            },
            // Tiles start on 128-pixel boundaries; seven reductions preserve
            // those boundaries and avoid cross-material mip contamination.
            mip_level_count: 8,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Match the D3D8 encoded-color sampling used by the local atlas.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut width = SIDE;
        let mut height = page.used_height;
        for mip in 0..8u32 {
            let level = if mip == 0 {
                &page.atlas
            } else {
                &page.mips[(mip - 1) as usize]
            };
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &image,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                level,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some((width * 4) as u32),
                    rows_per_image: Some(height as u32),
                },
                wgpu::Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
            );
            width /= 2;
            height /= 2;
        }
        let view = image.create_view(&Default::default());
        let make = |buf: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("whole-world object material"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let normal = make(normal);
        let reflected = make(reflected);
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole-world object vertices"),
            contents: bytemuck::cast_slice(&page.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole-world object indices"),
            contents: bytemuck::cast_slice(&page.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            vertex,
            index,
            count: page.indices.len() as u32,
            normal,
            reflected,
        }
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, reflected: bool) {
        pass.set_bind_group(
            0,
            if reflected {
                &self.reflected
            } else {
                &self.normal
            },
            &[],
        );
        pass.set_vertex_buffer(0, self.vertex.slice(..));
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    #[test]
    fn trim_scales_y_and_builds_independent_mips() {
        let vertex = Vertex {
            pos: [0.; 3],
            normal: [0.; 3],
            uv: [0.25, 0.03125],
            color: [0., 0.03125, 0.125, 0.03125],
            kind: 2.,
            diffuse: 0xffffffff,
            night_diffuse: 0xffff_ffff,
        };
        let page = Page {
            vertices: vec![vertex],
            indices: vec![0],
            atlas: vec![255; SIDE * 256 * 4],
            processed: 0,
            used_height: 256,
            mips: Vec::new(),
        }
        .finish();
        assert_eq!(page.mips.len(), 7);
        assert_eq!(page.atlas.len(), SIDE * 256 * 4);
        assert_eq!(page.vertices[0].uv[1], 0.5);
        assert_eq!(page.vertices[0].color[3], 0.5);
        assert_eq!(page.mips[6].len(), (SIDE / 128) * 2 * 4);
    }
}
