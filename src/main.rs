#![cfg_attr(windows, windows_subsystem = "windows")]
use flate2::read::ZlibDecoder;
use std::{
    env, fs,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

fn u32at(b: &[u8], i: usize) -> Result<u32, String> {
    let s = b.get(i..i + 4).ok_or("truncated data")?;
    Ok(u32::from_le_bytes(s.try_into().unwrap()))
}
fn f32at(b: &[u8], i: usize) -> Result<f32, String> {
    Ok(f32::from_bits(u32at(b, i)?))
}
fn read_string(b: &[u8], pos: &mut usize) -> Result<String, String> {
    let n = u32at(b, *pos)? as usize;
    *pos += 4;
    if n > 4096 {
        return Err("invalid string length".into());
    }
    let s = b.get(*pos..*pos + n).ok_or("truncated string")?;
    *pos += n;
    String::from_utf8(s.to_vec()).map_err(|_| "non-UTF8 subterrain name".into())
}
mod angelica_pck;
mod animation;
mod bmd;
mod bmd_baked;
#[cfg(windows)]
mod client_detect;
mod client_sun;
#[allow(dead_code)]
mod client_weather;
mod cloud_gpu;
#[allow(dead_code)]
mod cloud_surface;
mod color_chain;
mod display_gamma;
mod dxt;
mod ecmodel;
mod environment;
mod far_trees_gpu;
mod fog_matrix;
mod foliage_mips;
#[allow(dead_code)]
mod gfx_catalog;
mod gfx_color_track;
mod gfx_decal;
mod gfx_decal_gpu;
/// The wgpu receiver for the animated roots. Its diagnostic accessors are part
/// of the module's contract even while only its own tests call them.
#[allow(dead_code)]
mod gfx_effect_gpu;
/// The full `.gfx` grammar: every field the container can carry is parsed and
/// documented, and the runtime player consumes only the visual subset.
#[allow(dead_code)]
mod gfx_file;
#[allow(dead_code)]
mod gfx_light;
mod gfx_owner;
/// The runtime that turns a parsed root into this frame's quads. `reset`,
/// `textures` and `time_ms` are public contract for callers that are not wired
/// up yet, so the module carries the same allowance `gfx_file` does.
#[allow(dead_code)]
mod gfx_player;
mod gpu;
mod grass;
mod grass_instances;
mod grass_mips;
mod grass_wind;
#[cfg(windows)]
mod launcher_ui;
mod light_consumers;
mod material;
mod model_asset;
mod model_attachment;
mod model_dds;
mod model_draw;
#[cfg(test)]
#[path = "../tools/fixtures/model_draw.rs"]
mod model_draw_fixture;
mod model_draw_gpu;
mod model_light;
mod model_material;
mod model_playback;
mod model_queue;
mod model_receiver_class;
mod model_receiver_gpu;
mod model_texture_gpu;
mod modules;
mod music;
mod navigation;
mod objects;
mod pck;
mod post_gpu;
#[cfg(test)]
mod post_reference;
mod present_encode;
mod render_scale;
#[allow(dead_code)]
mod scene;
#[allow(dead_code)]
mod scene_lights;
mod scenery_visibility;
mod shore_gpu;
mod simple_water_gpu;
mod skeleton;
mod skeleton_pose;
mod skin;
mod skin_pose;
mod sky_celestial;
mod sky_config;
mod sky_geometry;
mod sky_gpu;
mod sky_time;
mod world_audio;
// Accessors that only the Phase 5 diagnostic and the future weather wiring use
// are kept public and unreferenced here, like `client_sun`.
#[allow(dead_code)]
mod sky_transition;
mod terrain_block;
mod terrain_cutoff;
mod terrain_gpu;
mod terrain_lightmaps;
#[allow(dead_code)]
mod terrain_lod;
mod terrain_residency;
mod terrain_sampling;
#[allow(dead_code)]
mod terrain_shading;
mod terrain_store;
mod terrain_upload;
mod texture_atlas;
mod texture_resolution;
mod tree_lod;
#[cfg(windows)]
mod tree_source;
#[allow(dead_code)]
mod tree_wind;
mod trees;
mod vegetation_material;
mod vegetation_textures;
mod view_settings;
mod water;
mod water_gpu;
mod weather_gpu;
#[cfg(windows)]
mod windows_setup;
mod world_models;
mod world_models_gpu;
mod world_objects;
struct Terrain {
    element: PathBuf,
    /// Cells per T2BK block edge (`world.trn2` header dword @20).
    grid: usize,
    width: usize,
    height: usize,
    heights: terrain_store::Heights,
    normals: terrain_store::Normals,
    grid_spacing: f32,
    materials: material::Materials,
    subterrain_names: Vec<String>,
    objects: objects::Objects,
    water: Vec<water::WaterArea>,
    environments: environment::Environments,
    scene_lights: std::sync::Arc<scene_lights::SceneLights>,
    /// Retained ECWLD so the renderer can own its effect light producers.
    scene: std::sync::Arc<scene::Scene>,
    /// Native `CECSceneBlock` regional load/evict bits for the ornament class
    /// (`0x424020`). Owned here because `Terrain` lives on the streaming worker.
    scenery_residency: scenery_visibility::RegionResidency,
    terrain_material_window: Option<Option<[usize; 4]>>,
    models: world_models::Assets,
    water_normals: Vec<Vec<image::RgbaImage>>,
    water_simple_frames: Vec<Vec<image::RgbaImage>>,
    trees: trees::Trees,
    grass: grass::Grass,
    /// Parsed native world cloud grid, if this map ships `world.cld` loose.
    cloud_surface: Option<std::sync::Arc<cloud_surface::CloudSurface>>,
}
fn terrain(root: &Path) -> Result<Terrain, String> {
    let dir = if root.join("element/maps/world/world.trn2").is_file() {
        root.join("element/maps/world")
    } else if root.join("maps/world/world.trn2").is_file() {
        root.join("maps/world")
    } else if root.join("world.trn2").is_file() {
        root.to_path_buf()
    } else {
        return Err("expected client root, element folder, or maps/world directory".into());
    };
    let t = fs::read(dir.join("world.trn2")).map_err(|e| e.to_string())?;
    if u32at(&t, 0)? != 0x54524e32 || u32at(&t, 4)? > 1 {
        return Err("unsupported TRN2".into());
    }
    let grid = u32at(&t, 20)? as usize;
    let grid_spacing = f32at(&t, 68)?;
    if !grid_spacing.is_finite() || grid_spacing <= 0.0 {
        return Err("invalid terrain grid spacing".into());
    }
    let rows = u32at(&t, 28)? as usize;
    let cols = u32at(&t, 32)? as usize;
    let count = u32at(&t, 52)? as usize;
    let subgrid = u32at(&t, 64)? as usize;
    let loader = u32at(&t, 76)?;
    let loff = u32at(&t, 80)? as usize;
    if loader != 1
        || grid == 0
        || grid > 256
        || subgrid == 0
        || subgrid % grid != 0
        || rows == 0
        || cols == 0
        || rows > 4096
        || cols > 4096
        || count > 10000
    {
        return Err("unsupported or invalid world terrain header".into());
    }
    let per = subgrid / grid;
    if rows % per != 0 || cols % per != 0 || rows / per * (cols / per) != count {
        return Err("inconsistent subterrain dimensions".into());
    }
    let w = cols.checked_mul(grid).ok_or("size overflow")?;
    let h = rows.checked_mul(grid).ok_or("size overflow")?;
    if w * h > 100_000_000 {
        return Err("terrain too large".into());
    }
    let mut pos = loff + 4;
    let mut names = Vec::new();
    for _ in 0..count {
        names.push(read_string(&t, &mut pos)?)
    }
    let cache_root = std::env::var_os("PW_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cache"))
        .join("terrain-v1");
    let mut terrain_pages = terrain_store::Builder::new(&cache_root, w, h, grid)?;
    // Colour sections are validated per block below. Neither current CPU nor
    // GPU renderer consumes them; do not allocate a second world-sized RGBA grid.
    let sin_table: Vec<f32> = (0..256)
        .map(|i| (i as f32 * std::f32::consts::TAU / 256.).sin())
        .collect();
    let cos_table: Vec<f32> = (0..256)
        .map(|i| (i as f32 * std::f32::consts::TAU / 256.).cos())
        .collect();
    let subs_x = cols / per;
    for (si, name) in names.iter().enumerate() {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("unsafe subterrain path".into());
        }
        let data =
            fs::read(dir.join(format!("{name}.t2bk"))).map_err(|e| format!("{name}: {e}"))?;
        if u32at(&data, 0)? != 0x54424b42 {
            return Err(format!("{name}: invalid T2BK"));
        };
        let v = u32at(&data, 4)?;
        if !(2..=6).contains(&v) {
            return Err(format!("{name}: unsupported version {v}"));
        }
        let n = u32at(&data, 8)? as usize;
        if n < per * per {
            return Err(format!("{name}: missing blocks"));
        };
        let flags = if v < 5 {
            if data[12] != 0 {
                0x15
            } else {
                0
            }
        } else {
            data[12]
        };
        // Validate the whole offset table and section layout before reading any
        // heights: this is what catches a flags bit the reader does not implement,
        // and it is what found the missing per-vertex colour section.
        let block_layout =
            terrain_block::layout(&data, grid).map_err(|e| format!("{name}: {e}"))?;
        for bi in 0..per * per {
            let off = u32at(&data, 16 + bi * 4)? as usize;
            let hs = u32at(&data, off + 16)? as usize;
            let ns = u32at(&data, off + 20)? as usize;
            let hdr = if v >= 6 {
                36
            } else if v >= 4 {
                32
            } else if v >= 3 {
                28
            } else {
                24
            };
            let raw = data
                .get(off + hdr..off + hdr + hs)
                .ok_or("truncated height data")?;
            let mut decoded = Vec::new();
            if flags & 4 != 0 {
                let mut d = ZlibDecoder::new(raw);
                d.by_ref()
                    .take(((grid + 1) * (grid + 1) * 4 + 64) as u64)
                    .read_to_end(&mut decoded)
                    .map_err(|e| format!("{name} block {bi}: {e}"))?;
            } else {
                decoded.extend_from_slice(raw)
            }
            let nv = (grid + 1) * (grid + 1);
            let mut values = Vec::with_capacity(nv);
            if flags & 8 != 0 {
                let min = f32at(&decoded, 4)?;
                let max = f32at(&decoded, 8)?;
                if u32at(&decoded, 12)? as usize != nv {
                    return Err("bad incremental height count".into());
                }
                let mut acc = 0i32;
                for j in 0..nv {
                    let p = 16 + j * 2;
                    let s = decoded.get(p..p + 2).ok_or("truncated deltas")?;
                    acc += i16::from_le_bytes([s[0], s[1]]) as i32;
                    values.push(min + acc as f32 * (max - min) / 32767.0);
                }
            } else {
                if decoded.len() != nv * 4 {
                    return Err(format!(
                        "{name} block {bi}: bad height size {} expected {}",
                        decoded.len(),
                        nv * 4
                    ));
                }
                for j in 0..nv {
                    values.push(f32at(&decoded, j * 4)?)
                }
            }
            let noff = off
                .checked_add(hdr)
                .and_then(|x| x.checked_add(hs))
                .ok_or("normal offset overflow")?;
            let normal_raw = data
                .get(noff..noff.checked_add(ns).ok_or("normal size overflow")?)
                .ok_or("truncated normal data")?;
            let mut normal_data = Vec::new();
            if flags & 0x10 != 0 {
                ZlibDecoder::new(normal_raw)
                    .take((nv * 12 + 64) as u64)
                    .read_to_end(&mut normal_data)
                    .map_err(|e| format!("{name} block {bi} normals: {e}"))?;
            } else {
                normal_data.extend_from_slice(normal_raw)
            }
            let mut block_normals = Vec::with_capacity(nv);
            if flags & 0x20 != 0 {
                let base = if u32at(&normal_data, 4).ok() == Some(nv as u32) {
                    8
                } else if u32at(&normal_data, 1).ok() == Some(nv as u32) {
                    5
                } else {
                    return Err(format!("{name} block {bi}: invalid normal header"));
                };
                for j in 0..nv {
                    let az = *normal_data.get(base + 2 * j).ok_or("truncated normals")? as usize;
                    let el = *normal_data
                        .get(base + 2 * j + 1)
                        .ok_or("truncated normals")? as usize;
                    let se = sin_table[el];
                    block_normals.push([
                        (se * cos_table[az] * 127.) as i8,
                        (cos_table[el] * 127.) as i8,
                        (se * sin_table[az] * 127.) as i8,
                    ]);
                }
            } else {
                for j in 0..nv {
                    let mut n = [0i8; 3];
                    for k in 0..3 {
                        let v = f32at(&normal_data, j * 12 + k * 4)?;
                        if !v.is_finite() {
                            return Err("nonfinite normal".into());
                        }
                        n[k] = (v.clamp(-1., 1.) * 127.) as i8;
                    }
                    block_normals.push(n);
                }
            }
            // Third section: per-vertex RGBA, present when flag bit 0x1 is set.
            if let Some((cs, ce)) = block_layout.blocks[bi].colour {
                let raw = data.get(cs..ce).ok_or("truncated vertex colour data")?;
                let mut decoded_colour = Vec::new();
                ZlibDecoder::new(raw)
                    .take((nv * 4 + 64) as u64)
                    .read_to_end(&mut decoded_colour)
                    .map_err(|e| format!("{name} block {bi} vertex colour: {e}"))?;
                if decoded_colour.len() != nv * 4 {
                    return Err(format!(
                        "{name} block {bi}: vertex colour is {} bytes, expected {}",
                        decoded_colour.len(),
                        nv * 4
                    ));
                }
            }
            let br = (si / subs_x) * per + bi / per;
            let bc = (si % subs_x) * per + bi % per;
            let mut heights = Vec::with_capacity(grid * grid);
            let mut normals = Vec::with_capacity(grid * grid);
            for y in 0..grid {
                for x in 0..grid {
                    let val = values[y * (grid + 1) + x];
                    if !val.is_finite() {
                        return Err("nonfinite height".into());
                    }
                    heights.push(val);
                    normals.push(block_normals[y * (grid + 1) + x]);
                }
            }
            terrain_pages.block(bc, br, &heights, &normals)?;
        }
        eprintln!("Loaded terrain tile {}/{}: {}", si + 1, count, name);
        #[cfg(windows)]
        windows_setup::loading_progress(format!("Loading terrain tiles: {} / {}", si + 1, count));
    }
    #[cfg(windows)]
    windows_setup::loading_progress("Loading world scenery and water...".into());
    let (heights, normals) = terrain_pages.finish(terrain_store::budget())?;
    let world_scene = scene::Scene::load(&dir.join("world.ecwld"))?;
    let cloud_surface = match fs::read(dir.join("world.cld")) {
        Ok(bytes) => {
            let cloud = cloud_surface::parse(&bytes).map_err(|e| format!("world.cld: {e}"))?;
            let (cell_count, vertex_count, group_count, index_count) =
                cloud.cells.iter().flatten().fold(
                    (0usize, 0usize, 0usize, 0usize),
                    |(cells, vertices, groups, indices), cell| {
                        (
                            cells + usize::from(!cell.vertices.is_empty()),
                            vertices + cell.vertices.len(),
                            groups + cell.groups.len(),
                            indices + cell.groups.iter().map(|g| g.indices.len()).sum::<usize>(),
                        )
                    },
                );
            eprintln!("Loaded native MOXB cloud grid {}x{}: {cell_count} populated cells, {vertex_count} vertices, {group_count} groups, {index_count} indices", cloud.width, cloud.height);
            Some(std::sync::Arc::new(cloud))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("world.cld unavailable; native cloud surface omitted");
            None
        }
        Err(e) => return Err(format!("world.cld: {e}")),
    };
    if (world_scene.width - w as f32 * grid_spacing).abs() > 0.1
        || (world_scene.length - h as f32 * grid_spacing).abs() > 0.1
    {
        return Err("ECWLD and TRN2 world dimensions differ".into());
    }
    let water = water::load(&world_scene, &dir)?;
    eprintln!(
        "Loaded {} water areas from client BSData grids",
        water.len()
    );
    let ornaments = world_scene.load_ornaments(&dir.join("world.ecbsd"))?;
    eprintln!(
        "Loaded {} unique world ornament placements; nearby BMD meshes will stream during flight",
        ornaments.len()
    );
    let count_trees: usize = world_scene.blocks.iter().map(|b| b.trees.len()).sum();
    let count_models: usize = world_scene.blocks.iter().map(|b| b.models.len()).sum();
    eprintln!("Loaded scene placements: {count_trees} trees, {count_models} EC models");
    eprintln!("Loading world terrain textures and masks...");
    #[cfg(windows)]
    windows_setup::loading_progress("Loading terrain materials and masks...".into());
    let materials = material::Materials::load(&dir, &t, &names, w, h, grid_spacing)?;
    let element = dir
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid world path")?;
    let water_normals = water::load_normals(element)?;
    let water_simple_frames = water::load_simple_frames(element)?;
    let objects = objects::Objects::open(element, ornaments)?;
    let placements: Vec<_> = world_scene
        .blocks
        .iter()
        .flat_map(|b| b.models.iter().cloned())
        .collect();
    let models = match world_models::Assets::open(element, placements.clone()) {
        Ok(models) => models,
        Err(e) => world_models::Assets::load_with(placements, |_| Err(e.clone())),
    };
    eprintln!("Retained {} EC model assets for {} placements; {} unloaded placements (ordinary animated submission enabled unless PW_DISABLE_WORLD_MODELS is set)", models.assets.len(), models.instances.len(), models.unloaded.len());
    for failure in &models.failures {
        eprintln!("EC model dependency: {failure}");
    }
    let grass = grass::Grass::load(&world_scene, &dir, element)?;
    #[cfg(windows)]
    windows_setup::loading_progress(
        "Preparing trees (first run may take several minutes)...".into(),
    );
    let tree_cache = if let Some(dir) = std::env::var_os("PW_TREE_CACHE") {
        PathBuf::from(dir)
    } else {
        #[cfg(windows)]
        {
            windows_setup::prepare_trees(element)?
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("tree-cache")
        }
    };
    let trees = trees::Trees::open(element, &world_scene, &tree_cache)?;
    #[cfg(windows)]
    if std::env::var_os("PW_TREE_CACHE").is_none() && trees.types.iter().any(|tree| tree.is_none())
    {
        return Err(format!("Some prepared tree types could not be loaded. Close Perfect World Explorer, remove this cache folder, and try again:\n{}",tree_cache.display()));
    }
    let environments = environment::Environments::load(&world_scene, &dir)?;
    // Ordinary world-model receivers query these actual client records.
    // Do not also add them to static baked BMDs or terrain: those surfaces
    // use their separately traced baked-light paths.
    let scene_lights = match scene_lights::SceneLights::load(&dir.join("scenelights.dat")) {
        Ok(lights) => {
            eprintln!(
                "Loaded {} client scene lights (static query only)",
                lights.lights.len()
            );
            lights
        }
        Err(e) => {
            eprintln!("Scene lights unavailable: {e}");
            scene_lights::SceneLights::default()
        }
    };
    Ok(Terrain {
        models,
        environments,
        scene_lights: std::sync::Arc::new(scene_lights),
        scene: std::sync::Arc::new(world_scene),
        scenery_residency: scenery_visibility::RegionResidency::default(),
        terrain_material_window: None,
        element: element.to_path_buf(),
        grass,
        water_normals,
        water_simple_frames,
        trees,
        cloud_surface,
        water,
        objects,
        materials,
        subterrain_names: names,
        grid,
        width: w,
        height: h,
        heights,
        normals,
        grid_spacing,
    })
}
fn export_png(terrain: &Terrain, output: &Path) -> Result<(), String> {
    export_height_png(terrain.width, terrain.height, &terrain.heights, output)
}
fn export_height_png(
    w: usize,
    h: usize,
    values: &terrain_store::Heights,
    output: &Path,
) -> Result<(), String> {
    let mut heights = values.reader();
    let f = fs::File::create(output).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let mut stream = writer.stream_writer().map_err(|e| e.to_string())?;
    let mut rgb = vec![0u8; w * 3];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let z = heights.height(i);
            let dx = heights.height(y * w + (x + 1).min(w - 1))
                - heights.height(y * w + x.saturating_sub(1));
            let dy = heights.height((y + 1).min(h - 1) * w + x)
                - heights.height(y.saturating_sub(1) * w + x);
            let light = (0.74 + (-dx * 0.025 + dy * 0.025)).clamp(0.30, 1.15);
            let base = if z < 0.0 {
                [50., 83., 94.]
            } else if z < 80.0 {
                [104., 127., 82.]
            } else if z < 250.0 {
                [131., 133., 94.]
            } else {
                [153., 150., 136.]
            };
            for c in 0..3 {
                rgb[x * 3 + c] = (base[c] * light).clamp(0., 255.) as u8;
            }
        }
        stream.write_all(&rgb).map_err(|e| e.to_string())?;
    }
    stream.finish().map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    io::stderr().flush().ok();
    Ok(())
}
mod viewer;

fn main() {
    #[cfg(windows)]
    std::panic::set_hook(Box::new(|info| {
        windows_setup::show_error(&format!(
            "Perfect World Explorer encountered an unexpected error:\n{info}"
        ))
    }));
    #[allow(unused_mut)]
    let mut args: Vec<_> = env::args_os().collect();
    #[cfg(windows)]
    if args.len() == 1 {
        match windows_setup::start_window() {
            Ok(Some(path)) => args.push(path.into_os_string()),
            Ok(None) => return,
            Err(e) => {
                windows_setup::show_error(&e);
                return;
            }
        }
    }
    if args.len() < 2 || (args.len() > 4 && args.len() != 9) {
        eprintln!(
            "Usage: perfect-world-explorer <client-root> [terrain-output.png | --preview-3d output.png]"
        );
        std::process::exit(2)
    }
    let path = PathBuf::from(&args[1]);
    #[cfg(windows)]
    let loading = windows_setup::LoadingWindow::start();
    let loaded = terrain(&path);
    let result = loaded.and_then(|mut t| match args.len() {
        #[cfg(windows)]
        2 => gpu::run(t, Some(loading)),
        #[cfg(not(windows))]
        2 => gpu::run(t),
        3 => export_png(&t, Path::new(&args[2])),
        4 if args[2] == "--preview-3d" => viewer::preview(&mut t, Path::new(&args[3])),
        4 if args[2] == "--gpu-preview" => gpu::preview(t, Path::new(&args[3]), None),
        9 if args[2] == "--gpu-preview-at" => {
            let mut coords = [0f32; 5];
            for i in 0..5 {
                coords[i] = args[3 + i]
                    .to_string_lossy()
                    .parse::<f32>()
                    .map_err(|_| "invalid camera number")?;
            }
            gpu::preview(t, Path::new(&args[8]), Some(coords))
        }
        9 if args[2] == "--preview-at" => {
            let mut coords = [0f32; 5];
            for i in 0..5 {
                coords[i] = args[3 + i]
                    .to_string_lossy()
                    .parse::<f32>()
                    .map_err(|_| "invalid camera number")?;
            }
            viewer::preview_at(
                &mut t,
                Path::new(&args[8]),
                coords[0],
                coords[1],
                coords[2],
                coords[3],
                coords[4],
            )
        }
        _ => Err("invalid arguments".into()),
    });
    if let Err(e) = result {
        eprintln!("Error: {e}");
        #[cfg(windows)]
        windows_setup::show_error(&e);
        std::process::exit(1)
    }
}

#[cfg(test)]
mod ram_export_tests {
    #[test]
    fn streamed_png_preserves_decoded_rgb() {
        let path =
            std::env::temp_dir().join(format!("pw-streamed-export-{}.png", std::process::id()));
        let heights = vec![0.; 12].into();
        super::export_height_png(4, 3, &heights, &path).unwrap();
        let decoder = png::Decoder::new(std::fs::File::open(&path).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height), (4, 3));
        assert_eq!(frame.color_type, png::ColorType::Rgb);
        assert_eq!(&pixels[..frame.buffer_size()], &[76u8, 93, 60].repeat(12));
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
}
