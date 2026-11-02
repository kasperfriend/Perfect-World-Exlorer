//! Experimental GPU world renderer: actual client terrain, static BMDs and water masks.
//! Baked terrain material atlas, fixed lighting and flat water are not client parity.
use crate::Terrain;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};
use wgpu::util::DeviceExt;
use winit::{
    event::{DeviceEvent, ElementState, Event, MouseButton, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, WindowBuilder},
};
const TILE: usize = 32;
use crate::view_settings::{DetailSettings, ViewSettings};

const TEX: usize = 128;
const ATLAS: usize = 4096;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Vertex {
    pub(crate) pos: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) color: [f32; 4],
    pub(crate) kind: f32,
    pub(crate) diffuse: u32,
    pub(crate) night_diffuse: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Uniform {
    matrix: [[f32; 4]; 4],
    direction: [f32; 4],
    clip: [f32; 4],
    bounds: [f32; 4],
    tree_bounds: [f32; 4],
    sun: [f32; 4],
    light: [f32; 4],
    region_ambient: [f32; 4],
    region_diffuse: [f32; 4],
    region_direction: [f32; 4],
    grass_eye: [f32; 4],
    grass_wind: [f32; 4],
    grass_history: [[f32; 4]; 80],
    fog_range: [f32; 4], // start, end, enabled, reserved
    fog_color: [f32; 4], // encoded client ARGB
}
fn minimap_matrix(x: f32, z: f32, range: f32) -> Mat4 {
    let eye = Vec3::new(x, 10000., z);
    // Game world +Z is north. Flip horizontal clip space because the
    // right-handed top-down view otherwise puts west on the right.
    Mat4::orthographic_rh(range, -range, -range, range, 20000., 1.)
        * Mat4::look_to_rh(eye, Vec3::NEG_Y, Vec3::Z)
}
/// Keep the world-map point under the cursor fixed when its image is scaled.
fn anchored_zoom_offset(offset: egui::Vec2, cursor_in_image: egui::Vec2, ratio: f32) -> egui::Vec2 {
    (offset + cursor_in_image * (ratio - 1.)).max(egui::Vec2::ZERO)
}
/// Show the saved/default value as both a gold rail tick and a numeric label.
fn marked_slider(
    ui: &mut egui::Ui,
    slider: egui::Slider<'_>,
    label: &str,
    default: f32,
    min: f32,
    max: f32,
    logarithmic: bool,
    enabled: bool,
) -> egui::Response {
    let label = format!("{label}  (default: {default})");
    let r = ui.add_enabled(enabled, slider.text(label));
    let t = if logarithmic {
        (default / min).ln() / (max / min).ln()
    } else {
        (default - min) / (max - min)
    }
    .clamp(0., 1.);
    let thickness = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let radius = thickness / 2.5;
    let width = ui.spacing().slider_width;
    let x = r.rect.left() + radius + (width - 2. * radius) * t;
    let y = r.rect.center().y + thickness * 0.5 - 1.;
    ui.painter().line_segment(
        [egui::pos2(x, y - 4.), egui::pos2(x, y + 2.)],
        egui::Stroke::new(2.0f32, egui::Color32::from_rgb(255, 204, 96)),
    );
    r
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuPage {
    Closed,
    Main,
    Settings,
    Teleports,
    WorldMap,
}
impl MenuPage {
    fn escape(self) -> Self {
        match self {
            Self::Closed => Self::Main,
            Self::Main => Self::Closed,
            Self::Settings | Self::Teleports => Self::Main,
            Self::WorldMap => Self::Closed,
        }
    }
    fn is_open(self) -> bool {
        self != Self::Closed
    }
}
/// The terrain the recovered rules select: the camera block window (`0x77ecf0`)
/// and the level of every block inside it (`0x77e360`). Comparing it with the
/// mesh being drawn is what decides whether that mesh is stale.
#[derive(Clone, PartialEq)]
struct LodState {
    window: crate::terrain_lod::Window,
    levels: Vec<u8>,
}
struct MeshData {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    atlas: Vec<u8>,
    grass_mips: Vec<crate::grass_mips::Tile>,
    grass: crate::grass_instances::Data,
    center: (isize, isize),
    tree_center: (f32, f32),
    detail: DetailSettings,
    lod: LodState,
    /// Resident terrain blocks and the index range each occupies.
    spans: Vec<crate::terrain_residency::Span>,
    terrain_end: u32,
    object_start: u32,
    water_start: u32,
    water_batches: Vec<WaterBatch>,
    local_pages: Vec<crate::world_objects::Page>,
}
#[derive(Clone, Debug)]
struct WaterBatch {
    // Do not merge different authored area classes.
    simple: bool,
    start: u32,
    end: u32,
    height: f32,
    bounds: [f32; 4], // min x, max x, min z, max z
}
impl WaterBatch {
    // Conservative homogeneous clip-space rejection for the rectangular bounds.
    fn visible(&self, matrix: Mat4) -> bool {
        let [x0, x1, z0, z1] = self.bounds;
        let corners = [
            Vec3::new(x0, self.height, z0),
            Vec3::new(x0, self.height, z1),
            Vec3::new(x1, self.height, z0),
            Vec3::new(x1, self.height, z1),
        ]
        .map(|p| matrix * p.extend(1.));
        !(corners.iter().all(|p| p.x < -p.w)
            || corners.iter().all(|p| p.x > p.w)
            || corners.iter().all(|p| p.y < -p.w)
            || corners.iter().all(|p| p.y > p.w)
            || corners.iter().all(|p| p.z < 0.)
            || corners.iter().all(|p| p.z > p.w))
    }
    fn distance_squared(&self, p: Vec3) -> f32 {
        let [x0, x1, z0, z1] = self.bounds;
        let x = p.x.clamp(x0, x1);
        let z = p.z.clamp(z0, z1);
        (p - Vec3::new(x, self.height, z)).length_squared()
    }
}
fn basis(v: [f32; 3]) -> Vec3 {
    let v = Vec3::from_array(v);
    if v.length_squared() < 1e-8 {
        Vec3::Y
    } else {
        v.normalize()
    }
}
fn push_quad(v: &mut Vec<Vertex>, i: &mut Vec<u32>, points: [[f32; 3]; 4], color: [f32; 4]) {
    let n = v.len() as u32;
    for p in points {
        v.push(Vertex {
            pos: p,
            normal: [0., 1., 0.],
            uv: [0., 0.],
            color,
            kind: 1.,
            diffuse: 0xffff_ffff,
            night_diffuse: 0xffff_ffff,
        })
    }
    i.extend_from_slice(&[n, n + 1, n + 2, n + 1, n + 3, n + 2]);
}
/// Recovered block window and per-block levels (`src/terrain_lod.rs`). The client
/// re-evaluates `0x77e360` for every resident block on every frame, so the viewer
/// re-runs the same rule per frame and rebuilds geometry only when the answer
/// changes; the window origin moves with the camera's block (`0x77ecf0`).
fn lod_window(
    width: usize,
    height: usize,
    spacing: f32,
    cam: [f32; 2],
    radius: isize,
) -> (crate::terrain_lod::Window, crate::terrain_lod::WindowLod) {
    let cols = width / TILE;
    let rows = height / TILE;
    let block_world = TILE as f32 * spacing;
    let gx = cam[0] / spacing + width as f32 * 0.5;
    let gy = height as f32 * 0.5 - cam[1] / spacing;
    let lod = crate::terrain_lod::LodConfig {
        block_size: block_world,
        ..Default::default()
    };
    // `world+0x1bc`: draw range in world units, one slider step = one block.
    let range = radius.max(1) as f32 * block_world;
    let win = crate::terrain_lod::window(
        [gx / TILE as f32, gy / TILE as f32],
        [cols, rows],
        range,
        block_world,
    );
    // Viewer world axis: x = (cell - width/2) * spacing, z = (height/2 - cell) * spacing.
    let levels = crate::terrain_lod::window_lod(
        win,
        &lod,
        cam,
        |x, y| {
            let cx = (x * TILE) as f32 + TILE as f32 * 0.5;
            let cy = (y * TILE) as f32 + TILE as f32 * 0.5;
            [
                (cx - width as f32 * 0.5) * spacing,
                (height as f32 * 0.5 - cy) * spacing,
            ]
        },
        |x, y| x < cols && y < rows,
    );
    (win, levels)
}
fn build(
    t: &mut Terrain,
    cx: f32,
    cz: f32,
    detail: DetailSettings,
    residency: &mut crate::terrain_residency::Residency,
) -> Result<MeshData, String> {
    let rad = detail.radius;
    let gx = cx / t.grid_spacing + t.width as f32 * 0.5;
    let gy = t.height as f32 * 0.5 - cz / t.grid_spacing;
    // Camera block, used as the rebuild anchor compared against `self.center`.
    let bx = (gx / TILE as f32).floor() as isize;
    let by = (gy / TILE as f32).floor() as isize;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut atlas = vec![0u8; ATLAS * ATLAS * 4];
    // Recovered block residency, resolution levels and block-edge rule
    // (src/terrain_lod.rs). The client keeps a square window of blocks around
    // the camera's block (`0x77ecf0`), gives each one of three meshes by camera
    // distance (`0x77e360`) and closes block boundaries with the coarser
    // neighbour's edge stride (`0x7c40e6`). This is the near mesh; the persistent
    // whole-world mesh below the window keeps the rest of the world visible.
    if t.grid != TILE || TILE != crate::terrain_lod::CELLS {
        return Err(format!(
            "T2BK blocks are {} cells but the recovered block/LOD rule assumes {} \
             (elementclient 0x7e5f70); refusing to draw substitute geometry",
            t.grid,
            crate::terrain_lod::CELLS
        ));
    }
    if t.width % TILE != 0 || t.height % TILE != 0 {
        return Err(format!(
            "T2 terrain grid {}x{} is not a whole number of {TILE}-cell blocks",
            t.width, t.height
        ));
    }
    let (win, levels) = lod_window(t.width, t.height, t.grid_spacing, [cx, cz], rad);
    let lod_state = LodState {
        window: win,
        levels: levels.levels.clone(),
    };
    // The loader stores `grid` cells per block and drops each block's last row
    // and column (the neighbour re-supplies them), so a block's offset-`TILE`
    // vertex clamps to the last stored cell at the world border.
    let sample = |cell_x: usize, cell_y: usize| {
        let x = cell_x.min(t.width - 1);
        let y = cell_y.min(t.height - 1);
        (x, y, y * t.width + x)
    };
    // `0x77e250`: the resident set follows the window. Blocks that enter are
    // meshed at the level `0x77e360` picked, blocks whose level changed are
    // re-meshed, and blocks the window leaves release their geometry.
    let census =
        residency.update(
            &win,
            |x, y| levels.level(x, y),
            |x, y, level| {
                let cases = levels.cases[(y - win.y0) * (win.x1 - win.x0) + (x - win.x0)];
                let surface = crate::terrain_lod::block_surface(level, cases);
                let mut block_vertices = Vec::with_capacity(surface.grid.len());
                let mut low = [f32::INFINITY; 3];
                let mut high = [f32::NEG_INFINITY; 3];
                for g in &surface.grid {
                    let (cell_x, cell_y, at) =
                        sample(x * TILE + g[0] as usize, y * TILE + g[1] as usize);
                    let n = t.normals.get(at).copied().ok_or_else(|| {
                        format!("cell {at} outside {}x{} terrain", t.width, t.height)
                    })?;
                    let height = *t.heights.get(at).ok_or_else(|| {
                        format!("cell {at} outside {}x{} terrain", t.width, t.height)
                    })?;
                    let pos = [
                        (cell_x as f32 - t.width as f32 * 0.5) * t.grid_spacing,
                        height,
                        (t.height as f32 * 0.5 - cell_y as f32) * t.grid_spacing,
                    ];
                    for axis in 0..3 {
                        low[axis] = low[axis].min(pos[axis]);
                        high[axis] = high[axis].max(pos[axis]);
                    }
                    block_vertices.push(Vertex {
                        pos,
                        normal: [n[0] as f32 / 127., n[1] as f32 / 127., n[2] as f32 / 127.],
                        uv: [0.; 2],
                        color: [1.; 4],
                        kind: 0.,
                        diffuse: 0xffff_ffff,
                        night_diffuse: 0xffff_ffff,
                    });
                }
                Ok((
                    block_vertices,
                    surface.indices,
                    [low[0], low[1], low[2], high[0], high[1], high[2]],
                ))
            },
        )?;
    let spans = residency.append(&win, &mut vertices, &mut indices);
    eprintln!(
        "Terrain residency: {} blocks resident ({} entered, {} evicted, {} re-meshed), \
         {} triangles; {} blocks submitted",
        census.resident,
        census.loaded,
        census.evicted,
        census.remeshed,
        census.triangles,
        spans.len()
    );
    let terrain_end = indices.len() as u32;
    // Trees are now entirely GPU-instanced at three runtime-generated LODs.
    let mut atlas_allocator = crate::texture_atlas::Atlas::new();
    // Client MOXB mesh and bitmask-backed placements. Transforms and streaming
    // radius are provisional until the game's randomization and fade are ported.
    let mut grass_builder = crate::grass_instances::Builder::default();
    let mut grass_sample = None;
    let mut grass_slots: HashMap<(String, u8), crate::texture_atlas::Slot> = HashMap::new();
    let mut grass_mips = Vec::new();
    // Reserve at least one 128px slot for every distinct nearby material, so
    // a few large textures cannot starve all later grass types.
    let mut grass_pending = HashSet::new();
    for area in &t.grass.areas {
        const RANGE: f32 = 200.0;
        if (area.center[0] - cx).abs() > area.width as f32 * area.grid * 0.5 + RANGE
            || (area.center[2] - cz).abs() > area.height as f32 * area.grid * 0.5 + RANGE
        {
            continue;
        }
        if let Some(Some(model)) = t.grass.models.get(area.type_id) {
            // CELGrassType initialization overrides the serialized value.
            let alpha = 84u8;
            for mesh in &model.meshes {
                if t.grass.textures.contains_key(&mesh.texture) {
                    grass_pending.insert((mesh.texture.clone(), alpha));
                }
            }
        }
    }
    for area in &t.grass.areas {
        let half_x = area.width as f32 * area.grid * 0.5;
        let half_z = area.height as f32 * area.grid * 0.5;
        const GRASS_RANGE: f32 = 200.0;
        if (area.center[0] - cx).abs() > half_x + GRASS_RANGE
            || (area.center[2] - cz).abs() > half_z + GRASS_RANGE
        {
            continue;
        }
        let Some(Some(model)) = t.grass.models.get(area.type_id) else {
            continue;
        };
        let typ = &t.grass.types[area.type_id];
        for (mesh_index, mesh) in model.meshes.iter().enumerate() {
            let Some(img) = t.grass.textures.get(&mesh.texture) else {
                continue;
            };
            let alpha_ref = 84u8;
            let material_key = (mesh.texture.clone(), alpha_ref);
            let slot = if let Some(&slot) = grass_slots.get(&material_key) {
                slot
            } else {
                // Keep the largest source dimension; the square slot keeps the
                // grass shader's existing per-material alpha threshold intact.
                let n = (img.width().max(img.height()) as usize)
                    .next_power_of_two()
                    .clamp(TEX, ATLAS);
                grass_pending.remove(&material_key);
                let reserve = grass_pending.len().min((ATLAS / TEX).pow(2) - 1);
                let Some(slot) = atlas_allocator.allocate(n, n, reserve, true) else {
                    eprintln!("Grass atlas full: {}", mesh.texture);
                    continue;
                };
                let ox = slot.x;
                let oy = slot.y;
                for py in 0..slot.h {
                    for px in 0..slot.w {
                        let x = (px as u32 * img.width() / slot.w as u32).min(img.width() - 1);
                        let y = (py as u32 * img.height() / slot.h as u32).min(img.height() - 1);
                        let o = ((oy + py) * ATLAS + ox + px) * 4;
                        atlas[o..o + 4].copy_from_slice(&img.get_pixel(x, y).0);
                    }
                }
                grass_slots.insert(material_key, slot);
                grass_mips.push((slot, alpha_ref));
                slot
            };
            let ox = slot.x as f32 / ATLAS as f32;
            let oy = slot.y as f32 / ATLAS as f32;
            let scale = slot.w as f32 / ATLAS as f32;
            let alpha = 84. / 255.;
            let key = (area.type_id, mesh_index);
            if !grass_builder.contains(key) {
                let (ymin, ymax) = model
                    .meshes
                    .iter()
                    .flat_map(|m| m.vertices.iter())
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(v.pos[1]), hi.max(v.pos[1]))
                    });
                let height = (ymax - ymin).max(0.00001);
                let mesh_vertices = mesh
                    .vertices
                    .iter()
                    .map(|v| Vertex {
                        pos: v.pos,
                        normal: v.normal,
                        uv: [ox + v.uv[0] * scale, oy + v.uv[1] * scale],
                        color: [ox, oy, scale, alpha],
                        kind: 3.75,
                        diffuse: if typ.sight_range.is_finite() && typ.sight_range > 0. {
                            typ.sight_range.to_bits()
                        } else {
                            0
                        },
                        night_diffuse: ((v.pos[1] / height).powi(2) * typ.softness.max(0.))
                            .to_bits(),
                    })
                    .collect();
                grass_builder.mesh(key, mesh_vertices, mesh.indices.clone());
            }
            for placement in &area.placements {
                let x = area.center[0] - half_x
                    + (placement.col as f32 + placement.jitter_x) * area.grid;
                let z = area.center[2] + half_z - placement.row as f32 * area.grid
                    + placement.jitter_z * area.grid;
                if (x - cx) * (x - cx) + (z - cz) * (z - cz) > GRASS_RANGE * GRASS_RANGE {
                    continue;
                }
                let gx = x / t.grid_spacing + t.width as f32 * 0.5;
                let gy = t.height as f32 * 0.5 - z / t.grid_spacing;
                if gx < 0. || gy < 0. || gx >= (t.width - 1) as f32 || gy >= (t.height - 1) as f32 {
                    continue;
                }
                let ix = gx as usize;
                let iy = gy as usize;
                let fx = gx - ix as f32;
                let fy = gy - iy as f32;
                let a = t.heights[iy * t.width + ix];
                let b = t.heights[iy * t.width + ix + 1];
                let c = t.heights[(iy + 1) * t.width + ix];
                let d = t.heights[(iy + 1) * t.width + ix + 1];
                let ground = if fx + fy <= 1. {
                    a + (b - a) * fx + (c - a) * fy
                } else {
                    d + (c - d) * (1. - fx) + (b - d) * (1. - fy)
                };
                if grass_sample.is_none() {
                    grass_sample = Some([x, ground, z]);
                }
                let wind_index = (placement.wind_index + (x - 10000.).abs() * 2.).floor() % 80.;
                grass_builder.instance_for_area(
                    key,
                    [x, ground - 0.02, z],
                    placement.scale,
                    placement.yaw,
                    wind_index,
                    area.center,
                );
            }
        }
    }
    let grass = grass_builder.finish();
    if std::env::var_os("PW_GRASS_DEBUG").is_some() {
        let (v, i, b) = grass.counts();
        eprintln!("Grass instancing: {v} mesh vertices, {i} placements, {b} batches; first position {grass_sample:?}");
    }
    // Stream placed static scenery near the camera; original transforms, geometry and UVs.
    let object_start = indices.len() as u32;
    let nearby = t
        .objects
        .update(cx, cz, (rad as f32 - 1.0) * TILE as f32 * t.grid_spacing)?;
    // Sort largest rectangles first, then use as many atlas pages as needed.
    // Unlike the legacy 128px atlas, local BMDs keep their source dimensions.
    let mut requested: HashMap<String, (usize, usize)> = HashMap::new();
    for &id in &nearby {
        if let Some(model) = t.objects.get(id) {
            for mesh in &model.meshes {
                let key = mesh.texture.to_lowercase();
                if let Some(tex) = t.objects.texture(&key) {
                    requested.insert(key, (tex.width, tex.height));
                }
            }
        }
    }
    let mut requested: Vec<_> = requested.into_iter().collect();
    requested.sort_by_key(|(_, (w, h))| {
        std::cmp::Reverse(w.next_power_of_two().max(TEX) * h.next_power_of_two().max(TEX))
    });
    let mut texture_slots: HashMap<String, (usize, crate::texture_atlas::Slot)> = HashMap::new();
    let mut pages: Vec<(crate::texture_atlas::Atlas, crate::world_objects::Page)> = Vec::new();
    for (key, (w, h)) in &requested {
        let mut chosen = None;
        for (i, (packing, _)) in pages.iter_mut().enumerate() {
            if let Some(slot) = packing.allocate(*w, *h, 0, false) {
                chosen = Some((i, slot));
                break;
            }
        }
        let (i, slot) = if let Some(s) = chosen {
            s
        } else {
            let mut packing = crate::texture_atlas::Atlas::new();
            let slot = packing
                .allocate(*w, *h, 0, false)
                .ok_or("local scenery texture exceeds maximum atlas size")?;
            let i = pages.len();
            pages.push((
                packing,
                crate::world_objects::Page {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    atlas: vec![0; ATLAS * ATLAS * 4],
                    processed: 0,
                    used_height: 0,
                    mips: Vec::new(),
                },
            ));
            (i, slot)
        };
        pages[i].1.used_height = pages[i].1.used_height.max(slot.y + slot.h);
        let tex = t.objects.texture(key).unwrap();
        for py in 0..slot.h {
            for px in 0..slot.w {
                let sample = tex.sample_rgba(
                    (px as f32 + 0.5) / slot.w as f32,
                    (py as f32 + 0.5) / slot.h as f32,
                );
                let o = ((slot.y + py) * ATLAS + slot.x + px) * 4;
                pages[i].1.atlas[o..o + 4].copy_from_slice(&sample);
            }
        }
        texture_slots.insert(key.clone(), (i, slot));
    }
    eprintln!(
        "Local scenery: {} source-resolution textures in {} GPU pages",
        requested.len(),
        pages.len()
    );
    for id in nearby {
        let Some(model) = t.objects.get(id) else {
            continue;
        };
        let z = basis(model.direction);
        let y = basis(model.up);
        let x = y.cross(z).normalize_or_zero();
        for mesh in &model.meshes {
            let key = mesh.texture.to_lowercase();
            let Some((page_id, slot)) = texture_slots.get(&key).copied() else {
                continue;
            };
            // v5 stores the client's no-cull flag in the material; the draw
            // state (stage, alpha test, culling, kind) comes from one place.
            let kind = match crate::bmd_baked::classify(
                mesh.version,
                !mesh.texture.trim().is_empty(),
                mesh.material.as_ref().is_some_and(|m| m.two_sided),
            ) {
                Ok(state) => state.kind,
                Err(e) => {
                    eprintln!("Skipping baked mesh {}: {e}", mesh.name);
                    continue;
                }
            };
            let page = &mut pages[page_id].1;
            let base = page.vertices.len() as u32;
            let ox = slot.x as f32 / ATLAS as f32;
            let oy = slot.y as f32 / ATLAS as f32;
            let sx = slot.w as f32 / ATLAS as f32;
            let sy = slot.h as f32 / ATLAS as f32;
            for vert in &mesh.vertices {
                let p = Vec3::from_array(model.position)
                    + x * (vert.position[0] * model.scale[0])
                    + y * (vert.position[1] * model.scale[1])
                    + z * (vert.position[2] * model.scale[2]);
                let n = (x * vert.normal[0] + y * vert.normal[1] + z * vert.normal[2])
                    .normalize_or_zero();
                page.vertices.push(Vertex {
                    pos: p.to_array(),
                    normal: n.to_array(),
                    uv: [ox + vert.uv[0] * sx, oy + vert.uv[1] * sy],
                    color: [ox, oy, sx, sy],
                    kind,
                    diffuse: vert.day_diffuse,
                    night_diffuse: vert.night_diffuse,
                });
            }
            page.indices
                .extend(mesh.indices.iter().map(|n| base + *n as u32));
        }
    }
    let local_pages = pages
        .into_iter()
        .map(|(_, page)| page.finish())
        .filter(|p| !p.indices.is_empty())
        .collect();
    // Keep opaque indices separate so water can sample reflected/refracted scene passes.
    let water_start = indices.len() as u32;
    let mut water_batches: Vec<WaterBatch> = Vec::new();
    let mut areas: Vec<_> = t.water.iter().collect();
    areas.sort_by(|a, b| a.height.total_cmp(&b.height));
    for a in areas {
        let start = indices.len() as u32;
        let mut bounds = [
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let hx = a.columns as f32 * a.grid_size * 0.5;
        let hz = a.rows as f32 * a.grid_size * 0.5;
        let rgb = [
            ((a.color >> 16) & 255) as f32 / 255.,
            ((a.color >> 8) & 255) as f32 / 255.,
            (a.color & 255) as f32 / 255.,
            1.,
        ];
        for row in 0..a.rows {
            for col in 0..a.columns {
                if a.mask[row * a.columns + col] & 1 == 0 {
                    continue;
                }
                let x = a.center[0] - hx + col as f32 * a.grid_size;
                let z = a.center[2] + hz - row as f32 * a.grid_size;
                let s = a.grid_size;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].max(x + s);
                bounds[2] = bounds[2].min(z - s);
                bounds[3] = bounds[3].max(z);
                let first = vertices.len();
                push_quad(
                    &mut vertices,
                    &mut indices,
                    [
                        [x, a.height, z],
                        [x + s, a.height, z],
                        [x, a.height, z - s],
                        [x + s, a.height, z - s],
                    ],
                    rgb,
                );
                // Retail mesh adds an integer per-cell base, then spans one
                // UV unit within each cell; integer bases wrap identically.
                for (vertex, uv) in vertices[first..]
                    .iter_mut()
                    .zip(crate::water_gpu::cell_base_uv())
                {
                    vertex.uv = uv;
                }
            }
        }
        let end = indices.len() as u32;
        if end > start {
            if let Some(last) = water_batches.last_mut().filter(|last| {
                last.height == a.height && last.simple == a.simple && last.end == start
            }) {
                last.end = end;
                last.bounds = [
                    last.bounds[0].min(bounds[0]),
                    last.bounds[1].max(bounds[1]),
                    last.bounds[2].min(bounds[2]),
                    last.bounds[3].max(bounds[3]),
                ];
            } else {
                water_batches.push(WaterBatch {
                    simple: a.simple,
                    start,
                    end,
                    height: a.height,
                    bounds,
                });
            }
        }
    }
    Ok(MeshData {
        vertices,
        indices,
        grass_mips: crate::grass_mips::prepare(&atlas, &grass_mips, ATLAS),
        grass,
        atlas,
        center: (bx, by),
        tree_center: (cx, cz),
        detail,
        lod: lod_state,
        spans,
        terrain_end,
        object_start,
        water_start,
        water_batches,
        local_pages,
    })
}
const SHADER: &str = r#"
struct Camera { matrix: mat4x4<f32>, direction: vec4<f32>, clip: vec4<f32>, bounds:vec4<f32>, tree_bounds:vec4<f32>, sun:vec4<f32>, light:vec4<f32>, region_ambient:vec4<f32>, region_diffuse:vec4<f32>, region_direction:vec4<f32>, grass_eye:vec4<f32>, grass_wind:vec4<f32>, grass_history:array<vec4<f32>,80>, fog_range:vec4<f32>, fog_color:vec4<f32> }; @group(0) @binding(0) var<uniform> camera:Camera;
@group(0) @binding(1) var atlas:texture_2d<f32>; @group(0) @binding(2) var atlas_sampler:sampler;
fn fogged(linear:vec3<f32>, reciprocal_w:f32)->vec3<f32> {
 if camera.fog_range.z<0.5 {return linear;}
 let view_z=1./max(reciprocal_w,0.0000001);
 let span=camera.fog_range.y-camera.fog_range.x;
 let f=select(clamp((view_z-camera.fog_range.x)/select(span,0.001,abs(span)<0.001),0.,1.),select(0.,1.,view_z>=camera.fog_range.y),abs(span)<0.001);
 let c=max(linear,vec3<f32>(0.));let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(encoded,camera.fog_color.rgb,f);
 return select(v/12.92,pow((max(v,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),v>vec3<f32>(0.04045));
}
// Retail lm_refract_vs, c0=(1,water_height,0.075,0), 0x7a91dd.
// Unlike grass, above-plane BMD vertices have zero submerged depth.
fn bmd_refraction_transmittance(world:vec3<f32>)->f32 {
 let depth=max(camera.clip.x-world.y,0.);let delta=camera.grass_eye.xyz-world;
 if abs(delta.y)<=0.000001 {return select(1.,0.,depth>0.);}
 return exp2(-0.075*depth*length(delta)/delta.y);
}
fn bmd_pass_fog(linear:vec3<f32>,reciprocal_w:f32,transmittance:f32)->vec3<f32> {
 if camera.clip.y>=0. || camera.fog_range.z<0.5 {return fogged(linear,reciprocal_w);}
 let c=max(linear,vec3<f32>(0.));let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(camera.fog_color.rgb,encoded,clamp(transmittance,0.,1.));
 return select(v/12.92,pow((max(v,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),v>vec3<f32>(0.04045));
}

struct VIn { @location(0) pos:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) color:vec4<f32>, @location(4) kind:f32, @location(5) diffuse:u32, @location(7) night_diffuse:u32 };
struct VOut { @builtin(position) pos:vec4<f32>, @location(9) bmd_refraction:f32, @location(0) normal:vec3<f32>, @location(1) uv:vec2<f32>, @location(2) color:vec4<f32>, @location(3) @interpolate(flat) kind:f32, @location(4) world:vec3<f32>, @location(5) diffuse:vec3<f32> };
// 0x7b9bc0 quantizes the night weight, mixes bytes with integer products,
// divides by 256 (not 255), and forces alpha to 255. Keep that arithmetic
// integer until the final vertex-color conversion; UNORM round trips can
// otherwise fall just below an exact integer at a floor boundary.
fn bmd_baked_rgb(day:u32, night:u32, night_factor:f32)->vec3<u32> {
 let w=u32(clamp(night_factor,0.,1.)*255.);
 let a=vec3<u32>((day>>16u)&255u,(day>>8u)&255u,day&255u);
 let b=vec3<u32>((night>>16u)&255u,(night>>8u)&255u,night&255u);
 return (a*(255u-w)+b*w)>>vec3<u32>(8u);
}
@vertex fn vs(i:VIn)->VOut { var o:VOut; var pos=i.pos; var norm=i.normal;
 if i.kind>4.5 { let d=normalize(camera.direction.xyz); let az=atan2(d.z,d.x); let el=asin(clamp(d.y,-1.,1.)); let rho=i.color.w; let q=vec3<f32>(cos(el)*i.normal.x-sin(el)*i.normal.z,i.normal.y,sin(el)*i.normal.x+cos(el)*i.normal.z); let a=az+rho;var v=vec3<f32>(cos(a)*q.x-sin(a)*q.y,sin(a)*q.x+cos(a)*q.y,q.z);if rho>1000. { v=i.normal; }pos+=vec3<f32>(v.x,v.z,v.y);norm=vec3<f32>(0.,1.,0.); }
 o.pos=camera.matrix*vec4<f32>(pos,1.);o.bmd_refraction=1.;if camera.clip.y<0. && i.kind>1.5 && i.kind<2.5 {o.bmd_refraction=bmd_refraction_transmittance(pos);}o.world=pos;o.normal=norm;o.uv=i.uv;o.color=i.color;o.kind=i.kind;o.diffuse=vec3<f32>(bmd_baked_rgb(i.diffuse,i.night_diffuse,1.-camera.direction.w))/255.;return o; }
@fragment fn fs(i:VOut, @builtin(front_facing) front:bool)->@location(0) vec4<f32> { if i.kind>1.5 && i.kind<2.05 && camera.clip.w == 0. && (front == (camera.clip.z != 0.)) {discard;} if camera.clip.y != 0.0 && (i.world.y-camera.clip.x)*camera.clip.y < 0.0 {discard;} if i.kind>0.5 && i.kind<1.5 {return vec4<f32>(fogged(i.color.rgb,i.pos.w),1.);} let uv_dx=dpdx(i.uv);let uv_dy=dpdy(i.uv);var uv=i.uv; if i.kind>1.5 {let scale=vec2<f32>(i.color.z,select(i.color.z,i.color.w,i.kind<2.5));let dims=vec2<f32>(textureDimensions(atlas));let mip=clamp(log2(max(max(length(uv_dx*dims),length(uv_dy*dims)),1.)),0.,7.);let border=0.5*exp2(mip);let half_pixel=min(vec2<f32>(border)/dims,scale*0.499);let local=fract((i.uv-i.color.xy)/scale)*scale;uv=i.color.xy+clamp(local,half_pixel,scale-half_pixel);} let raw_tex=textureSampleGrad(atlas,atlas_sampler,uv,uv_dx,uv_dy);let encoded=raw_tex.rgb;let linear=select(encoded/12.92,pow((encoded+0.055)/1.055,vec3<f32>(2.4)),encoded>vec3<f32>(0.04045));let tex=vec4<f32>(linear,raw_tex.a);if ((i.kind>1.5 && i.kind<2.5) || i.kind>3.5) && tex.a<select(0.5,i.color.w,i.kind>3.5 && i.kind<3.9) {discard;} if i.kind>1.5 && i.kind<2.5 {let c=clamp(raw_tex.rgb*i.diffuse*2.,vec3<f32>(0.),vec3<f32>(1.));let rgb=select(c/12.92,pow((c+0.055)/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));return vec4<f32>(bmd_pass_fog(rgb,i.pos.w,i.bmd_refraction),1.);} let light=camera.light.x+camera.light.y*max(dot(normalize(i.normal),normalize(camera.sun.xyz)),0.);return vec4<f32>(fogged(tex.rgb*light*camera.light.zww,i.pos.w),1.);}
"#;
struct Renderer {
    models: Option<crate::world_models_gpu::World>,
    model_lights: std::sync::Arc<crate::scene_lights::SceneLights>,
    model_scene: std::sync::Arc<crate::scene::Scene>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    terrain_gpu: crate::terrain_gpu::TerrainGpu,
    sky: crate::sky_gpu::SkyGpu,
    far_trees: crate::far_trees_gpu::FarTrees,
    grass: crate::grass_instances::Gpu,
    tree_center: (f32, f32),
    detail: DetailSettings,
    /// Window/levels the current terrain mesh was built for.
    lod: LodState,
    /// Resident terrain blocks (`0x77e250`) and the runs submitted last frame.
    spans: Vec<crate::terrain_residency::Span>,
    terrain_runs: Vec<(u32, u32)>,
    terrain_end: u32,
    world_width: usize,
    world_height: usize,
    world_spacing: f32,
    uniform: wgpu::Buffer,
    reflected_uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    reflected_bind: wgpu::BindGroup,
    water: crate::water_gpu::WaterGpu,
    simple_water: crate::simple_water_gpu::SimpleWaterGpu,
    shore: Option<crate::shore_gpu::ShoreGpu>,
    post: crate::post_gpu::PostGpu,
    object_start: u32,
    local_pages: Vec<crate::world_objects::GpuPage>,
    world_pages: Vec<crate::world_objects::GpuPage>,
    world_done: bool,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    water_start: u32,
    water_batches: Vec<WaterBatch>,
    started: Instant,
    wind_last: Instant,
    world_glow: crate::post_gpu::WorldGlow,
    celestial_visibility: crate::client_sun::Visibility,
    // Native SetLightParam/SetAmbient state persists when a frame selects no
    // environment box (0x41ec65 keeps the in-flight environment). Keeping the
    // last published regional light avoids erasing ordinary model light on an
    // uncovered frame while baked surfaces stay independent of it.
    retained_regional_light: Option<crate::environment::RegionalLight>,
    glow_requested_fade: Option<f32>,
    grass_wind_state: crate::grass_wind::GrassWind,
    atlas: wgpu::Texture,
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    depth: wgpu::Texture,
    center: (isize, isize),
}
pub(crate) fn depth(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
impl Renderer {
    fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
        m: MeshData,
        t: &mut Terrain,
    ) -> Result<Self, String> {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera"),
            contents: bytemuck::bytes_of(&Uniform {
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
                direction: [0., 0., -1., 0.],
                clip: [0.; 4],
                bounds: [0.; 4],
                tree_bounds: [0.; 4],
                sun: [-0.32, 0.82, 0.46, 0.],
                light: [0.55, 0.45, 1., 1.],
                region_ambient: [0.; 4],
                region_diffuse: [0.; 4],
                region_direction: [0.; 4],
                grass_eye: [0.; 4],
                grass_wind: [0.; 4],
                grass_history: [[0.; 4]; 80],
                fog_range: [0.; 4],
                fog_color: [0.; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let reflected_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("reflected camera"),
            contents: bytemuck::bytes_of(&Uniform {
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
                direction: [0., 0., -1., 0.],
                clip: [0.; 4],
                bounds: [0.; 4],
                tree_bounds: [0.; 4],
                sun: [-0.32, 0.82, 0.46, 0.],
                light: [0.55, 0.45, 1., 1.],
                region_ambient: [0.; 4],
                region_diffuse: [0.; 4],
                region_direction: [0.; 4],
                grass_eye: [0.; 4],
                grass_wind: [0.; 4],
                grass_history: [[0.; 4]; 80],
                fog_range: [0.; 4],
                fog_color: [0.; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world material atlas with grass mips"),
            size: wgpu::Extent3d {
                width: ATLAS as u32,
                height: ATLAS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 8,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // D3D8 samples/filter authored color bytes without automatic sRGB decode.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = atlas.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let reflected_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reflected world bindings"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: reflected_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let sky = crate::sky_gpu::SkyGpu::new(&device, &queue, format, &t.element)?;
        let water =
            crate::water_gpu::WaterGpu::new(&device, &queue, format, w, h, &t.water_normals);
        let simple_water = crate::simple_water_gpu::SimpleWaterGpu::new(
            &device,
            &queue,
            format,
            &t.water_simple_frames,
        );
        let shore =
            match crate::shore_gpu::ShoreGpu::new(&device, &queue, format, &t.element, &t.water) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Shoreline waves unavailable: {e}");
                    None
                }
            };
        let far_trees = crate::far_trees_gpu::FarTrees::new(
            &device,
            &queue,
            format,
            &t.trees,
            &uniform,
            &reflected_uniform,
        );
        let terrain_gpu = crate::terrain_gpu::TerrainGpu::new(
            &device,
            &queue,
            format,
            t,
            &uniform,
            &reflected_uniform,
        )?;
        // Material masks and DDS buffers are no longer needed once uploaded.
        // Flush staging before adding native-size scenery pages to cap peak memory.
        t.materials.release_gpu_source();
        queue.submit(std::iter::empty());
        device.poll(wgpu::Maintain::Wait);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world WGSL"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        const ATTR: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4,4=>Float32,5=>Uint32,7=>Uint32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world GPU pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTR,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&m.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&m.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let count = m.indices.len() as u32;
        crate::grass_mips::upload(&queue, &atlas, &m.atlas, &m.grass_mips, ATLAS);
        let grass = crate::grass_instances::Gpu::new(
            &device,
            format,
            &layout,
            SHADER,
            m.grass,
            Some(&terrain_gpu.grass_light_layout),
        );
        let d = depth(&device, w, h);
        let post = crate::post_gpu::PostGpu::new(&device, format, w, h);
        let local_pages = m
            .local_pages
            .into_iter()
            .map(|page| {
                crate::world_objects::GpuPage::upload(
                    &device,
                    &queue,
                    &layout,
                    &sampler,
                    &uniform,
                    &reflected_uniform,
                    page,
                )
            })
            .collect();
        // The verified ordinary world queue is enabled by default. Unsupported
        // material/effect paths remain explicit instance failures, not stand-ins.
        let models = if std::env::var_os("PW_DISABLE_WORLD_MODELS").is_none()
            && !t.models.instances.is_empty()
        {
            let assets = std::mem::take(&mut t.models);
            let mut world =
                crate::world_models_gpu::World::new(&device, format, assets, &t.element)?;
            // Actual world effect owners: the ECWLD placements and the hook-free
            // ECM EventType-100 roots publish into the same scene-light grid the
            // receiving models query. A missing archive is a reported failure,
            // not a silent removal of the producer path.
            match world.attach_gfx_archive(&t.element) {
                Ok(()) => {}
                Err(e) => eprintln!("World FX light roots unavailable: {e}"),
            }
            let effects = match world.install_scene_effects(&t.scene) {
                Ok(count) => count,
                Err(e) => return Err(format!("ECWLD effect light owners: {e}")),
            };
            let events = match world.install_model_events() {
                Ok(count) => count,
                Err(e) => return Err(format!("ECM event light owners: {e}")),
            };
            eprintln!("Installed {effects} scene effect and {events} model-event light producers");
            for failure in world.gfx_failures() {
                eprintln!("FX light: {failure}");
            }
            Some(world)
        } else {
            None
        };
        Ok(Self {
            models,
            model_lights: t.scene_lights.clone(),
            model_scene: t.scene.clone(),
            terrain_gpu,
            sky,
            far_trees,
            grass,
            tree_center: m.tree_center,
            detail: m.detail,
            terrain_end: m.terrain_end,
            world_width: t.width,
            world_height: t.height,
            world_spacing: t.grid_spacing,
            device,
            queue,
            pipeline,
            uniform,
            reflected_uniform,
            bind,
            reflected_bind,
            post,
            water,
            simple_water,
            shore,
            object_start: m.object_start,
            local_pages,
            world_pages: Vec::new(),
            world_done: false,
            layout,
            sampler,
            water_start: m.water_start,
            water_batches: m.water_batches,
            started: Instant::now(),
            wind_last: Instant::now(),
            world_glow: crate::post_gpu::WorldGlow::default(),
            celestial_visibility: crate::client_sun::Visibility::default(),
            retained_regional_light: None,
            glow_requested_fade: None,
            grass_wind_state: crate::grass_wind::GrassWind::new(),
            atlas,
            vertex,
            index,
            count,
            depth: d,
            center: m.center,
            lod: m.lod,
            spans: m.spans,
            terrain_runs: Vec::new(),
        })
    }
    fn update_mesh(&mut self, m: MeshData) {
        self.grass.update(&self.device, m.grass);
        self.local_pages.clear();
        self.local_pages = m
            .local_pages
            .into_iter()
            .map(|page| {
                crate::world_objects::GpuPage::upload(
                    &self.device,
                    &self.queue,
                    &self.layout,
                    &self.sampler,
                    &self.uniform,
                    &self.reflected_uniform,
                    page,
                )
            })
            .collect();
        self.vertex = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&m.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.index = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&m.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.count = m.indices.len() as u32;
        self.center = m.center;
        self.tree_center = m.tree_center;
        self.detail = m.detail;
        self.lod = m.lod;
        self.spans = m.spans;
        self.terrain_end = m.terrain_end;
        self.object_start = m.object_start;
        self.water_start = m.water_start;
        self.water_batches = m.water_batches;
        crate::grass_mips::upload(&self.queue, &self.atlas, &m.atlas, &m.grass_mips, ATLAS);
    }
    fn draw_opaque(
        &self,
        e: &mut wgpu::CommandEncoder,
        label: &str,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        bind: &wgpu::BindGroup,
        reflected: bool,
        sky_reflected: bool,
        terrain_depth_source: Option<&wgpu::Texture>,
        clear_color: Option<wgpu::Color>,
    ) {
        let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color.unwrap_or(wgpu::Color {
                        r: 0.18,
                        g: 0.32,
                        b: 0.48,
                        a: 1.,
                    })),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if clear_color.is_none() {
            self.sky.draw(&mut p, sky_reflected);
        }
        // The main view submits only the blocks its frustum keeps
        // (`block+0x74`); the other passes reuse the full terrain because they
        // look through different matrices and must not lose blocks.
        let runs = if reflected {
            None
        } else {
            Some(self.terrain_runs.as_slice())
        };
        self.terrain_gpu.draw(
            &mut p,
            reflected,
            &self.vertex,
            &self.index,
            self.terrain_end,
            runs,
        );
        drop(p);
        if let Some(source) = terrain_depth_source {
            self.water.copy_terrain_depth(e, source);
        }
        let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("opaque scenery after terrain"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if std::env::var_os("PW_UI_TEST").is_none()
            && std::env::var_os("PW_SKIP_TREES_TEST").is_none()
        {
            self.far_trees.draw(&mut p, reflected);
        }
        self.grass
            .draw(&mut p, bind, Some(&self.terrain_gpu.grass_light_bind));
        p.set_pipeline(&self.pipeline);
        p.set_bind_group(0, bind, &[]);
        p.set_vertex_buffer(0, self.vertex.slice(..));
        p.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        let local_end = if self.world_done {
            self.object_start
        } else {
            self.water_start
        };
        p.draw_indexed(self.terrain_end..local_end, 0, 0..1);
        if !self.world_done {
            for page in &self.local_pages {
                page.draw(&mut p, reflected);
            }
        }
        // Do not submit world pages on top of the same locally streamed
        // ornaments while preload is in progress: the coplanar duplicates
        // caused depth flicker. Switch atomically only after all pages arrive.
        if self.world_done {
            for page in &self.world_pages {
                page.draw(&mut p, reflected);
            }
        }
    }
    fn draw_models(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        uniform: &Uniform,
        camera: &Camera,
        advance: bool,
    ) {
        // Effects outside this radius are unloaded and publish no lights. The
        // radius is in ECWLD blocks, using the scene's authored block size.
        let world_radius = self.detail.radius.max(1) as f32 * self.model_scene.block_size.max(1.);
        if let Some(light) = camera.regional_light {
            self.retained_regional_light = Some(light);
        }
        let light = camera.regional_light.or(self.retained_regional_light);
        if let Some(models) = &mut self.models {
            models.set_view_center(camera.pos.x, camera.pos.z, world_radius);
        }
        if let (Some(models), Some(light)) = (&mut self.models, light) {
            // Reuse the root logic tick used by regions and the glow presenter.
            // A fresh wall-time sample here both bypassed the 250ms cap and
            // included all asset-loading time on the first receiving-model draw.
            // Water passes reuse the sampled pose; never tick again per view.
            let delta = if advance { camera.logic_delta_ms } else { 0 };
            // Authored EC-BSD direction only (light_consumers::DirectionSource
            // ::Authored). The animated celestial curve never reaches here.
            let d = light.material_direction().travel();
            let frame = crate::model_draw_gpu::Frame {
                blend_enabled: false,
                matrix: uniform.matrix,
                lighting: crate::model_material::Lighting {
                    environment_ambient: [light.ambient[0], light.ambient[1], light.ambient[2], 1.],
                    additional_ambient: [0.; 4],
                    directional_diffuse: [light.diffuse[0], light.diffuse[1], light.diffuse[2], 1.],
                    point_ambient: [0.; 4],
                    point_diffuse: [0.; 4],
                    transparency: -1.,
                    point_enabled: false,
                },
                direction: d,
                point_position: [0.; 3],
                attenuation: [0., 0.4, 0.],
                eye_direction: uniform.direction[..3].try_into().unwrap(),
                directional_specular: [light.specular[0], light.specular[1], light.specular[2], 1.],
                specular_variant: true,
                vertex_fog: crate::fog_matrix::MODEL_VERTEX_FOG,
                fog_color: uniform.fog_color,
                fog_range: uniform.fog_range,
                clip_plane: [0., uniform.clip[1], 0., -uniform.clip[0] * uniform.clip[1]],
                reverse_front: uniform.clip[2] != 0.,
                alpha_compare: 5,
                alpha_reference: 84. / 255.,
            };
            models.update_pass(
                &self.device,
                &self.queue,
                frame,
                camera.pos.to_array(),
                &self.model_lights,
                crate::environment::night_factor(camera.time_of_day),
                delta,
                advance,
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world receiving-model pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            models.draw(&mut pass);
        }
    }
    /// Render the actual loaded terrain and scenery from a north-up orthographic camera.
    /// Water is drawn as its real client grid with a flat color in this map pass.
    fn render_minimap(
        &self,
        target: &wgpu::TextureView,
        map_depth: &wgpu::TextureView,
        camera: &Camera,
        range: f32,
        full_world: bool,
    ) {
        let matrix = if full_world {
            let w = self.world_width as f32 * self.world_spacing;
            let h = self.world_height as f32 * self.world_spacing;
            Mat4::orthographic_rh(w * 0.5, -w * 0.5, -h * 0.5, h * 0.5, 20000., 1.)
                * Mat4::look_to_rh(Vec3::new(0., 10000., 0.), Vec3::NEG_Y, Vec3::Z)
        } else {
            minimap_matrix(camera.pos.x, camera.pos.z, range)
        };
        let (bx, by) = self.center;
        let rad = self.detail.radius;
        let maxx = ((self.world_width - 1) / TILE * TILE) as isize;
        let maxy = ((self.world_height - 1) / TILE * TILE) as isize;
        let x0 = ((bx - rad) * TILE as isize).clamp(0, maxx);
        let x1 = ((bx + rad + 1) * TILE as isize).clamp(0, maxx);
        let y0 = ((by - rad) * TILE as isize).clamp(0, maxy);
        let y1 = ((by + rad + 1) * TILE as isize).clamp(0, maxy);
        let bounds = [
            (x0 as f32 - self.world_width as f32 * 0.5) * self.world_spacing,
            (x1 as f32 - self.world_width as f32 * 0.5) * self.world_spacing,
            (self.world_height as f32 * 0.5 - y1 as f32) * self.world_spacing,
            (self.world_height as f32 * 0.5 - y0 as f32) * self.world_spacing,
        ];
        let angle = (camera.time_of_day - 6.) * std::f32::consts::PI / 12.;
        let daylight = crate::environment::daylight(camera.time_of_day);
        let twilight = (1. - (angle.sin().abs() / 0.35).clamp(0., 1.)) * daylight;
        let uniform = Uniform {
            matrix: matrix.to_cols_array_2d(),
            direction: [0., -1., 0., daylight],
            region_ambient: camera.region_uniform().0,
            region_diffuse: camera.region_uniform().1,
            region_direction: camera.region_uniform().2,
            grass_eye: [
                camera.pos.x,
                camera.pos.y,
                camera.pos.z,
                if camera.grass_distance_fade { 1. } else { 0. },
            ],
            grass_wind: [
                self.started.elapsed().as_secs_f32(),
                camera.grass_wind_strength,
                if camera.area_grass_lightmaps { 1. } else { 0. },
                0.,
            ],
            grass_history: self.grass_wind_state.history(),
            fog_range: [0.; 4],
            fog_color: [0.; 4],
            clip: [
                0.,
                0.,
                1., // horizontal projection flip reverses winding
                if camera.model_backface_culling {
                    0.
                } else {
                    1.
                },
            ],
            bounds,
            tree_bounds: [
                camera.pos.x,
                camera.pos.z,
                camera.tree_wind_strength,
                self.started.elapsed().as_secs_f32(),
            ],
            sun: [
                angle.cos() * 0.8,
                angle.sin().max(0.05),
                0.46,
                if camera.terrain_lightmaps {
                    daylight
                } else {
                    -1.
                },
            ],
            light: [
                0.17 + 0.38 * daylight,
                0.07 + 0.38 * daylight,
                1.,
                1. - 0.18 * twilight,
            ],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let mut e = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("minimap"),
            });
        self.draw_opaque(
            &mut e,
            "minimap top down",
            target,
            map_depth,
            &self.bind,
            false,
            false,
            None,
            None,
        );
        {
            let mut pass = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("minimap water grids"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: map_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, self.vertex.slice(..));
            pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(self.water_start..self.count, 0, 0..1);
        }
        self.queue.submit(Some(e.finish()));
    }
    fn render(
        &mut self,
        target: &wgpu::TextureView,
        target_raw: &wgpu::TextureView,
        matrix: Mat4,
        camera: &Camera,
        w: u32,
        h: u32,
    ) {
        // Per-frame, per-view visibility of the resident terrain (`0x77e250`):
        // the same block walk the client runs before the level rule.
        self.terrain_runs = crate::terrain_residency::visible_runs(&self.spans, matrix);
        let now = Instant::now();
        let dt = now.duration_since(self.wind_last).as_secs_f32();
        self.grass_wind_state.update(dt);
        self.wind_last = now;
        self.post.set_sharpen(camera.sharpen);
        self.world_glow.select(
            camera.bloom,
            crate::environment::daylight(camera.time_of_day),
            camera.underwater_depth > 0.,
            camera.bloom_mode,
            camera.bloom_tint,
        );
        // Viewer fade-control changes are explicit SetFade requests, not a
        // permanently forced value that defeats native mode-triggered StopFade.
        if camera.bloom && self.glow_requested_fade != Some(camera.bloom_brightness_fade) {
            self.world_glow.set_fade(
                self.world_glow.brightness_fade(),
                camera.bloom_brightness_fade,
                1000,
            );
            self.glow_requested_fade = Some(camera.bloom_brightness_fade);
        }
        // AdjustEnvironment passes the frozen +0x490 logic tick to glow
        // (0x4214dc), including its 250ms cap; do not resample wall time here.
        self.world_glow.tick(camera.logic_delta_ms as i32);
        let glow_parameters = self.world_glow.parameters();
        self.post.set_glow_color(
            camera.bloom_tint,
            self.world_glow.brightness_fade(),
            camera.bloom_opacity_fade,
        );
        let direction = camera.direction();
        // Only real scene effects allocate the chain; the retained screen-space
        // fog diagnostic is not an input (production fog is per material).
        let use_post = crate::view_settings::post_required(
            camera.bloom,
            camera.sharpen,
            camera.underwater_depth > 0.,
            camera.client_gamma,
        );
        let post_view = if use_post {
            Some(self.post.scene_view())
        } else {
            None
        };
        let scene_target = post_view.as_ref().unwrap_or(target);
        let post_raw = if use_post {
            Some(self.post.scene_raw_view())
        } else {
            None
        };
        let scene_target_raw = post_raw.as_ref().unwrap_or(target_raw);
        // Disc azimuth comes from the authored region direction; elevation is
        // the recovered sky clock. Only the celestial rows read the curve.
        let authored = camera
            .regional_light
            .or(self.retained_regional_light)
            .map(|l| l.material_direction().toward_sun())
            .unwrap_or([0.6, -0.4, 0.8]);
        let celestial = crate::client_sun::sky_object_direction(camera.time_of_day, authored)
            .unwrap_or([0., -1., 0.])
            .map(|x| -x);
        // The same retained light that feeds the discs' azimuth also keeps the
        // published default-area flag word stable across frames with no region.
        self.celestial_visibility.update(
            camera.logic_delta_ms,
            camera
                .regional_light
                .or(self.retained_regional_light)
                .map(|l| l.celestial_flags)
                .unwrap_or(0x18),
        );
        let celestial_alpha = if camera.celestial_discs {
            self.celestial_visibility.alpha(camera.time_of_day)
        } else {
            0.
        };
        self.sky.update(
            &self.queue,
            direction,
            w,
            h,
            camera.time_of_day,
            celestial,
            celestial_alpha,
            false,
        );
        self.sky.update(
            &self.queue,
            direction,
            w,
            h,
            camera.time_of_day,
            celestial,
            celestial_alpha,
            true,
        );
        self.far_trees.update_lods(
            &self.queue,
            camera.pos.to_array(),
            self.detail.tree_lod_scale,
        );
        let (bx, by) = self.center;
        let rad = self.detail.radius;
        let maxx = ((self.world_width - 1) / TILE * TILE) as isize;
        let maxy = ((self.world_height - 1) / TILE * TILE) as isize;
        let x0 = ((bx - rad) * TILE as isize).clamp(0, maxx);
        let x1 = ((bx + rad + 1) * TILE as isize).clamp(0, maxx);
        let y0 = ((by - rad) * TILE as isize).clamp(0, maxy);
        let y1 = ((by + rad + 1) * TILE as isize).clamp(0, maxy);
        let bounds = [
            (x0 as f32 - self.world_width as f32 * 0.5) * self.world_spacing,
            (x1 as f32 - self.world_width as f32 * 0.5) * self.world_spacing,
            (self.world_height as f32 * 0.5 - y1 as f32) * self.world_spacing,
            (self.world_height as f32 * 0.5 - y0 as f32) * self.world_spacing,
        ];
        // Tree LODs are selected per instance on the GPU, not by the streamer.
        let tree_bounds = [
            camera.pos.x,
            camera.pos.z,
            camera.tree_wind_strength,
            self.started.elapsed().as_secs_f32(),
        ];
        let angle = (camera.time_of_day - 6.0) * std::f32::consts::PI / 12.0;
        let daylight = crate::environment::daylight(camera.time_of_day);
        let twilight = (1. - ((angle.sin().abs()) / 0.35).clamp(0., 1.)) * daylight;
        let sun = [
            angle.cos() * 0.8,
            angle.sin().max(0.05),
            0.46,
            if camera.terrain_lightmaps {
                daylight
            } else {
                -1.
            },
        ];
        let light = [
            0.17 + 0.38 * daylight,
            0.07 + 0.38 * daylight,
            1.,
            1. - 0.18 * twilight,
        ];
        let normal = Uniform {
            matrix: matrix.to_cols_array_2d(),
            direction: [direction.x, direction.y, direction.z, daylight],
            clip: [
                0.,
                0.,
                1., // horizontal projection flip reverses winding
                if camera.model_backface_culling {
                    0.
                } else {
                    1.
                },
            ],
            bounds,
            tree_bounds,
            sun,
            light,
            region_ambient: camera.region_uniform().0,
            region_diffuse: camera.region_uniform().1,
            region_direction: camera.region_uniform().2,
            grass_eye: [
                camera.pos.x,
                camera.pos.y,
                camera.pos.z,
                if camera.grass_distance_fade { 1. } else { 0. },
            ],
            grass_wind: [
                self.started.elapsed().as_secs_f32(),
                camera.grass_wind_strength,
                if camera.area_grass_lightmaps { 1. } else { 0. },
                0.,
            ],
            grass_history: self.grass_wind_state.history(),
            fog_range: camera.fog_uniform().0,
            fog_color: camera.fog_uniform().1,
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&normal));
        self.water.update(
            &self.queue,
            &crate::water_gpu::WaterUniform {
                matrix: matrix.to_cols_array_2d(),
                inverse_matrix: matrix.inverse().to_cols_array_2d(),
                eye_time: [
                    camera.pos.x,
                    camera.pos.y,
                    camera.pos.z,
                    self.started.elapsed().as_secs_f32(),
                ],
                size: [
                    w as f32,
                    h as f32,
                    camera.far,
                    if camera.water_reflections { 1. } else { 0. },
                ],
                animation: [
                    crate::water_gpu::normal_phase(self.started.elapsed().as_secs_f64()),
                    camera.underwater_fog.end,
                    0.,
                    crate::water_gpu::normal_offset_angle(self.started.elapsed().as_secs_f64()),
                ],
                edge_mode: [if camera.water_reflections { 1. } else { 0. }, 0., 0., 0.],
                // Water-surface fog: the client binds the region's own triple
                // (white 100..400 above water, blue 10..60 below).
                fog: camera.active_fog().range(),
                fog_color: camera.active_fog().color4(),
            },
        );
        self.simple_water
            .update(&self.queue, matrix, self.started.elapsed().as_secs_f64());
        let reflection = self.water.reflection();
        let reflect_depth = self.water.depth();
        let refraction = self.water.refraction();
        let scene_color = self.water.scene();
        let scene_depth = self.depth.create_view(&Default::default());
        let mut e = self.device.create_command_encoder(&Default::default());
        self.draw_opaque(
            &mut e,
            "main scene",
            &scene_color,
            &scene_depth,
            &self.bind,
            false,
            false,
            Some(&self.depth),
            None,
        );
        let model_target = self.water.scene_raw();
        self.draw_models(&mut e, &model_target, &scene_depth, &normal, camera, true);
        self.water.copy_scene_to_refraction(&mut e);
        self.water.copy_depth(&mut e, &self.depth);
        self.water.blit(&mut e, scene_target);
        self.queue.submit(Some(e.finish()));
        // Diagnostic mode for exercising window/input UI on memory-limited software GPUs.
        if std::env::var_os("PW_UI_TEST").is_some() {
            if use_post {
                self.post.render(
                    &self.device,
                    &self.queue,
                    target,
                    &self.water.sample_depth(),
                    w,
                    h,
                    self.started.elapsed().as_secs_f32(),
                    camera.far,
                    camera.underwater_depth,
                    if camera.underwater_depth > 0. {
                        camera.underwater_fog
                    } else {
                        camera.air_fog
                    },
                    false, // no screen-space fog; scene geometry uses material fog
                    false,
                    camera.bloom,
                    camera.bloom_strength,
                    glow_parameters,
                    camera.client_gamma,
                    camera.gamma_setting,
                );
            }
            return;
        }

        // The reflection texture is reused only after its previous water draw is
        // submitted. Each distinct height gets its own mirror and clip plane.
        let mut visible: Vec<WaterBatch> = self
            .water_batches
            .iter()
            .filter(|b| b.visible(matrix))
            .cloned()
            .collect();
        visible.sort_by(|a, b| {
            b.distance_squared(camera.pos)
                .total_cmp(&a.distance_squared(camera.pos))
        });
        if std::env::var_os("PW_WATER_DEBUG").is_some() {
            static LOG_ONCE: std::sync::Once = std::sync::Once::new();
            LOG_ONCE.call_once(|| {
                eprintln!(
                    "Visible water reflection planes: {:?}",
                    visible.iter().map(|b| b.height).collect::<Vec<_>>()
                );
            });
        }
        if !camera.water_reflections {
            let mut e = self.device.create_command_encoder(&Default::default());
            for batch in visible {
                if batch.simple {
                    self.simple_water.draw(
                        &mut e,
                        scene_target_raw,
                        &scene_depth,
                        &self.vertex,
                        &self.index,
                        batch.start,
                        batch.end,
                    );
                    continue;
                }
                self.water.draw_water(
                    &mut e,
                    scene_target,
                    &scene_depth,
                    &self.vertex,
                    &self.index,
                    batch.start,
                    batch.end,
                );
            }
            self.queue.submit(Some(e.finish()));
            if let Some(shore) = &self.shore {
                shore.draw(
                    &self.device,
                    &self.queue,
                    scene_target_raw,
                    &scene_depth,
                    matrix,
                    camera.pos,
                    1. - daylight,
                    self.started.elapsed().as_secs_f32(),
                );
            }
            if use_post {
                self.post.render(
                    &self.device,
                    &self.queue,
                    target,
                    &self.water.sample_depth(),
                    w,
                    h,
                    self.started.elapsed().as_secs_f32(),
                    camera.far,
                    camera.underwater_depth,
                    if camera.underwater_depth > 0. {
                        camera.underwater_fog
                    } else {
                        camera.air_fog
                    },
                    false, // no screen-space fog; scene geometry uses material fog
                    false,
                    camera.bloom,
                    camera.bloom_strength,
                    glow_parameters,
                    camera.client_gamma,
                    camera.gamma_setting,
                );
            }
            return;
        }
        for batch in visible {
            if batch.simple {
                let mut e = self.device.create_command_encoder(&Default::default());
                self.simple_water.draw(
                    &mut e,
                    scene_target_raw,
                    &scene_depth,
                    &self.vertex,
                    &self.index,
                    batch.start,
                    batch.end,
                );
                self.queue.submit(Some(e.finish()));
                continue;
            }
            let plane = batch.height;
            if camera.pos.y > plane {
                // Client 0x7cc6ac-0x7cc8b5: render below this plane into
                // refraction, cleared to the region's water fog color. Fog
                // distances start just beyond the eye-to-plane height and
                // extend by 55% of the configured underwater fog end.
                let fog_range = crate::water_gpu::refraction_fog_range(
                    camera.pos.y,
                    plane,
                    camera.underwater_fog.end,
                );
                let mut refracted = normal;
                refracted.clip = [plane, -1., 1., normal.clip[3]];
                // Clipped refraction binds the water fog color with the
                // plane-relative range for every material of that pass.
                let pass_fog = crate::fog_matrix::refraction_fog(
                    crate::fog_matrix::Fog {
                        start: camera.underwater_fog.start,
                        end: camera.underwater_fog.end,
                        color: camera.underwater_fog.color,
                        enabled: true,
                    },
                    fog_range[0],
                    fog_range[1],
                );
                refracted.fog_range = pass_fog.range();
                refracted.fog_color = pass_fog.color4();
                self.queue
                    .write_buffer(&self.reflected_uniform, 0, bytemuck::bytes_of(&refracted));
                let linear = |x: f32| -> f64 {
                    let x = x.clamp(0., 1.);
                    if !self.water.is_srgb() {
                        return x as f64;
                    }
                    if x <= 0.04045 {
                        (x / 12.92) as f64
                    } else {
                        (((x + 0.055) / 1.055).powf(2.4)) as f64
                    }
                };
                let fog = camera.underwater_fog.color;
                let mut re = self.device.create_command_encoder(&Default::default());
                self.draw_opaque(
                    &mut re,
                    "underwater refraction",
                    &refraction,
                    &reflect_depth,
                    &self.reflected_bind,
                    true,
                    false,
                    None,
                    Some(wgpu::Color {
                        r: linear(fog[0]),
                        g: linear(fog[1]),
                        b: linear(fog[2]),
                        a: 1.,
                    }),
                );
                self.queue.submit(Some(re.finish()));
                self.water.render_edge(
                    &self.device,
                    &self.queue,
                    matrix,
                    plane,
                    bounds,
                    &reflect_depth,
                    self.terrain_gpu.edge_mesh(),
                    (&self.vertex, &self.index, self.terrain_end),
                );
            } else {
                // Client 0x7cca50 renders the sky and the scene above this
                // water height into its refraction target, using air fog.
                let mut above = normal;
                above.clip = [plane, 1., 1., normal.clip[3]];
                // 0x7cca50: the below-surface scene is drawn with air fog even
                // while the camera is underwater.
                let above_fog = crate::fog_matrix::below_surface_fog(crate::fog_matrix::Fog {
                    start: camera.air_fog.start,
                    end: camera.air_fog.end,
                    color: camera.air_fog.color,
                    enabled: true,
                });
                above.fog_range = above_fog.range();
                above.fog_color = above_fog.color4();
                self.queue
                    .write_buffer(&self.reflected_uniform, 0, bytemuck::bytes_of(&above));
                let mut re = self.device.create_command_encoder(&Default::default());
                self.draw_opaque(
                    &mut re,
                    "above-water refraction",
                    &refraction,
                    &reflect_depth,
                    &self.reflected_bind,
                    true,
                    false,
                    None,
                    None,
                );
                // Ornamental EC models enter refraction only from underwater.
                if camera.underwater_depth > 0. {
                    let raw = self.water.refraction_raw();
                    self.draw_models(&mut re, &raw, &reflect_depth, &above, camera, false);
                }
                self.water.draw_water(
                    &mut re,
                    scene_target,
                    &scene_depth,
                    &self.vertex,
                    &self.index,
                    batch.start,
                    batch.end,
                );
                self.queue.submit(Some(re.finish()));
                continue;
            }
            let flip = Mat4::from_translation(Vec3::new(0., 2. * plane, 0.))
                * Mat4::from_scale(Vec3::new(1., -1., 1.));
            let reflected = Uniform {
                matrix: (matrix * flip).to_cols_array_2d(),
                direction: [direction.x, -direction.y, direction.z, daylight],
                // The water reflection cancels the main projection's X flip,
                // so the original front-face winding applies again.
                clip: [
                    plane,
                    if camera.pos.y >= plane { 1. } else { -1. },
                    0.,
                    if camera.model_backface_culling {
                        0.
                    } else {
                        1.
                    },
                ],
                bounds,
                tree_bounds,
                sun,
                light,
                region_ambient: camera.region_uniform().0,
                region_diffuse: camera.region_uniform().1,
                region_direction: camera.region_uniform().2,
                grass_eye: [
                    camera.pos.x,
                    camera.pos.y,
                    camera.pos.z,
                    if camera.grass_distance_fade { 1. } else { 0. },
                ],
                grass_wind: [
                    self.started.elapsed().as_secs_f32(),
                    camera.grass_wind_strength,
                    if camera.area_grass_lightmaps { 1. } else { 0. },
                    0.,
                ],
                grass_history: self.grass_wind_state.history(),
                fog_range: camera.fog_uniform().0,
                fog_color: camera.fog_uniform().1,
            };
            self.queue
                .write_buffer(&self.reflected_uniform, 0, bytemuck::bytes_of(&reflected));
            let mut e = self.device.create_command_encoder(&Default::default());
            self.draw_opaque(
                &mut e,
                "reflection",
                &reflection,
                &reflect_depth,
                &self.reflected_bind,
                true,
                true,
                None,
                None,
            );
            let raw = self.water.reflection_raw();
            self.draw_models(&mut e, &raw, &reflect_depth, &reflected, camera, false);
            self.water.draw_water(
                &mut e,
                scene_target,
                &scene_depth,
                &self.vertex,
                &self.index,
                batch.start,
                batch.end,
            );
            self.queue.submit(Some(e.finish()));
        }
        if let Some(shore) = &self.shore {
            shore.draw(
                &self.device,
                &self.queue,
                scene_target_raw,
                &scene_depth,
                matrix,
                camera.pos,
                1. - daylight,
                self.started.elapsed().as_secs_f32(),
            );
        }
        if use_post {
            self.post.render(
                &self.device,
                &self.queue,
                target,
                &self.water.sample_depth(),
                w,
                h,
                self.started.elapsed().as_secs_f32(),
                camera.far,
                camera.underwater_depth,
                if camera.underwater_depth > 0. {
                    camera.underwater_fog
                } else {
                    camera.air_fog
                },
                false, // no screen-space fog; scene geometry uses material fog
                false,
                camera.bloom,
                camera.bloom_strength,
                glow_parameters,
                camera.client_gamma,
                camera.gamma_setting,
            );
        }
    }
}
fn water_depth(areas: &[crate::water::WaterArea], pos: Vec3) -> f32 {
    areas
        .iter()
        .filter(|a| a.height > pos.y && a.contains_xz(pos.x, pos.z))
        .map(|a| a.height - pos.y)
        .min_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.)
}
struct Camera {
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    far: f32,
    time_of_day: f32,
    water_reflections: bool,
    atmospheric_fog: bool,
    celestial_discs: bool,
    bloom: bool,
    sharpen: bool,
    bloom_strength: f32,
    bloom_mode: u8,
    bloom_tint: u32,
    bloom_brightness_fade: f32,
    bloom_opacity_fade: f32,
    client_gamma: bool,
    gamma_setting: f32,
    underwater_depth: f32,
    underwater_fog: crate::environment::Fog,
    air_fog: crate::environment::Fog,
    terrain_lightmaps: bool,
    model_backface_culling: bool,
    tree_wind_strength: f32,
    grass_wind_strength: f32,
    regional_tree_lighting: bool,
    grass_distance_fade: bool,
    area_grass_lightmaps: bool,
    regional_light: Option<crate::environment::RegionalLight>,
    logic_delta_ms: u32,
}
impl Camera {
    fn new(t: &Terrain) -> Self {
        let x = t.width / 2;
        let y = t.height / 2;
        Self {
            pos: Vec3::new(0., t.heights[y * t.width + x] + 160., 0.),
            yaw: 0.,
            pitch: -0.15,
            far: 16000.,
            time_of_day: 12.,
            water_reflections: true,
            atmospheric_fog: false,
            celestial_discs: false,
            bloom: false,
            sharpen: false,
            bloom_strength: 1.,
            bloom_mode: 0,
            bloom_tint: 0xffffffff,
            bloom_brightness_fade: 0.,
            bloom_opacity_fade: 0.,
            client_gamma: false,
            gamma_setting: 41.,
            underwater_depth: 0.,
            underwater_fog: Default::default(),
            air_fog: Default::default(),
            terrain_lightmaps: true,
            model_backface_culling: true,
            tree_wind_strength: 0.2,
            grass_wind_strength: 0.,
            regional_tree_lighting: false,
            grass_distance_fade: true,
            area_grass_lightmaps: true,
            regional_light: None,
            logic_delta_ms: 0,
        }
    }
    fn active_fog(&self) -> crate::fog_matrix::Fog {
        // Main/reflection table fog uses the camera's active regional set; the
        // selection (and its enable rule) lives in the audited fog matrix.
        crate::fog_matrix::camera_fog(
            self.underwater_depth > 0.,
            self.atmospheric_fog,
            crate::fog_matrix::Fog {
                start: self.air_fog.start,
                end: self.air_fog.end,
                color: self.air_fog.color,
                enabled: true,
            },
            crate::fog_matrix::Fog {
                start: self.underwater_fog.start,
                end: self.underwater_fog.end,
                color: self.underwater_fog.color,
                enabled: true,
            },
        )
    }
    fn fog_uniform(&self) -> ([f32; 4], [f32; 4]) {
        let f = self.active_fog();
        (f.range(), f.color4())
    }
    fn region_uniform(&self) -> ([f32; 4], [f32; 4], [f32; 4]) {
        if let Some(l) = self.regional_light.filter(|_| self.regional_tree_lighting) {
            let d = Vec3::from_array(l.toward_sun).normalize_or_zero();
            if d.length_squared() > 0.5 {
                return (
                    [l.ambient[0], l.ambient[1], l.ambient[2], 1.],
                    [l.diffuse[0], l.diffuse[1], l.diffuse[2], 0.],
                    [d.x, d.y, d.z, 0.],
                );
            }
        }
        ([0.; 4], [0.; 4], [0.; 4])
    }
    fn direction(&self) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(s * cp, sp, -c * cp)
    }
    fn matrix(&self, w: u32, h: u32, far: f32) -> Mat4 {
        // Client world is left-handed; a right-handed view must reverse screen X.
        Mat4::from_scale(Vec3::new(-1., 1., 1.))
            * Mat4::perspective_rh(70f32.to_radians(), w as f32 / h.max(1) as f32, far, 1.)
            * Mat4::look_to_rh(self.pos, self.direction(), Vec3::Y)
    }
}
async fn adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let a = if let Ok(name) = std::env::var("WGPU_ADAPTER_NAME") {
        let query = name.to_lowercase();
        instance
            .enumerate_adapters(wgpu::Backends::all())
            .into_iter()
            .find(|a| {
                a.get_info().name.to_lowercase().contains(&query)
                    && surface.map_or(true, |s| a.is_surface_supported(s))
            })
            .ok_or_else(|| format!("No compatible GPU adapter contains {name:?}"))?
    } else {
        instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::util::power_preference_from_env()
                    .unwrap_or(wgpu::PowerPreference::HighPerformance),
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No compatible GPU adapter")?
    };
    eprintln!("GPU adapter: {:?}", a.get_info());
    let (d, q) = a
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: a.features() & wgpu::Features::TEXTURE_COMPRESSION_BC,
                required_limits: wgpu::Limits::downlevel_defaults().using_resolution(a.limits()),
            },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok((a, d, q))
}
pub fn run(
    mut t: Terrain,
    #[cfg(windows)] loading: Option<crate::windows_setup::LoadingWindow>,
) -> Result<(), String> {
    #[cfg(windows)]
    crate::windows_setup::loading_progress("Preparing GPU world and textures...".into());
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Perfect World Explorer — WASD fly, E/Q altitude, arrows look, Shift speed")
            .with_maximized(true)
            .with_visible(false)
            .build(&event_loop)
            .map_err(|e| e.to_string())?,
    );
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::all()),
        ..Default::default()
    });
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| e.to_string())?;
    let (adapter, device, queue) = pollster::block_on(adapter(&instance, Some(&surface)))?;
    let software_adapter = adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    window.set_title(&format!(
        "Perfect World Explorer — {} — click mouse to look, Esc menu, WASD fly",
        adapter.get_info().name
    ));
    let caps = surface.get_capabilities(&adapter);
    let surface_format = *caps
        .formats
        .iter()
        .find(|x| x.is_srgb())
        .unwrap_or(&caps.formats[0]);
    // Every material shader returns linear color and the render target performs
    // the single encode. When the swapchain has no sRGB format, composition
    // happens in an sRGB intermediate that is encoded once at presentation
    // (`present_encode`); a UNORM swapchain is never written with linear values.
    let format = crate::present_encode::render_format(surface_format);
    let size = window.inner_size();
    let mut config = surface
        .get_default_config(&adapter, size.width.max(1), size.height.max(1))
        .ok_or("surface configuration unavailable")?;
    config.format = surface_format;
    if surface_format.is_srgb() {
        config
            .view_formats
            .push(surface_format.remove_srgb_suffix());
    }
    surface.configure(&device, &config);
    let mut camera = Camera::new(&t);
    camera.underwater_fog = t
        .environments
        .underwater(camera.pos.to_array(), camera.time_of_day);
    camera.air_fog = t
        .environments
        .atmosphere(camera.pos.to_array(), camera.time_of_day);
    let mut settings = ViewSettings::load();
    let mut residency = crate::terrain_residency::Residency::default();
    let m = build(
        &mut t,
        camera.pos.x,
        camera.pos.z,
        settings.detail,
        &mut residency,
    )?;
    let mut renderer = Renderer::new(
        device,
        queue,
        format,
        config.width,
        config.height,
        m,
        &mut t,
    )?;
    let mut display_gamma: Option<crate::display_gamma::DisplayGamma> = None;
    // Only allocated for UNORM-only swapchains; None is the normal sRGB route.
    let mut present_encode: Option<crate::present_encode::PresentEncode> =
        if crate::color_chain::present_encode_required(surface_format.is_srgb()) {
            Some(crate::present_encode::PresentEncode::new(
                &renderer.device,
                surface_format,
                config.width,
                config.height,
            ))
        } else {
            None
        };
    let mut music = crate::music::MusicPlayer::new(&t.element);
    #[cfg(windows)]
    drop(loading);
    window.set_visible(true);
    let water_areas = t.water.clone();
    let environments = t.environments.clone();
    let mut fog_transition =
        crate::environment::FogTransition::new(&environments, camera.pos.to_array());
    let mut regional_streaming = environments.streaming();
    let mut streamed_transition = regional_streaming.as_mut().map(|active| {
        crate::environment::FogTransition::new_streamed(
            &environments,
            active,
            camera.pos.to_array(),
        )
    });
    let whole_element = t.element.clone();
    let map_space = crate::navigation::MapSpace {
        width: t.width,
        height: t.height,
        spacing: t.grid_spacing,
        heights: t.heights.clone(),
    };
    let destinations =
        crate::navigation::destinations(&whole_element.join("maps/world/world_targets.txt"));
    let whole_ornaments = t.objects.ornaments.clone();
    let mut world_rx: Option<std::sync::mpsc::Receiver<crate::world_objects::Message>> = None;
    let mut world_cancel: Option<Arc<std::sync::atomic::AtomicBool>> = None;
    let mut world_enabled = false;
    let mut world_status = String::from("Off");
    // The expensive atlas and scene rebuild runs off the UI thread. At most one
    // request is in flight; once it completes, the latest camera position is used.
    let (width, height, spacing) = (t.width, t.height, t.grid_spacing);
    let (request_tx, request_rx) = std::sync::mpsc::channel::<(f32, f32, DetailSettings)>();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel::<Result<MeshData, String>>(1);
    std::thread::Builder::new()
        .name("world-streaming".into())
        .spawn(move || {
            let mut residency = crate::terrain_residency::Residency::default();
            while let Ok((x, z, detail)) = request_rx.recv() {
                let started = Instant::now();
                let result = build(&mut t, x, z, detail, &mut residency);
                eprintln!(
                    "Background terrain build at ({x:.1},{z:.1}) took {:.2}s",
                    started.elapsed().as_secs_f32()
                );
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| format!("cannot start world streaming thread: {e}"))?;
    let egui_ctx = egui::Context::default();
    let mut egui_state = egui_winit::State::new(
        egui_ctx.clone(),
        egui::ViewportId::ROOT,
        &*window,
        Some(window.scale_factor() as f32),
        None,
    );
    let mut egui_renderer = egui_wgpu::Renderer::new(&renderer.device, format, None, 1);
    let map_texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("live minimap"),
        size: wgpu::Extent3d {
            width: 320,
            height: 320,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let map_view = map_texture.create_view(&Default::default());
    let map_depth_tex = depth(&renderer.device, 320, 320);
    let map_depth = map_depth_tex.create_view(&Default::default());
    let map_id = egui_renderer.register_native_texture(
        &renderer.device,
        &map_view,
        wgpu::FilterMode::Linear,
    );
    let full_map_texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("full-world overhead map"),
        size: wgpu::Extent3d {
            width: 1024,
            height: 1408,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let full_map_view = full_map_texture.create_view(&Default::default());
    let full_depth_tex = depth(&renderer.device, 1024, 1408);
    let full_map_depth = full_depth_tex.create_view(&Default::default());
    let full_map_id = egui_renderer.register_native_texture(
        &renderer.device,
        &full_map_view,
        wgpu::FilterMode::Linear,
    );
    let mut original_map_id: Option<egui::TextureHandle> = None;
    let mut original_map_error: Option<String> = None;
    let mut original_map_rx: Option<std::sync::mpsc::Receiver<Result<image::RgbaImage, String>>> =
        None;
    let mut original_map_requested = false;
    let mut map_original = false;
    let mut map_click_teleport = false;
    let mut map_zoom = 1.0f32;
    let mut map_refresh = Instant::now() - std::time::Duration::from_secs(10);
    let mut teleport_query = String::new();
    let ui_zones = crate::music::Zones::open(&whole_element.join("maps/world/precinct.clt")).ok();
    let mut last_zone: Option<usize> = None;
    let mut zone_initialized = false;
    let mut zone_check = Instant::now() - std::time::Duration::from_secs(1);
    let mut zone_banner: Option<(String, Instant)> = None;
    let mut settings_tab = 0usize;
    let mut fullscreen_applied = false;
    let mut menu_page = MenuPage::Main;
    let mut streaming = false;
    let mut keys = HashSet::new();
    let mut captured = false;
    let mut saved_settings = settings.clone();
    let mut last_settings_save = Instant::now();
    let mut last = Instant::now();
    let mut region_tick_ms = renderer.started.elapsed().as_millis() as u64;
    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => {
                if let WindowEvent::KeyboardInput {ref event,..}=event {
                    if event.physical_key == PhysicalKey::Code(KeyCode::Escape) && event.state == ElementState::Pressed && !event.repeat {
                        menu_page=menu_page.escape();keys.clear();
                        if captured {let _=window.set_cursor_grab(CursorGrabMode::None);window.set_cursor_visible(true);captured=false;}
                        if std::env::var_os("PW_INPUT_DEBUG").is_some() {eprintln!("Escape menu: {:?}",menu_page);}
                        return;
                    }
                }
                if let WindowEvent::KeyboardInput {ref event,..}=event {
                    if event.physical_key==PhysicalKey::Code(KeyCode::KeyM) && event.state==ElementState::Pressed && !event.repeat && (menu_page!=MenuPage::Teleports) {
                        menu_page=if menu_page==MenuPage::WorldMap {MenuPage::Closed} else {MenuPage::WorldMap};
                        keys.clear(); map_refresh=Instant::now()-std::time::Duration::from_secs(10);
                        if captured {let _=window.set_cursor_grab(CursorGrabMode::None);window.set_cursor_visible(true);captured=false;}
                        return;
                    }
                }
                // egui must receive focus changes even while the menu is closed;
                // otherwise it believes the window is unfocused and ignores clicks.
                if menu_page.is_open() || matches!(event,WindowEvent::Focused(_)) {
                    let response=egui_state.on_window_event(&window,&event);
                    if menu_page.is_open() && (response.consumed || matches!(event,WindowEvent::KeyboardInput{..})) && !matches!(event, WindowEvent::RedrawRequested | WindowEvent::Resized(_) | WindowEvent::CloseRequested | WindowEvent::Focused(_)) {return;}
                }
                match event {
                WindowEvent::CloseRequested => { if let Err(e)=settings.save(){eprintln!("Settings save: {e}");} target.exit(); },
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(k) = event.physical_key {
                        if event.state == ElementState::Pressed {
                            keys.insert(k);
                        } else {
                            keys.remove(&k);
                        }
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } if !captured && !menu_page.is_open() => {
                    let result = window
                        .set_cursor_grab(CursorGrabMode::Locked)
                        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                    if result.is_ok() {
                        window.set_cursor_visible(false);
                        captured = true;
                        if std::env::var_os("PW_INPUT_DEBUG").is_some() {
                            eprintln!("Mouse captured");
                        }
                    } else {
                        eprintln!("Could not capture mouse: {:?}", result.err());
                    }
                }
                WindowEvent::Focused(false) if captured => {
                    let _ = window.set_cursor_grab(CursorGrabMode::None);
                    window.set_cursor_visible(true);
                    captured = false;
                    keys.clear();
                }
                WindowEvent::Resized(s) if s.width > 0 && s.height > 0 => {
                    config.width = s.width;
                    config.height = s.height;
                    surface.configure(&renderer.device, &config);
                    renderer.depth = depth(&renderer.device, s.width, s.height);
                    renderer.water.resize(&renderer.device, s.width, s.height);
                    renderer.post.resize(&renderer.device,s.width,s.height);
                    if let Some(gamma)=display_gamma.as_mut() {gamma.resize(&renderer.device,s.width,s.height);}
                    if let Some(encode)=present_encode.as_mut() {encode.resize(&renderer.device,s.width,s.height);}
                }
                WindowEvent::RedrawRequested => {
                    if settings != saved_settings && last_settings_save.elapsed().as_secs_f32() >= 1. {
                        if let Err(e)=settings.save(){eprintln!("Settings save: {e}");} else {saved_settings=settings.clone();}
                        last_settings_save=Instant::now();
                    }
                    let dt = last.elapsed().as_secs_f32().min(0.1);
                    last = Instant::now();
                    let speed = if menu_page.is_open() { 0. } else if keys.contains(&KeyCode::ShiftLeft) {
                        settings.camera_speed * settings.boost
                    } else {
                        settings.camera_speed
                    };
                    let (s, c) = camera.yaw.sin_cos();
                    let f = Vec3::new(s, 0., -c);
                    let r = Vec3::new(-c, 0., -s);
                    if keys.contains(&KeyCode::KeyW) {
                        camera.pos += f * speed * dt
                    }
                    if keys.contains(&KeyCode::KeyS) {
                        camera.pos -= f * speed * dt
                    }
                    if keys.contains(&KeyCode::KeyA) {
                        camera.pos -= r * speed * dt
                    }
                    if keys.contains(&KeyCode::KeyD) {
                        camera.pos += r * speed * dt
                    }
                    if keys.contains(&KeyCode::KeyE) || keys.contains(&KeyCode::Space) {
                        camera.pos.y += speed * dt
                    }
                    if keys.contains(&KeyCode::KeyQ) || keys.contains(&KeyCode::ControlLeft) {
                        camera.pos.y -= speed * dt
                    }
                    if keys.contains(&KeyCode::ArrowLeft) {
                        camera.yaw += dt
                    }
                    if keys.contains(&KeyCode::ArrowRight) {
                        camera.yaw -= dt
                    }
                    if keys.contains(&KeyCode::ArrowUp) {
                        camera.pitch = (camera.pitch + dt).min(1.5)
                    }
                    if keys.contains(&KeyCode::ArrowDown) {
                        camera.pitch = (camera.pitch - dt).max(-1.5)
                    }
                    if streaming {
                        match result_rx.try_recv() {
                            Ok(Ok(mesh)) => {
                                renderer.update_mesh(mesh);
                                streaming = false;
                            }
                            Ok(Err(e)) => {
                                eprintln!("Streaming error: {e}");
                                target.exit();
                                return;
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {}
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                eprintln!("World streaming worker exited");
                                target.exit();
                                return;
                            }
                        }
                    }
                    // `0x77e360` runs for every resident block on every frame and
                    // the window origin follows the camera's block, so re-select
                    // both per frame and rebuild only when the drawn mesh would
                    // differ. The in-flight flag rate-limits the rebuilds.
                    let aim = if streaming {
                        None
                    } else {
                        let (win, levels) = lod_window(
                            width,
                            height,
                            spacing,
                            [camera.pos.x, camera.pos.z],
                            settings.detail.radius,
                        );
                        Some(LodState {
                            window: win,
                            levels: levels.levels,
                        })
                    };
                    let stale = match &aim {
                        Some(aim) => aim != &renderer.lod || renderer.detail != settings.detail,
                        None => false,
                    };
                    if stale {
                        if request_tx.send((camera.pos.x, camera.pos.z, settings.detail)).is_err() {
                            eprintln!("World streaming worker exited");
                            target.exit();
                            return;
                        }
                        streaming = true;
                    }
                    music.update(settings.music_enabled,settings.music_volume,camera.pos.x,camera.pos.z);
                    if zone_check.elapsed()>=std::time::Duration::from_millis(350){
                        zone_check=Instant::now();let zone=ui_zones.as_ref().and_then(|z|z.find(camera.pos.x,camera.pos.z));
                        if (!zone_initialized || zone!=last_zone) && settings.show_zone_entry {if let Some(i)=zone {if let Some(db)=&ui_zones {let name=db.zones[i].name.trim();if !name.is_empty(){zone_banner=Some((name.to_string(),Instant::now()));}}}}
                        zone_initialized=true;last_zone=zone;
                    }
                    if !settings.show_zone_entry {zone_banner=None;}
                    if settings.fullscreen!=fullscreen_applied {window.set_fullscreen(if settings.fullscreen{Some(Fullscreen::Borderless(window.current_monitor()))}else{None});fullscreen_applied=settings.fullscreen;}

                    if settings.whole_world != world_enabled {
                        world_enabled=settings.whole_world;
                        if let Some(c)=world_cancel.take(){c.store(true,std::sync::atomic::Ordering::Relaxed);}
                        world_rx=None;renderer.world_pages.clear();renderer.world_done=false;
                        if world_enabled {
                            let (tx,rx)=std::sync::mpsc::sync_channel(1);let cancel=Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let c=cancel.clone();let root=whole_element.clone();let ornaments=whole_ornaments.clone();
                            std::thread::Builder::new().name("whole-world-objects".into()).spawn(move ||crate::world_objects::build(root,ornaments,tx,c)).expect("world preload thread");
                            world_rx=Some(rx);world_cancel=Some(cancel);world_status=String::from("Loading world objects...");
                        } else {world_status=String::from("Off");}
                    }
                    if let Some(rx)=&world_rx {match rx.try_recv(){
                        Ok(crate::world_objects::Message::Page(page))=>{
                            let processed=page.processed;
                            let uploaded=crate::world_objects::GpuPage::upload(&renderer.device,&renderer.queue,&renderer.layout,&renderer.sampler,&renderer.uniform,&renderer.reflected_uniform,page);
                            renderer.world_pages.push(uploaded);world_status=format!("Loading: {processed}/{} objects, {} GPU chunks",whole_ornaments.len(),renderer.world_pages.len());
                            eprintln!("{world_status}");
                        },
                        Ok(crate::world_objects::Message::Done{processed,skipped,stats})=>{renderer.world_done=true;world_status=format!("Loaded {processed} objects; {skipped} meshes use fallback texture; {} pages; peak decoded-texture residency {} MiB of {} MiB budget, {} evictions, {} dedup alias hits",stats.pages,stats.residency.peak_resident_bytes>>20,stats.residency.budget_bytes>>20,stats.residency.evictions,stats.residency.alias_hits);eprintln!("{world_status}");},
                        Ok(crate::world_objects::Message::Error(e))=>{world_status=format!("World load failed: {e}");eprintln!("{world_status}");},
                        Err(_)=>{}
                    }}
                    camera.far = if settings.whole_world {settings.far.max(32000.)} else {settings.far};
                    camera.time_of_day = settings.time_of_day;
                    camera.tree_wind_strength=settings.tree_wind_strength;camera.grass_wind_strength=settings.grass_wind_strength;
                    camera.regional_tree_lighting=settings.regional_tree_lighting;
                    camera.grass_distance_fade=settings.grass_distance_fade;
                    camera.area_grass_lightmaps=settings.area_grass_lightmaps;
                    camera.water_reflections=settings.water_reflections;
                    camera.atmospheric_fog=settings.atmospheric_fog;
                    camera.celestial_discs=settings.celestial_discs;
                    camera.bloom=settings.bloom;
                    camera.bloom_strength=settings.bloom_strength;
                    camera.bloom_mode=settings.bloom_mode;
                    camera.bloom_tint=settings.bloom_tint;
                    camera.bloom_brightness_fade=settings.bloom_brightness_fade;
                    camera.bloom_opacity_fade=settings.bloom_opacity_fade;
                    camera.sharpen=settings.sharpen;
                    let whole_frame_gamma=crate::display_gamma::enabled(settings.client_gamma, settings.fullscreen);
                    let frame_gamma_setting=settings.gamma_setting;
                    if whole_frame_gamma && display_gamma.is_none() {display_gamma=Some(crate::display_gamma::DisplayGamma::new(&renderer.device,format,config.width,config.height));}
                    if !whole_frame_gamma {display_gamma=None;}
                    // Retail skips the display ramp in windowed mode; in fullscreen it
                    // belongs after bloom, maps and UI, never in the scene-only pass.
                    // `color_chain` makes the mutually exclusive ownership testable.
                    camera.client_gamma=crate::color_chain::scene_chain_ramp(false,whole_frame_gamma);
                    camera.gamma_setting=settings.gamma_setting;
                    camera.terrain_lightmaps=settings.terrain_lightmaps;
                    camera.model_backface_culling=settings.model_backface_culling;
                    camera.underwater_depth=water_depth(&water_areas,camera.pos);
                    let region_tick = renderer.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                    let region_delta = crate::environment::frame_ms(region_tick.saturating_sub(region_tick_ms));
                    region_tick_ms = region_tick;
                    camera.logic_delta_ms = region_delta;
                    let legacy_fog=fog_transition.update_ms(&environments,camera.pos.to_array(),camera.time_of_day,region_delta);
                    let streamed_fog=match (streamed_transition.as_mut(),regional_streaming.as_mut()) {
                        (Some(transition),Some(active))=>Some(transition.update_streamed_ms(&environments,active,camera.pos.to_array(),camera.time_of_day,region_delta)),
                        _=>None,
                    };
                    if settings.streamed_regions && streamed_fog.is_some() {
                        (camera.air_fog,camera.underwater_fog)=streamed_fog.unwrap();
                        camera.regional_light=streamed_transition.as_ref().and_then(|t|t.lighting(camera.time_of_day));
                    } else {
                        (camera.air_fog,camera.underwater_fog)=legacy_fog;
                        camera.regional_light=fog_transition.lighting(camera.time_of_day);
                    }
                    if menu_page==MenuPage::WorldMap && map_original && !original_map_requested {
                        original_map_requested=true;
                        let root=whole_element.clone();let (tx,rx)=std::sync::mpsc::channel();
                        std::thread::spawn(move || {let _=tx.send(crate::navigation::original_map(&root));});
                        original_map_rx=Some(rx);
                    }
                    if let Some(rx)=&original_map_rx {if let Ok(result)=rx.try_recv(){
                        match result { Ok(img)=>{let size=[img.width() as usize,img.height() as usize];let pixels=img.into_raw();let color=egui::ColorImage::from_rgba_unmultiplied(size,&pixels);original_map_id=Some(egui_ctx.load_texture("original world map",color,egui::TextureOptions::LINEAR));},Err(e)=>{eprintln!("Original map: {e}");original_map_error=Some(e);}}
                        original_map_rx=None;
                    }}
                    match surface.get_current_texture() {
                        Ok(frame) => {
                            let surface_view = frame.texture.create_view(&Default::default());
                            let surface_raw = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(surface_format.remove_srgb_suffix()), ..Default::default() });
                            // Composition target: the whole-frame gamma intermediate, else the
                            // present-encode intermediate, else the swapchain itself.
                            let composed_view=if whole_frame_gamma {Some(display_gamma.as_ref().unwrap().view())} else if let Some(encode)=present_encode.as_ref() {Some(encode.view())} else {None};
                            let composed_raw=if whole_frame_gamma {Some(display_gamma.as_ref().unwrap().raw_view())} else if let Some(encode)=present_encode.as_ref() {Some(encode.raw_view())} else {None};
                            let scene_view=composed_view.as_ref().unwrap_or(&surface_view);
                            let scene_raw=composed_raw.as_ref().unwrap_or(&surface_raw);
                            renderer.render(
                                scene_view,
                                scene_raw,
                                camera.matrix(config.width, config.height, camera.far),
                                &camera,
                                config.width,
                                config.height,
                            );
                            // Refresh the top-down view on every presented frame. The old
                            // 350ms timer capped minimap motion to fewer than 3 FPS.
                            if settings.show_minimap {
                                renderer.render_minimap(&map_view,&map_depth,&camera,settings.minimap_range,false);
                            }
                            if menu_page==MenuPage::WorldMap && !map_original && map_refresh.elapsed()>=std::time::Duration::from_secs(2) {
                                renderer.render_minimap(&full_map_view,&full_map_depth,&camera,0.,true);
                                map_refresh=Instant::now();
                            }
                            if menu_page.is_open() || settings.show_minimap || zone_banner.as_ref().is_some_and(|(_,t)|t.elapsed().as_secs_f32()<4.) {
                                let input=egui_state.take_egui_input(&window);
                                let output=egui_ctx.run(input,|ctx| {
                                    if menu_page==MenuPage::Main {
                                        egui::Window::new("PERFECT WORLD EXPLORER  |  ESC MENU")
                                            .default_pos(egui::pos2(16.,16.)).default_size(egui::vec2(380.,270.)).min_size(egui::vec2(280.,220.)).collapsible(false).resizable(true).vscroll(true)
                                            .show(ctx,|ui|{
                                                ui.set_min_height(ui.available_height());
                                                ui.add_space(8.);ui.vertical_centered(|ui|{
                                                    ui.label(egui::RichText::new("WORLD EXPLORER").size(24.).strong());
                                                    ui.label(egui::RichText::new("EXPLORER  •  BUILD 2026.11").small().color(egui::Color32::LIGHT_BLUE));ui.add_space(14.);
                                                    let button_width=ui.available_width().min(320.);
                                                    if ui.add_sized([button_width,42.],egui::Button::new(egui::RichText::new("RESUME EXPLORING").size(17.))).clicked(){menu_page=MenuPage::Closed;}
                                                    ui.add_space(8.);
                                                    if ui.add_sized([button_width,52.],egui::Button::new(egui::RichText::new("SETTINGS").size(22.).strong()).fill(egui::Color32::from_rgb(38,83,116))).clicked(){settings_tab=0;menu_page=MenuPage::Settings;}
                                                    ui.add_space(8.);
                                                    if ui.add_sized([button_width,40.],egui::Button::new(egui::RichText::new("TELEPORT LIST").size(17.))).clicked(){menu_page=MenuPage::Teleports;}
                                                    ui.add_space(8.);
                                                    if ui.add_sized([button_width,40.],egui::Button::new(egui::RichText::new("WORLD MAP  [M]").size(17.))).clicked(){menu_page=MenuPage::WorldMap;map_refresh=Instant::now()-std::time::Duration::from_secs(10);}
                                                    ui.add_space(8.);
                                                    if ui.add_sized([button_width,36.],egui::Button::new("Exit Explorer")).clicked(){if let Err(e)=settings.save(){eprintln!("Settings save: {e}");}target.exit();}
                                                });
                                            });
                                    }
                                    if menu_page==MenuPage::Settings {
                                        egui::Window::new("SETTINGS  |  ESC to go back")
                                            .default_pos(egui::pos2(16.,16.)).collapsible(false).resizable(true)
                                            .default_size(egui::vec2(490.,460.)).min_size(egui::vec2(310.,250.)).max_height(config.height as f32 / window.scale_factor() as f32-35.).vscroll(true)
                                            .show(ctx,|ui| {
                                                ui.horizontal_wrapped(|ui|{
                                                    let tab_width=((ui.available_width()-24.)/4.).clamp(84.,140.);
                                                    for (idx,name) in ["GRAPHICS","WORLD","SOUND","INTERFACE"].iter().enumerate(){
                                                        if ui.add_sized([tab_width,36.],egui::SelectableLabel::new(settings_tab==idx,*name)).clicked(){settings_tab=idx;}
                                                    }
                                                });ui.separator();ui.add_space(4.);
                                                let defaults=ViewSettings::default();
                                                ui.label(egui::RichText::new("Gold tick and label show the default value").small().weak());
                                                match settings_tab {
                                                    0=>{ui.heading("Graphics");ui.checkbox(&mut settings.fullscreen,"Fullscreen (borderless)");ui.checkbox(&mut settings.bloom,"Bloom");ui.checkbox(&mut settings.sharpen,"Client fullscreen sharpen");marked_slider(ui,egui::Slider::new(&mut settings.bloom_strength,0.0..=4.0),"Bloom strength",defaults.bloom_strength,0.,4.,false,settings.bloom);egui::ComboBox::from_label("Bloom mode").selected_text(match settings.bloom_mode {4=>"Client special 4 (mono)",5=>"Client special 5 (low glow)",_=>"Automatic"}).show_ui(ui,|ui| {ui.selectable_value(&mut settings.bloom_mode,0,"Automatic");ui.selectable_value(&mut settings.bloom_mode,4,"Client special 4 (mono)");ui.selectable_value(&mut settings.bloom_mode,5,"Client special 5 (low glow)");});ui.label("Client glow caller tint / opacity (preview)");let mut tint=[((settings.bloom_tint>>16)&255)as f32/255.,((settings.bloom_tint>>8)&255)as f32/255.,(settings.bloom_tint&255)as f32/255.,(settings.bloom_tint>>24)as f32/255.];if ui.color_edit_button_rgba_unmultiplied(&mut tint).changed() {let c=tint.map(|v|(v.clamp(0.,1.)*255.).round()as u32);settings.bloom_tint=(c[3]<<24)|(c[0]<<16)|(c[1]<<8)|c[2];}marked_slider(ui,egui::Slider::new(&mut settings.bloom_brightness_fade,0.0..=1.0),"Glow fade target (1s transition)",0.,0.,1.,false,settings.bloom);marked_slider(ui,egui::Slider::new(&mut settings.bloom_opacity_fade,0.0..=1.0),"Glow opacity fade (preview)",0.,0.,1.,false,settings.bloom);ui.checkbox(&mut settings.client_gamma,"Client gamma (fullscreen only)");marked_slider(ui,egui::Slider::new(&mut settings.gamma_setting,0.0..=100.0),"Gamma",defaults.gamma_setting,0.,100.,false,settings.client_gamma);}
                                                    1=>{ui.heading("World");ui.checkbox(&mut settings.whole_world,"Render whole world (trees, rocks, buildings)");ui.label(&world_status);if let Some(models)=&renderer.models {let (ready,failed,total)=models.status();ui.label(format!("Animated world models: {ready}/{total}; rejected {failed}"));let g=models.gfx_status();let alpha=models.alpha_status();ui.label(format!("FX light producers: {} loaded, {} pending, {} elements, {} failed, {} non-light roots; alpha meshes ordered {}/{} ({} drawable of {} slots, {} failed, {} without bounds)",g.live,g.pending,g.emitters,g.failed_owners+models.gfx_failures().len(),g.non_light_owners,alpha.ordered,alpha.alpha_meshes,alpha.drawable,alpha.slots,alpha.failed,alpha.without_bounds)).on_hover_text(format!("Loader queue: {} frame(s) ticked, one pending root admitted per frame (native 0x5b1a00). Non-light roots loaded cleanly but contain no LightType element, so the sprites/particles/trails inside them are outside this pass.",g.frames));}marked_slider(ui,egui::Slider::new(&mut settings.far,2000.0..=24000.0),"View distance",defaults.far,2000.,24000.,false,true);marked_slider(ui,egui::Slider::new(&mut settings.detail.radius,2..=12),"Terrain block window (blocks)",defaults.detail.radius as f32,2.,12.,false,true);marked_slider(ui,egui::Slider::new(&mut settings.detail.tree_lod_scale,0.25..=4.0),"Tree LOD distance scale",defaults.detail.tree_lod_scale,0.25,4.,false,true);marked_slider(ui,egui::Slider::new(&mut settings.tree_wind_strength,0.0..=1.0),"Tree leaf rocking",defaults.tree_wind_strength,0.,1.,false,true);marked_slider(ui,egui::Slider::new(&mut settings.time_of_day,0.0..=24.0),"Time of day",defaults.time_of_day,0.,24.,false,true);ui.checkbox(&mut settings.water_reflections,"Water reflections");ui.checkbox(&mut settings.celestial_discs,"Client sun and moon (experimental)").on_hover_text("Uses the original textures and recovered sky-clock direction. Disc size, visibility, halo and blend remain approximate.");ui.checkbox(&mut settings.atmospheric_fog,"Client atmospheric fog (limits distant visibility)");ui.checkbox(&mut settings.terrain_lightmaps,"Client terrain lightmaps");ui.checkbox(&mut settings.area_grass_lightmaps,"Grass area-centered lightmaps").on_hover_text("Client area-center selection with clamped tile edges. Disable only to compare the legacy per-vertex selection.");ui.checkbox(&mut settings.regional_tree_lighting,"Experimental regional tree lighting");ui.checkbox(&mut settings.streamed_regions,"Streamed regional lighting/fog (experimental)").on_hover_text("Select environment regions from nearby world blocks using client-like priority. Changes atmospheric and underwater fog; affects trees only when experimental regional tree lighting is enabled. Asynchronous client load order is not yet reproduced.");ui.checkbox(&mut settings.grass_distance_fade,"Client grass distance fade");marked_slider(ui,egui::Slider::new(&mut settings.grass_wind_strength,0.0..=1.0),"Grass wind (experimental)",defaults.grass_wind_strength,0.,1.,false,true);ui.checkbox(&mut settings.model_backface_culling,"Cull building back faces (experimental)");if camera.underwater_depth>0.{ui.label("Underwater effect active");}ui.label("Full world requires substantial GPU memory.");}
                                                    2=>{ui.heading("Sound");ui.checkbox(&mut settings.music_enabled,"Play music from current zone");marked_slider(ui,egui::Slider::new(&mut settings.music_volume,0.0..=1.0),"Music volume",defaults.music_volume,0.,1.,false,true);ui.label(music.status());ui.label("Music follows the client zone playlists; ambient sounds are not yet implemented.");}
                                                    _=>{ui.heading("Interface");ui.checkbox(&mut settings.show_zone_entry,"Show entering zone notification");ui.checkbox(&mut settings.show_minimap,"Show minimap");marked_slider(ui,egui::Slider::new(&mut settings.minimap_range,100.0..=600.0),"Minimap half-width (world units)",defaults.minimap_range,100.,600.,false,settings.show_minimap);ui.separator();ui.heading("Camera controls");marked_slider(ui,egui::Slider::new(&mut settings.camera_speed,10.0..=3000.0).logarithmic(true),"Flight speed",defaults.camera_speed,10.,3000.,true,true);marked_slider(ui,egui::Slider::new(&mut settings.boost,1.0..=10.0),"Shift multiplier",defaults.boost,1.,10.,false,true);marked_slider(ui,egui::Slider::new(&mut settings.sensitivity,0.0005..=0.01).logarithmic(true),"Mouse sensitivity",defaults.sensitivity,0.0005,0.01,true,true);}
                                                }
                                                ui.separator();ui.horizontal(|ui|{if ui.add_sized([135.,32.],egui::Button::new("Back to menu")).clicked(){menu_page=MenuPage::Main;}if ui.button("Restore defaults").clicked(){settings=ViewSettings::default();}});
                                            });
                                    }
                                    if menu_page==MenuPage::Teleports {
                                        egui::Window::new("TELEPORT LIST  |  ESC to go back").default_pos(egui::pos2(16.,16.)).default_size(egui::vec2(540.,570.)).min_size(egui::vec2(340.,280.)).max_height(config.height as f32/window.scale_factor() as f32-35.).collapsible(false).resizable(true).show(ctx,|ui|{
                                            ui.heading("Explore destinations");ui.label(egui::RichText::new("Locations and coordinates from the client’s world_targets.txt").small().weak());ui.add_space(6.);
                                            ui.add(egui::TextEdit::singleline(&mut teleport_query).hint_text("Search locations or ID...").desired_width(f32::INFINITY));
                                            match &destinations {Ok(list)=>{
                                                let q=teleport_query.trim().to_lowercase();let filtered=list.iter().filter(|p|q.is_empty() || p.name.to_lowercase().contains(&q) || p.id.to_string().contains(&q)).collect::<Vec<_>>();
                                                ui.label(format!("{} of {} destinations",filtered.len(),list.len()));ui.separator();
                                                egui::ScrollArea::vertical().max_height((ui.available_height()-42.).max(100.)).show(ui,|ui|{for place in filtered {
                                                    ui.horizontal(|ui| {ui.vertical(|ui|{ui.label(egui::RichText::new(&place.name).strong());ui.label(egui::RichText::new(format!("ID {}  •  x {:.0}   y {:.0}   z {:.0}",place.id,place.position[0],place.position[1],place.position[2])).small().weak());});
                                                    if ui.button("Teleport").clicked(){camera.pos=Vec3::from_array(place.position)+Vec3::Y*3.;menu_page=MenuPage::Closed;keys.clear();}});ui.separator();
                                                }});
                                            },Err(e)=>{ui.colored_label(egui::Color32::LIGHT_RED,format!("Could not load destinations: {e}"));}}
                                            if ui.button("Back to menu").clicked(){menu_page=MenuPage::Main;}
                                        });
                                    }
                                    if menu_page==MenuPage::WorldMap {
                                        let scale=window.scale_factor() as f32;let screen_w=config.width as f32/scale;let screen_h=config.height as f32/scale;
                                        egui::Window::new("WORLD MAP  |  M or ESC to close").default_pos(egui::pos2(16.,16.)).default_size(egui::vec2((screen_w-32.).min(760.),(screen_h-32.).min(900.))).min_size(egui::vec2(330.,300.)).max_height(screen_h-35.).collapsible(false).resizable(true).show(ctx,|ui|{
                                            ui.label(egui::RichText::new("North: up  |  East: right  |  Mouse wheel: zoom").small().strong());
                                            if !map_original && !renderer.world_done {ui.horizontal_wrapped(|ui|{ui.label(egui::RichText::new("Distant scenery: ").small().weak());if settings.whole_world {ui.label(egui::RichText::new(&world_status).small().weak());}else if ui.button("Load whole world scenery").clicked(){settings.whole_world=true;}});}
                                            let remaining=(ui.available_height()-78.).max(100.);let visible_width=ui.available_width();let base_width=visible_width.min(remaining*8./11.).max(120.);let image_size=egui::vec2(base_width*map_zoom,base_width*map_zoom*11./8.);
                                            let texture=if map_original {original_map_id.as_ref().map(|t|t.id())} else {Some(full_map_id)};
                                            let mut zoom_anchor=None;
                                            let scroll=egui::ScrollArea::both().max_height(remaining).auto_shrink([false,false]).show(ui,|ui|{
                                                if let Some(id)=texture {let response=ui.add(egui::Image::new((id,image_size)).sense(egui::Sense::click()));let rect=response.rect;
                                                    if response.hovered() {let delta=ui.input(|i|i.raw_scroll_delta.y);if delta!=0. {if let Some(pointer)=ui.input(|i|i.pointer.hover_pos()){let next=(map_zoom*(delta*0.005).exp()).clamp(1.,4.);zoom_anchor=Some((pointer,rect,next/map_zoom));map_zoom=next;}}ui.input_mut(|i|{i.raw_scroll_delta=egui::Vec2::ZERO;i.smooth_scroll_delta=egui::Vec2::ZERO;});}
                                                    let uv=map_space.to_uv(camera.pos.x,camera.pos.z);if (0.0..=1.0).contains(&uv[0])&&(0.0..=1.0).contains(&uv[1]) {let p=rect.left_top()+egui::vec2(uv[0]*rect.width(),uv[1]*rect.height());ui.painter().circle_filled(p,5.,egui::Color32::from_rgb(255,210,60));ui.painter().circle_stroke(p,7.,egui::Stroke::new(2.0f32,egui::Color32::BLACK));}
                                                    if map_click_teleport && response.clicked(){if let Some(pos)=response.interact_pointer_pos(){let uv=(pos-rect.left_top())/rect.size();let [x,z]=map_space.from_uv(uv.x,uv.y);let surface=water_areas.iter().filter(|a|a.contains_xz(x,z)).fold(map_space.ground(x,z),|h,a|h.max(a.height));camera.pos=Vec3::new(x,surface+30.,z);menu_page=MenuPage::Closed;keys.clear();}}
                                                }else {if let Some(e)=&original_map_error{ui.colored_label(egui::Color32::LIGHT_RED,format!("Original map unavailable: {e}"));}else{ui.label("Loading original client map tiles...");}ui.allocate_space(egui::vec2(80.,80.));}
                                            });
                                            if let Some((pointer,rect,ratio))=zoom_anchor {let mut state=scroll.state;let local=pointer-rect.min;state.offset=anchored_zoom_offset(state.offset,local,ratio);state.store(ctx,scroll.id);ctx.request_repaint();}
                                            ui.separator();ui.horizontal_wrapped(|ui|{ui.label("Style:");if ui.selectable_label(!map_original,"Realistic").clicked(){map_original=false;map_refresh=Instant::now()-std::time::Duration::from_secs(10);}if ui.selectable_label(map_original,"Original").clicked(){map_original=true;}ui.separator();ui.checkbox(&mut map_click_teleport,"Click to teleport");marked_slider(ui,egui::Slider::new(&mut map_zoom,1.0..=4.0),"Zoom",1.,1.,4.,false,true);});
                                        });
                                    }
                                    if settings.show_minimap {egui::Area::new(egui::Id::new("minimap-hud")).anchor(egui::Align2::RIGHT_TOP,egui::vec2(-16.,16.)).interactable(false).show(ctx,|ui|{egui::Frame::none().fill(egui::Color32::from_black_alpha(175)).inner_margin(5.).show(ui,|ui|{let response=ui.add(egui::Image::new((map_id,egui::vec2(200.,200.))));let center=response.rect.center();let dir=egui::vec2(camera.yaw.sin(),camera.yaw.cos());let side=egui::vec2(-dir.y,dir.x);let points=vec![center+dir*10.,center-dir*7.+side*5.,center-dir*7.-side*5.];ui.painter().add(egui::Shape::convex_polygon(points,egui::Color32::from_rgb(255,225,130),egui::Stroke::new(1.0f32,egui::Color32::BLACK)));ui.painter().text(response.rect.left_top()+egui::vec2(5.,5.),egui::Align2::LEFT_TOP,"N",egui::FontId::proportional(14.),egui::Color32::WHITE);});});}
                                    if settings.show_zone_entry {if let Some((name,started))=&zone_banner {let elapsed=started.elapsed().as_secs_f32();if elapsed<4. {let alpha=(elapsed.min(4.-elapsed)*2.).clamp(0.,1.);egui::Area::new(egui::Id::new("zone-entry")).anchor(egui::Align2::CENTER_TOP,egui::vec2(0.,65.)).interactable(false).show(ctx,|ui|{ui.label(egui::RichText::new(format!("Entering {name}")).size(24.).color(egui::Color32::from_white_alpha((alpha*255.) as u8)));});}}}
                                });
                                egui_state.handle_platform_output(&window,output.platform_output);
                                let jobs=egui_ctx.tessellate(output.shapes,output.pixels_per_point);
                                let screen=egui_wgpu::ScreenDescriptor{size_in_pixels:[config.width,config.height],pixels_per_point:output.pixels_per_point};
                                for (id,delta) in &output.textures_delta.set {egui_renderer.update_texture(&renderer.device,&renderer.queue,*id,delta);}
                                let mut encoder=renderer.device.create_command_encoder(&Default::default());
                                let cmd=egui_renderer.update_buffers(&renderer.device,&renderer.queue,&mut encoder,&jobs,&screen);
                                {let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("settings menu"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:scene_view,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Load,store:wgpu::StoreOp::Store}})],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None});egui_renderer.render(&mut pass,&jobs,&screen);}
                                renderer.queue.submit(cmd.into_iter().chain(std::iter::once(encoder.finish())));
                                for id in &output.textures_delta.free {egui_renderer.free_texture(id);}
                            }
                            // Exactly one ramp stage, then exactly one output encode.
                            if let Some(encode) = present_encode.as_ref() {
                                // Ramp into the sRGB intermediate, then copy it once.
                                let ramp_target = encode.view();
                                if let Some(gamma) = display_gamma.as_ref() {
                                    gamma.present(
                                        &renderer.device,
                                        &renderer.queue,
                                        &ramp_target,
                                        frame_gamma_setting,
                                    );
                                }
                                encode.present(&renderer.device, &renderer.queue, &surface_view);
                            } else if let Some(gamma) = display_gamma.as_ref() {
                                gamma.present(
                                    &renderer.device,
                                    &renderer.queue,
                                    &surface_view,
                                    frame_gamma_setting,
                                );
                            }
                            frame.present();
                            if software_adapter { renderer.device.poll(wgpu::Maintain::Wait); }
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            surface.configure(&renderer.device, &config)
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => target.exit(),
                        Err(e) => eprintln!("surface: {e}"),
                    }
                }
                _ => {}
            }
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } if captured => {
                if std::env::var_os("PW_INPUT_DEBUG").is_some() {
                    eprintln!("Mouse motion: {:?}", delta);
                }
                camera.yaw -= delta.0 as f32 * settings.sensitivity;
                camera.pitch = (camera.pitch - delta.1 as f32 * settings.sensitivity).clamp(-1.5, 1.5);
            }
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        })
        .map_err(|e| e.to_string())
}
/// Render one frame without a window. Useful for validation on headless systems.
pub fn preview(
    mut t: Terrain,
    path: &std::path::Path,
    camera: Option<[f32; 5]>,
) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::all()),
        ..Default::default()
    });
    let (_adapter, device, queue) = pollster::block_on(adapter(&instance, None))?;
    let mut cam = if let Some([x, y, z, yaw, pitch]) = camera {
        if ![x, y, z, yaw, pitch].iter().all(|x| x.is_finite()) {
            return Err("non-finite camera".into());
        }
        Camera {
            pos: Vec3::new(x, y, z),
            yaw,
            pitch,
            far: 16000.,
            time_of_day: 12.,
            water_reflections: true,
            atmospheric_fog: false,
            celestial_discs: false,
            bloom: false,
            sharpen: false,
            bloom_strength: 1.,
            bloom_mode: 0,
            bloom_tint: 0xffffffff,
            bloom_brightness_fade: 0.,
            bloom_opacity_fade: 0.,
            client_gamma: false,
            gamma_setting: 41.,
            underwater_depth: 0.,
            underwater_fog: Default::default(),
            air_fog: Default::default(),
            terrain_lightmaps: true,
            model_backface_culling: true,
            tree_wind_strength: 0.2,
            grass_wind_strength: 0.,
            regional_tree_lighting: false,
            grass_distance_fade: true,
            area_grass_lightmaps: true,
            regional_light: None,
            logic_delta_ms: 0,
        }
    } else {
        Camera::new(&t)
    };
    cam.underwater_depth = water_depth(&t.water, cam.pos);
    if std::env::var_os("PW_TEST_NO_LIGHTMAPS").is_some() {
        cam.terrain_lightmaps = false;
    }
    cam.bloom = std::env::var_os("PW_PREVIEW_BLOOM").is_some();
    cam.bloom_tint = std::env::var("PW_PREVIEW_BLOOM_TINT")
        .ok()
        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0xffffffff);
    cam.bloom_brightness_fade = std::env::var("PW_PREVIEW_BLOOM_BRIGHTNESS_FADE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.);
    cam.bloom_opacity_fade = std::env::var("PW_PREVIEW_BLOOM_OPACITY_FADE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.);
    cam.bloom_mode = std::env::var("PW_PREVIEW_BLOOM_MODE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|m| matches!(m, 4 | 5))
        .unwrap_or(0);
    cam.bloom_strength = std::env::var("PW_PREVIEW_BLOOM_STRENGTH")
        .ok()
        .and_then(|x| x.parse::<f32>().ok())
        .unwrap_or(1.)
        .clamp(0., 4.);
    if std::env::var_os("PW_PREVIEW_NO_REFLECTION").is_some() {
        cam.water_reflections = false;
    }
    if let Ok(hour) = std::env::var("PW_PREVIEW_HOUR") {
        if let Ok(h) = hour.parse::<f32>() {
            cam.time_of_day = h.clamp(0., 24.);
        }
    }
    cam.underwater_fog = t
        .environments
        .underwater(cam.pos.to_array(), cam.time_of_day);
    let mut residency = crate::terrain_residency::Residency::default();
    let mesh = build(
        &mut t,
        cam.pos.x,
        cam.pos.z,
        DetailSettings::default(),
        &mut residency,
    )?;
    let (width, height) = (960u32, 720u32);
    let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::new(device, queue, fmt, width, height, mesh, &mut t)?;
    if std::env::var_os("PW_PREVIEW_WORLD_TEST").is_some() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let root = t.element.clone();
        let ornaments = t.objects.ornaments.clone();
        std::thread::spawn(move || {
            crate::world_objects::build(
                root,
                ornaments,
                tx,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
        });
        for msg in rx {
            match msg {
                crate::world_objects::Message::Page(page) => {
                    eprintln!(
                        "Preview uploaded world chunk: {} vertices",
                        page.vertices.len()
                    );
                    renderer
                        .world_pages
                        .push(crate::world_objects::GpuPage::upload(
                            &renderer.device,
                            &renderer.queue,
                            &renderer.layout,
                            &renderer.sampler,
                            &renderer.uniform,
                            &renderer.reflected_uniform,
                            page,
                        ));
                }
                crate::world_objects::Message::Done {
                    processed,
                    skipped,
                    stats,
                } => {
                    eprintln!(
                        "Preview world preload: {processed} objects, {skipped} fallback meshes, {} pages, peak residency {} MiB / {} MiB budget, {} evictions",
                        stats.pages,
                        stats.residency.peak_resident_bytes >> 20,
                        stats.residency.budget_bytes >> 20,
                        stats.residency.evictions
                    );
                    renderer.world_done = true;
                    break;
                }
                crate::world_objects::Message::Error(e) => return Err(e),
            }
        }
    }
    let alternate_format = [fmt.remove_srgb_suffix()];
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless world image"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: fmt,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: if fmt.is_srgb() {
            &alternate_format
        } else {
            &[]
        },
    });
    renderer.render(
        &target.create_view(&Default::default()),
        &target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(fmt.remove_srgb_suffix()),
            ..Default::default()
        }),
        cam.matrix(width, height, cam.far),
        &cam,
        width,
        height,
    );
    if std::env::var_os("PW_PREVIEW_MINIMAP").is_some() {
        let d = depth(&renderer.device, width, height);
        renderer.render_minimap(
            &target.create_view(&Default::default()),
            &d.create_view(&Default::default()),
            &cam,
            256.,
            false,
        );
    }
    let row = width * 4;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image readback"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = renderer.device.create_command_encoder(&Default::default());
    e.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit(Some(e.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    renderer.device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let bytes = buffer.slice(..).get_mapped_range();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut out = encoder.write_header().map_err(|e| e.to_string())?;
    out.write_image_data(&bytes).map_err(|e| e.to_string())?;
    out.finish().map_err(|e| e.to_string())?;
    drop(bytes);
    buffer.unmap();
    Ok(())
}

#[cfg(test)]
mod water_batch_tests {
    use super::*;
    fn batch(bounds: [f32; 4], height: f32) -> WaterBatch {
        WaterBatch {
            simple: false,
            start: 0,
            end: 6,
            height,
            bounds,
        }
    }
    #[test]
    fn escape_navigates_menu_pages() {
        assert_eq!(MenuPage::Closed.escape(), MenuPage::Main);
        assert_eq!(MenuPage::Settings.escape(), MenuPage::Main);
        assert_eq!(MenuPage::Main.escape(), MenuPage::Closed);
    }
    #[test]
    fn north_camera_east_is_right_and_turning_left_points_west() {
        // Real client targets: Raging Tide has greater X than Archosaur.
        let pos = Vec3::new(1470.490, 219.608, 1180.864);
        let yaw = std::f32::consts::PI;
        let direction = |yaw: f32| Vec3::new(yaw.sin(), 0., -yaw.cos());
        let matrix = |yaw: f32| {
            Mat4::from_scale(Vec3::new(-1., 1., 1.))
                * Mat4::perspective_rh(70f32.to_radians(), 1., 32000., 1.)
                * Mat4::look_to_rh(pos, direction(yaw), Vec3::Y)
        };
        let north = pos + Vec3::Z * 100.;
        let east = north + Vec3::X * 30.;
        assert!(matrix(yaw).project_point3(east).x > matrix(yaw).project_point3(north).x);
        let left = direction(yaw + 0.25);
        let right = direction(yaw - 0.25);
        assert!(left.x < 0. && right.x > 0.);
        let screen = |d: Vec3| egui::vec2(d.x, -d.z);
        assert!(screen(left).x < 0. && screen(right).x > 0.);
        let strafe = Vec3::new(-yaw.cos(), 0., -yaw.sin());
        assert!(strafe.x > 0.);
    }
    #[test]
    fn wheel_zoom_keeps_cursor_world_point_stationary() {
        let offset = egui::vec2(90., 210.);
        let cursor = egui::vec2(140., 110.);
        // local image point is the cursor position plus the current offset.
        let local = offset + cursor;
        let scale = 1.5;
        let new = anchored_zoom_offset(offset, local, scale);
        assert!(((local * scale - new) - cursor).length() < 1e-5);
        let restored = anchored_zoom_offset(new, local * scale, 1. / scale);
        assert!((restored - offset).length() < 1e-4);
    }
    #[test]
    fn projection_mirroring_preserves_reversed_depth() {
        let normal = Mat4::perspective_rh(70f32.to_radians(), 1., 32000., 1.);
        let mirrored = Mat4::from_scale(Vec3::new(-1., 1., 1.)) * normal;
        for p in [Vec3::new(1., 2., -20.), Vec3::new(-3., 0., -1000.)] {
            let a = normal.project_point3(p);
            let b = mirrored.project_point3(p);
            assert!(
                (a.x + b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6 && (a.z - b.z).abs() < 1e-6
            );
        }
    }
    #[test]
    fn minimap_is_north_up_and_east_right() {
        let m = minimap_matrix(120., -70., 256.);
        let center = m.project_point3(Vec3::new(120., 0., -70.));
        let east = m.project_point3(Vec3::new(220., 0., -70.));
        let north = m.project_point3(Vec3::new(120., 0., 30.));
        assert!(center.x.abs() < 0.0001 && center.y.abs() < 0.0001);
        assert!(east.x > center.x && north.y > center.y);
    }
    #[test]
    fn full_map_projection_agrees_with_client_tile_coordinates() {
        let width = 8192.;
        let height = 11264.;
        let m = Mat4::orthographic_rh(
            width * 0.5,
            -width * 0.5,
            -height * 0.5,
            height * 0.5,
            20000.,
            1.,
        ) * Mat4::look_to_rh(Vec3::new(0., 10000., 0.), Vec3::NEG_Y, Vec3::Z);
        let nav = crate::navigation::MapSpace {
            width: 4096,
            height: 5632,
            spacing: 2.,
            heights: std::sync::Arc::from(vec![0.; 4]),
        };
        for (x, z) in [
            (-2825.774, 3092.210),
            (1470.490, 1180.864),
            (2681.930, -4268.359),
        ] {
            let clip = m.project_point3(Vec3::new(x, 0., z));
            let uv = nav.to_uv(x, z);
            assert!((clip.x * 0.5 + 0.5 - uv[0]).abs() < 0.00001);
            assert!((0.5 - clip.y * 0.5 - uv[1]).abs() < 0.00001);
        }
    }
    #[test]
    fn reversed_depth_keeps_far_surface_precision() {
        let projection = Mat4::perspective_rh(70f32.to_radians(), 1., 32000., 1.);
        let depth = |z: f32| projection.project_point3(Vec3::new(0., 0., -z)).z;
        assert!((depth(1.) - 1.).abs() < 1e-6);
        assert!(depth(32000.).abs() < 1e-6);
        assert!(depth(5000.) > depth(5000.01));
        let forward = Mat4::perspective_rh(70f32.to_radians(), 1., 1., 32000.);
        let d = |z: f32| forward.project_point3(Vec3::new(0., 0., -z)).z;
        assert!(
            (0..1000).any(|i| {
                let z = 5000. + i as f32 * 0.01;
                d(z) == d(z + 0.01) && depth(z) > depth(z + 0.01)
            }),
            "reversed depth should distinguish surfaces lost to forward depth precision"
        );
    }
    #[test]
    fn conservative_water_frustum() {
        let m = Mat4::perspective_rh(70f32.to_radians(), 1., 1., 100.)
            * Mat4::look_to_rh(
                Vec3::new(0., 10., 0.),
                Vec3::new(0., -0.3, -1.).normalize(),
                Vec3::Y,
            );
        assert!(batch([-3., 3., -30., -10.], 0.).visible(m));
        assert!(!batch([500., 510., -30., -10.], 0.).visible(m));
        assert!(!batch([-3., 3., 10., 20.], 0.).visible(m));
    }
    #[test]
    fn distance_to_nearest_surface_point() {
        assert_eq!(
            batch([-2., 2., -3., 3.], 4.).distance_squared(Vec3::new(5., 4., 0.)),
            9.
        );
    }
}
