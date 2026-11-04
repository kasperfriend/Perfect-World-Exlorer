//! Incremental whole-world BMD preload. Native-size texture atlases are shared
//! globally across geometry chunks, rather than duplicated at every page turn.
use crate::{
    gpu::Vertex,
    objects::Objects,
    scene::Ornament,
    scenery_visibility::{allows_with_context, Pass, RenderContext, Visibility},
};
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

/// One ornament's index range inside a page. The two EC-BSD flags decide
/// whether this range is submitted in reflection/refraction; the main and map
/// passes always submit it. `start`/`end` are indices into `Page::indices`.
#[derive(Clone, Debug)]
pub struct PlacementRange {
    pub start: u32,
    pub end: u32,
    pub export_id: u32,
    /// Original model/mesh encounter order, independent of storage chunking.
    pub order: u64,
    pub visibility: Visibility,
}

pub struct Page {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub atlas: Vec<u8>,
    /// Earlier emitted page whose immutable atlas this geometry reuses.
    pub shared_atlas: Option<usize>,
    pub processed: usize,
    pub used_height: usize,
    pub mips: Vec<Vec<u8>>,
    pub placements: Vec<PlacementRange>,
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
        if self.shared_atlas.is_some() {
            assert!(self.atlas.is_empty() && self.mips.is_empty());
            return self;
        }
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
/// Measured residency and resolution outcome of one whole-world preload
/// (P3.08: measured numbers, not estimates).
#[derive(Clone, Copy, Debug)]
pub struct DoneStats {
    pub residency: crate::objects::ResidencyStats,
    pub resolver: crate::texture_resolution::ResolverStats,
    /// Register rows recorded for references that resolved nowhere or were
    /// rejected by name (.sdr. shader registry, decode failures, missing).
    pub unresolved_rows: usize,
    /// Pages emitted by this preload.
    pub pages: usize,
    pub atlas_pages: usize,
    pub atlas_bytes: usize,
}
pub enum Message {
    Page(Page),
    Done {
        processed: usize,
        skipped: usize,
        stats: DoneStats,
    },
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
    shared_atlas: Option<usize>,
    slots: HashMap<String, crate::texture_atlas::Slot>,
    packing: crate::texture_atlas::Atlas,
    used_height: usize,
    placements: Vec<PlacementRange>,
}
impl Builder {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            atlas: vec![0; SIDE * SIDE * 4],
            shared_atlas: None,
            slots: HashMap::new(),
            packing: crate::texture_atlas::Atlas::new(),
            used_height: 0,
            placements: Vec::new(),
        }
    }
    fn shared(source: usize, height: usize) -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            atlas: Vec::new(),
            shared_atlas: Some(source),
            slots: HashMap::new(),
            packing: crate::texture_atlas::Atlas::new(),
            used_height: height,
            placements: Vec::new(),
        }
    }
    fn page(self, processed: usize) -> Page {
        Page {
            vertices: self.vertices,
            indices: self.indices,
            atlas: self.atlas,
            shared_atlas: self.shared_atlas,
            processed,
            used_height: self.used_height,
            mips: Vec::new(),
            placements: self.placements,
        }
        .finish()
    }
}
#[derive(Clone, Copy)]
struct SharedSlot {
    source: usize,
    height: usize,
    slot: crate::texture_atlas::Slot,
}
fn remember_atlas(page: &Builder, source: usize, slots: &mut HashMap<String, SharedSlot>) {
    assert!(page.shared_atlas.is_none());
    let height = page.used_height.clamp(crate::texture_atlas::UNIT, SIDE);
    for (key, &slot) in &page.slots {
        // Atlas content, slot coordinates and mip chain are immutable after
        // emission. Later chunks reuse them without resampling/re-encoding.
        slots.insert(
            key.clone(),
            SharedSlot {
                source,
                height,
                slot,
            },
        );
    }
}
fn atlas_mip_bytes(height: usize) -> usize {
    let height = height.clamp(crate::texture_atlas::UNIT, SIDE);
    (0..8)
        .map(|level| (SIDE >> level) * (height >> level) * 4)
        .sum()
}
/// Allocates (or reuses) the atlas slot for one texture inside a page and
/// blits native-resolution pixels. The slot key is the texture's CONTENT
/// key (Objects::texture_key), not the reference name: distinct names with
/// identical pixels share one slot, which measurably reduces slot count and
/// page turns. When the page is full and already holds geometry, a fresh
/// page replaces it in place and the finished page is returned for sending.
fn allocate_slot(
    page: &mut Builder,
    key: &str,
    tex: &crate::objects::Texture,
) -> Result<(crate::texture_atlas::Slot, Option<Builder>), String> {
    if let Some(&s) = page.slots.get(key) {
        return Ok((s, None));
    }
    let mut slot = page.packing.allocate(tex.width, tex.height, 0, false);
    let mut turned = None;
    if slot.is_none() && !page.indices.is_empty() {
        turned = Some(std::mem::replace(page, Builder::new()));
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
    page.slots.insert(key.to_string(), s);
    Ok((s, turned))
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
    let mut shared_slots = HashMap::new();
    let mut shared_geometry = HashMap::<usize, Builder>::new();
    let mut skipped = 0usize;
    let mut pages = 0usize;
    let mut atlas_pages = 0usize;
    let mut atlas_bytes = 0usize;
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
            // EC-BSD record +36 low bytes, recovered at 0x489f0b/0x489f12.
            // Keep the flags with every index range so a hidden placement is
            // never submitted just because its page is resident.
            let visibility = Visibility::from_extra(objects.ornaments[id].extra);
            let z = basis(model.direction);
            let y = basis(model.up);
            let x = y.cross(z).normalize_or_zero();
            for (mesh_index, mesh) in model.meshes.iter().enumerate() {
                // Stage, culling and alpha state come from the single recovered
                // table, not from per-asset guesses.
                let kind = match crate::bmd_baked::classify(
                    mesh.version,
                    !mesh.texture.trim().is_empty(),
                    mesh.material.as_ref().is_some_and(|m| m.two_sided),
                ) {
                    Ok(state) => state.kind,
                    Err(e) => {
                        // No silent skip: an unrepresentable mesh is an explicit
                        // error for the whole-world builder, not a dropped
                        // triangle. The BMD parser already rejects unsupported
                        // versions by name before this point.
                        return Err(format!("unsupported baked mesh {}: {e}", mesh.name));
                    }
                };
                // Content-keyed slot sharing (P3.08 cross-name dedup):
                // references with identical pixels collapse onto one atlas
                // slot regardless of how many names point at them.
                let key = match objects.texture_key(&mesh.texture) {
                    Some(hash) => format!("h{hash:016x}"),
                    None => String::from("<missing-client-texture>"),
                };
                let tex = if let Some(t) = objects.texture(&mesh.texture) {
                    t
                } else {
                    skipped += 1;
                    &fallback
                };
                if page.vertices.len() + mesh.vertices.len() > MAX_VERTICES
                    && !page.indices.is_empty()
                {
                    let finished = std::mem::replace(&mut page, Builder::new());
                    atlas_pages += 1;
                    atlas_bytes += atlas_mip_bytes(finished.used_height);
                    remember_atlas(&finished, pages, &mut shared_slots);
                    pages += 1;
                    if tx.send(Message::Page(finished.page(id))).is_err() {
                        return Ok(());
                    }
                }
                let (page, slot) = if let Some(shared) = shared_slots.get(&key).copied() {
                    let target = shared_geometry
                        .entry(shared.source)
                        .or_insert_with(|| Builder::shared(shared.source, shared.height));
                    if target.vertices.len() + mesh.vertices.len() > MAX_VERTICES
                        && !target.indices.is_empty()
                    {
                        let ready = std::mem::replace(
                            target,
                            Builder::shared(shared.source, shared.height),
                        );
                        pages += 1;
                        if tx.send(Message::Page(ready.page(id))).is_err() {
                            return Ok(());
                        }
                    }
                    (target, shared.slot)
                } else {
                    let (slot, turned) = allocate_slot(&mut page, &key, tex)?;
                    if let Some(finished) = turned {
                        atlas_pages += 1;
                        atlas_bytes += atlas_mip_bytes(finished.used_height);
                        remember_atlas(&finished, pages, &mut shared_slots);
                        pages += 1;
                        if tx.send(Message::Page(finished.page(id))).is_err() {
                            return Ok(());
                        }
                    }
                    (&mut page, slot)
                };
                let base = page.vertices.len() as u32;
                let range_start = page.indices.len() as u32;
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
                        kind,
                        diffuse: v.day_diffuse,
                        night_diffuse: v.night_diffuse,
                    });
                }
                page.indices
                    .extend(mesh.indices.iter().map(|&n| base + n as u32));
                let range_end = page.indices.len() as u32;
                if range_end > range_start {
                    page.placements.push(PlacementRange {
                        start: range_start,
                        end: range_end,
                        export_id: objects.ornaments[id].export_id,
                        order: ((id as u64) << 32) | mesh_index as u64,
                        visibility,
                    });
                }
            }
        }
        // A batch holds at most 32 placements' reused-atlas geometry; unlike
        // keeping every atlas builder alive this never retains world pixels.
        let mut ready: Vec<_> = shared_geometry.drain().collect();
        ready.sort_by_key(|(source, _)| *source);
        for (_, builder) in ready {
            if !builder.indices.is_empty() {
                pages += 1;
                if tx.send(Message::Page(builder.page(end))).is_err() {
                    return Ok(());
                }
            }
        }
    }
    if !page.indices.is_empty() {
        atlas_pages += 1;
        atlas_bytes += atlas_mip_bytes(page.used_height);
        pages += 1;
        if tx.send(Message::Page(page.page(limit))).is_err() {
            return Ok(());
        }
    }
    let _ = tx.send(Message::Done {
        processed: limit,
        skipped,
        stats: DoneStats {
            residency: objects.residency(),
            resolver: objects.resolver_stats(),
            unresolved_rows: objects.unresolved().len(),
            pages,
            atlas_pages,
            atlas_bytes,
        },
    });
    Ok(())
}
pub struct GpuPage {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    normal: Arc<wgpu::BindGroup>,
    reflected: Arc<wgpu::BindGroup>,
    placements: Vec<PlacementRange>,
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
        assert!(
            page.shared_atlas.is_none(),
            "shared atlas requires upload_world"
        );
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
        let normal = Arc::new(make(normal));
        let reflected = Arc::new(make(reflected));
        Self::geometry(device, page, normal, reflected)
    }
    /// Upload only geometry for a reused atlas. Bind groups keep the exact
    /// original texture/view alive; no second allocation or texture upload.
    pub fn upload_world(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        normal: &wgpu::Buffer,
        reflected: &wgpu::Buffer,
        existing: &[Self],
        page: Page,
    ) -> Self {
        if let Some(source) = page.shared_atlas {
            let source = existing
                .get(source)
                .expect("shared atlas must precede its geometry");
            Self::geometry(
                device,
                page,
                Arc::clone(&source.normal),
                Arc::clone(&source.reflected),
            )
        } else {
            Self::upload(device, queue, layout, sampler, normal, reflected, page)
        }
    }
    fn geometry(
        device: &wgpu::Device,
        page: Page,
        normal: Arc<wgpu::BindGroup>,
        reflected: Arc<wgpu::BindGroup>,
    ) -> Self {
        use wgpu::util::DeviceExt;
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
            placements: page.placements,
        }
    }
    /// Submits only the placements the recovered EC-BSD flags admit in this
    /// view. A page built without per-placement ranges is a diagnostic page:
    /// draw it in full, but say so instead of silently dropping it.
    pub fn draw_selected<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        reflected: bool,
        view: Pass,
    ) {
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
        if self.placements.is_empty() {
            eprintln!("scenery page has no placement visibility ranges; drawing all indices");
            pass.draw_indexed(0..self.count, 0, 0..1);
            return;
        }
        for r in &self.placements {
            if allows_with_context(view, r.visibility, RenderContext::default()) {
                pass.draw_indexed(r.start..r.end, 0, 0..1);
            }
        }
    }
    /// Freeze the original submission sequence once loading is complete.
    /// Texture sharing changes storage, never coplanar/depth-tie winners.
    pub fn draw_order(pages: &[Self]) -> Vec<(usize, usize)> {
        let mut order = Vec::new();
        for (page, p) in pages.iter().enumerate() {
            if p.placements.is_empty() {
                order.push((u64::MAX, page, usize::MAX));
            } else {
                for (range, r) in p.placements.iter().enumerate() {
                    order.push((r.order, page, range));
                }
            }
        }
        order.sort_unstable();
        order
            .into_iter()
            .map(|(_, page, range)| (page, range))
            .collect()
    }
    pub fn draw_ordered<'a>(
        pages: &'a [Self],
        order: &[(usize, usize)],
        pass: &mut wgpu::RenderPass<'a>,
        reflected: bool,
        view: Pass,
    ) {
        let mut bound = None;
        for &(page, range) in order {
            let p = &pages[page];
            let indices = if range == usize::MAX {
                0..p.count
            } else {
                let r = &p.placements[range];
                if !allows_with_context(view, r.visibility, RenderContext::default()) {
                    continue;
                }
                r.start..r.end
            };
            if bound != Some(page) {
                pass.set_bind_group(0, if reflected { &p.reflected } else { &p.normal }, &[]);
                pass.set_vertex_buffer(0, p.vertex.slice(..));
                pass.set_index_buffer(p.index.slice(..), wgpu::IndexFormat::Uint32);
                bound = Some(page);
            }
            pass.draw_indexed(indices, 0, 0..1);
        }
    }
    pub fn shares_atlas_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.normal, &other.normal) && Arc::ptr_eq(&self.reflected, &other.reflected)
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, reflected: bool) {
        self.draw_selected(pass, reflected, Pass::Main);
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    fn solid(fill: u8, side: usize) -> crate::objects::Texture {
        crate::objects::Texture {
            width: side,
            height: side,
            rgba: vec![fill; side * side * 4],
        }
    }
    #[test]
    fn emitted_atlas_reuses_slots_without_pixels_or_mip_allocations() {
        let mut original = Builder::new();
        let texture = solid(71, 128);
        let (slot, _) = allocate_slot(&mut original, "shared", &texture).unwrap();
        let mut slots = HashMap::new();
        remember_atlas(&original, 3, &mut slots);
        let reference = slots["shared"];
        assert_eq!(reference.source, 3);
        assert_eq!(reference.height, 128);
        assert_eq!(reference.slot.x, slot.x);
        let vertex = Vertex {
            pos: [0.; 3],
            normal: [0.; 3],
            uv: [0.01, 0.015625],
            color: [0., 0., 0.03125, 0.03125],
            kind: 2.,
            diffuse: 0,
            night_diffuse: 0,
        };
        let mut geometry = Builder::shared(reference.source, reference.height);
        geometry.vertices.push(vertex);
        geometry.indices.push(0);
        let page = geometry.page(32);
        assert_eq!(page.shared_atlas, Some(3));
        assert!(page.atlas.is_empty() && page.mips.is_empty());
        assert_eq!(page.vertices[0].uv[1], 0.5);
        assert_eq!(page.vertices[0].color[3], 1.);
        // Geometry chunks no longer multiply the source texture's pixel cost.
        assert_eq!(
            atlas_mip_bytes(128),
            (0..8).map(|l| (4096 >> l) * (128 >> l) * 4).sum::<usize>()
        );
    }
    #[test]
    fn content_dedup_reduces_page_turns() {
        // 512x512 textures occupy 16 atlas cells each; a 4096^2 page holds
        // 1024 cells = 64 of them. Push 128 references through both keying
        // schemes and count page turns.
        let textures: Vec<_> = (0..64).map(|i| solid(i as u8 + 1, 512)).collect();
        // Content-keyed: the 128 references are 64 contents twice, so every
        // second reference reuses a slot and ONE page suffices.
        let mut page = Builder::new();
        page.indices.push(0);
        let mut dedup_turns = 0;
        for i in 0..128 {
            let tex = &textures[i % 64];
            let (_slot, turned) =
                allocate_slot(&mut page, &format!("h{:016x}", i % 64), tex).unwrap();
            if turned.is_some() {
                dedup_turns += 1;
            }
        }
        assert_eq!(dedup_turns, 0);
        assert_eq!(page.slots.len(), 64);
        // Name-keyed with 65 distinct contents: the 65th overflows the page
        // and forces a turn — exactly what content dedup avoids when the
        // extra references are aliases.
        let mut page = Builder::new();
        page.indices.push(0);
        let mut unique_turns = 0;
        let extra = solid(200, 512);
        for i in 0..65 {
            let tex = if i < 64 { &textures[i] } else { &extra };
            let (_slot, turned) = allocate_slot(&mut page, &format!("name{i}"), tex).unwrap();
            if turned.is_some() {
                unique_turns += 1;
            }
        }
        assert_eq!(unique_turns, 1);
        assert!(dedup_turns < unique_turns);
    }
    #[test]
    fn missing_references_share_one_fallback_slot() {
        let mut page = Builder::new();
        page.indices.push(0);
        let fallback = solid(160, 1);
        for name in ["<missing-client-texture>"; 3] {
            let (_slot, turned) = allocate_slot(&mut page, name, &fallback).unwrap();
            assert!(turned.is_none());
        }
        assert_eq!(page.slots.len(), 1);
    }
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
            shared_atlas: None,
            vertices: vec![vertex],
            indices: vec![0],
            atlas: vec![255; SIDE * 256 * 4],
            processed: 0,
            used_height: 256,
            mips: Vec::new(),
            placements: Vec::new(),
        }
        .finish();
        assert_eq!(page.mips.len(), 7);
        assert_eq!(page.atlas.len(), SIDE * 256 * 4);
        assert_eq!(page.vertices[0].uv[1], 0.5);
        assert_eq!(page.vertices[0].color[3], 0.5);
        assert_eq!(page.mips[6].len(), (SIDE / 128) * 2 * 4);
    }
}
