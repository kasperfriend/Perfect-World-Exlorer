//! Headless effect previews: the production effect pass, rendered offscreen.
//!
//! Nothing here draws with its own code. A preview builds a [`World`] exactly
//! as the viewer does, installs the placements the map authors, runs the frame
//! loop so the loader queue and the activation gates do their work, and then
//! renders the same pass the window renders — into a texture instead of the
//! swap chain. The result is written as a PNG, one per placement per frame.
//!
//! ```text
//! cargo test --release --bin perfect-world-explorer -- --ignored --nocapture effect_previews
//! ```
//!
//! `effect_preview_smoke` needs no client files: it renders a synthetic root
//! through the same path and asserts that pixels appeared, so the harness itself
//! is gated on every run. `effect_previews` renders the shipped roots and reads:
//!
//! | variable | meaning |
//! | --- | --- |
//! | `PWE_PREVIEW_ELEMENT` | client `element` directory holding `gfx.pck` |
//! | `PWE_PREVIEW_WORLD` | an `.ecwld` to take placements from |
//! | `PWE_PREVIEW_ROOTS` | `;`-separated path substrings to keep (empty: all) |
//! | `PWE_PREVIEW_FRAMES` | `,`-separated shot times in milliseconds |
//! | `PWE_PREVIEW_MAX` | how many placements to render |
//! | `PWE_PREVIEW_SIZE` | PNG edge in pixels |
//! | `PWE_PREVIEW_FADE` | the distance fade to shoot with, 0 for off |
//! | `PWE_PREVIEW_OUT` | output directory |
//!
//! The number that decides whether an effect is the size the map authored is
//! printed beside every shot: the world-space bounding box of the quads that
//! were submitted, and that box's diagonal. A sheet drawn at twice its authored
//! size says so in that line whatever it looks like on screen.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::scene::{Block, Effect, Scene};
use crate::world_models_gpu::World;

/// The pass insists on an encoded UNORM target, and a preview wants the same
/// numbers the window would get, so no sRGB suffix here.
const PREVIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The empty-world colour. A dark slate makes an additive sheet and an
/// alpha-blended one look different, which a black background would hide.
const CLEAR: [f64; 4] = [0.05, 0.06, 0.09, 1.];

fn clear_color() -> wgpu::Color {
    wgpu::Color {
        r: CLEAR[0],
        g: CLEAR[1],
        b: CLEAR[2],
        a: CLEAR[3],
    }
}

/// A device for the preview, or `None` when this machine has no adapter at
/// all (a test runner without a Vulkan ICD). Callers decide whether that is a
/// skip or a failure: the smoke test skips, the corpus test refuses.
fn preview_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let options = wgpu::RequestAdapterOptions::default();
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&options))?;
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("effect preview"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
    };
    pollster::block_on(adapter.request_device(&descriptor, None)).ok()
}

fn empty_scene() -> Scene {
    let block = Block {
        row: 0,
        col: 0,
        trees: Vec::new(),
        water: Vec::new(),
        grass: Vec::new(),
        ornaments: Vec::new(),
        boxes: Vec::new(),
        effects: Vec::new(),
        models: Vec::new(),
        critters: Vec::new(),
        beziers: Vec::new(),
        sounds: Vec::new(),
    };
    Scene {
        width: 8192.,
        length: 11264.,
        block_size: 64.,
        rows: 1,
        cols: 1,
        tree_types: Vec::new(),
        grass_types: Vec::new(),
        blocks: vec![block],
    }
}

/// One placement on its own, so a shot cannot be explained by its neighbours.
///
/// `iValidTime` is forced to 2: the preview is asked for a root, and whether the
/// map draws that root at noon is a different question, which the census in the
/// findings answers.
fn one_effect_scene(record: &Effect) -> Scene {
    let mut scene = empty_scene();
    if let Some(block) = scene.blocks.first_mut() {
        let mut copy = record.clone();
        copy.valid_time = 2;
        block.effects.push(copy);
    }
    scene
}

#[derive(Clone, Copy)]
struct Shot {
    width: u32,
    height: u32,
    eye: [f32; 3],
    target: [f32; 3],
}

impl Shot {
    fn matrix(self) -> [[f32; 4]; 4] {
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let projection = glam::Mat4::perspective_rh(45f32.to_radians(), aspect, 0.05, 20_000.);
        let eye = glam::Vec3::from_array(self.eye);
        let target = glam::Vec3::from_array(self.target);
        let view = glam::Mat4::look_at_rh(eye, target, glam::Vec3::Y);
        (projection * view).to_cols_array_2d()
    }
}

/// Frame a world box from one fixed direction, so two shots of two roots are
/// comparable: the distance follows the box, not the camera.
fn framing_shot(centre: [f32; 3], radius: f32, size: u32) -> Shot {
    let azimuth = 35f32.to_radians();
    let elevation = 20f32.to_radians();
    let half_angle = 22.5f32.to_radians().tan();
    let distance = (radius.max(0.25) / half_angle) * 1.15 + 0.5;
    let direction = glam::Vec3::new(
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
        azimuth.cos() * elevation.cos(),
    );
    let target = glam::Vec3::from_array(centre);
    Shot {
        width: size,
        height: size,
        eye: (target + direction * distance).to_array(),
        target: centre,
    }
}

/// The world's frame constants with the neutral light the reader falls back to
/// when no environment record matches the eye.
fn effect_frame(
    matrix: [[f32; 4]; 4],
    eye: [f32; 3],
    target: [f32; 3],
) -> crate::model_draw_gpu::Frame {
    let lighting = crate::model_material::Lighting {
        environment_ambient: [0.85, 0.85, 0.9, 1.],
        additional_ambient: [0.; 4],
        directional_diffuse: [0.35, 0.35, 0.3, 1.],
        point_ambient: [0.; 4],
        point_diffuse: [0.; 4],
        transparency: -1.,
        point_enabled: false,
    };
    let forward = glam::Vec3::from_array(target) - glam::Vec3::from_array(eye);
    let forward = if forward.length_squared() > 1e-6 {
        forward.normalize()
    } else {
        glam::Vec3::NEG_Z
    };
    crate::model_draw_gpu::Frame {
        blend_enabled: false,
        matrix,
        lighting,
        direction: [0., -1., 0.],
        point_position: [0.; 3],
        attenuation: [0., 0.4, 0.],
        eye_direction: forward.to_array(),
        directional_specular: [1., 1., 1., 1.],
        specular_variant: true,
        vertex_fog: crate::fog_matrix::MODEL_VERTEX_FOG,
        fog_color: [0., 0., 0., 0.],
        fog_range: [0., 0., 0., 0.],
        clip_plane: [0.; 4],
        reverse_front: false,
        alpha_compare: 5,
        alpha_reference: 84. / 255.,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    quads: usize,
    covered: usize,
    min: [f32; 3],
    max: [f32; 3],
}

impl Stats {
    fn radius(&self) -> f32 {
        let span = glam::Vec3::from_array(self.max) - glam::Vec3::from_array(self.min);
        if span.is_finite() {
            span.length() * 0.5
        } else {
            1.
        }
    }

    fn centre(&self) -> [f32; 3] {
        let min = glam::Vec3::from_array(self.min);
        let max = glam::Vec3::from_array(self.max);
        ((min + max) * 0.5).to_array()
    }

    fn describe(&self) -> String {
        format!(
            "quads={} pixels={} box=({:.2},{:.2},{:.2})..({:.2},{:.2},{:.2}) diagonal={:.2}",
            self.quads,
            self.covered,
            self.min[0],
            self.min[1],
            self.min[2],
            self.max[0],
            self.max[1],
            self.max[2],
            self.radius() * 2.
        )
    }
}

fn quad_bounds(quads: &[crate::gfx_player::EffectQuad]) -> Option<([f32; 3], [f32; 3])> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for quad in quads {
        for corner in &quad.positions {
            if !corner.iter().all(|value| value.is_finite()) {
                continue;
            }
            any = true;
            for axis in 0..3 {
                min[axis] = min[axis].min(corner[axis]);
                max[axis] = max[axis].max(corner[axis]);
            }
        }
    }
    any.then_some((min, max))
}

/// A world with one placement installed, ready to be ticked: the archive
/// attached, the gates open, and the fade set. The count says how many
/// placements `install_scene_effects` accepted.
fn preview_world(
    device: &wgpu::Device,
    element: &Path,
    record: &Effect,
    fade: f32,
) -> (World, usize) {
    let mut world = World::without_models(device, PREVIEW_FORMAT);
    world.attach_gfx_archive(element).expect("attach gfx.pck");
    let installed = world.install_scene_effects(&one_effect_scene(record));
    world.set_view_center(record.position[0], record.position[2], 1.0e6);
    world.set_effects_enabled(true);
    world.set_effect_distance(fade);
    (world, installed)
}

/// Tick the world `ticks` times at 33 ms. One root per frame is the loader's
/// own budget (0x5b1a00) and an element needs its clock past its first
/// keypoint, so a shot always advances a short run instead of a single frame.
fn advance(
    world: &mut World,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shot: Shot,
    ticks: u32,
) {
    let frame = effect_frame(shot.matrix(), shot.eye, shot.target);
    let lights = crate::scene_lights::SceneLights::default();
    for _ in 0..ticks.max(1) {
        world.update(device, queue, frame, shot.target, &lights, 0., 33);
    }
}

/// Render the effect pass into an RGBA texture and write the frame as a PNG.
fn capture(
    world: &mut World,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shot: Shot,
    path: &Path,
) -> Result<Stats, String> {
    let mut stats = Stats::default();
    if let Some((min, max)) = quad_bounds(world.effect_quads()) {
        stats.min = min;
        stats.max = max;
    }
    stats.quads = world.effect_quad_count();
    let extent = wgpu::Extent3d {
        width: shot.width,
        height: shot.height,
        depth_or_array_layers: 1,
    };
    let color_descriptor = wgpu::TextureDescriptor {
        label: Some("effect preview colour"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PREVIEW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let color = device.create_texture(&color_descriptor);
    let depth_descriptor = wgpu::TextureDescriptor {
        label: Some("effect preview depth"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    };
    let depth = device.create_texture(&depth_descriptor);
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    // The effect pipelines are built with the world's compare function, so a
    // depth attachment has to be present. Cleared to zero and compared with
    // `GreaterEqual`, every quad at or in front of the far plane passes: a
    // preview shows the root rather than its occlusion.
    let color_attachment = wgpu::RenderPassColorAttachment {
        view: &color_view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(clear_color()),
            store: wgpu::StoreOp::Store,
        },
    };
    let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
        view: &depth_view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(0.),
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    };
    let pass_descriptor = wgpu::RenderPassDescriptor {
        label: Some("effect preview pass"),
        color_attachments: &[Some(color_attachment)],
        depth_stencil_attachment: Some(depth_attachment),
        occlusion_query_set: None,
        timestamp_writes: None,
    };
    let encoder_descriptor = wgpu::CommandEncoderDescriptor {
        label: Some("effect preview encoder"),
    };
    let mut encoder = device.create_command_encoder(&encoder_descriptor);
    {
        let mut pass = encoder.begin_render_pass(&pass_descriptor);
        world.draw(&mut pass);
    }
    let padded = (shot.width * 4).div_ceil(256) * 256;
    let readback_descriptor = wgpu::BufferDescriptor {
        label: Some("effect preview readback"),
        size: (padded as u64) * (shot.height as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    };
    let readback = device.create_buffer(&readback_descriptor);
    let source = wgpu::ImageCopyTexture {
        texture: &color,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    };
    let destination = wgpu::ImageCopyBuffer {
        buffer: &readback,
        layout: wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(padded),
            rows_per_image: Some(shot.height),
        },
    };
    encoder.copy_texture_to_buffer(source, destination, extent);
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    let mapped = receiver.recv().map_err(|error| error.to_string())?;
    mapped.map_err(|error| format!("preview readback failed: {error}"))?;
    let bytes = slice.get_mapped_range();
    let row_bytes = (shot.width * 4) as usize;
    let mut pixels = Vec::with_capacity(row_bytes * shot.height as usize);
    for row in 0..shot.height as usize {
        let start = row * padded as usize;
        pixels.extend_from_slice(&bytes[start..start + row_bytes]);
    }
    drop(bytes);
    readback.unmap();
    let clear = [
        (CLEAR[0] * 255.).round() as i32,
        (CLEAR[1] * 255.).round() as i32,
        (CLEAR[2] * 255.).round() as i32,
    ];
    let mut covered = 0usize;
    for pixel in pixels.chunks_exact(4) {
        if (0..3).any(|axis| (pixel[axis] as i32 - clear[axis]).abs() > 6) {
            covered += 1;
        }
    }
    stats.covered = covered;
    write_png(path, shot.width, shot.height, &pixels)?;
    Ok(stats)
}

/// Advance the world and then capture it: the one call a caller needs.
fn render_shot(
    world: &mut World,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shot: Shot,
    path: &Path,
    ticks: u32,
) -> Result<Stats, String> {
    advance(world, device, queue, shot, ticks);
    capture(world, device, queue, shot, path)
}

/// Write RGBA8 as a PNG, gamma-encoded so the file looks like the game rather
/// than like linear light: the pass renders into an encoded UNORM target, so
/// what comes back is scene-linear.
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut table = [0u8; 256];
    for (value, entry) in table.iter_mut().enumerate() {
        let linear = value as f32 / 255.;
        *entry = (linear.powf(1. / 2.2) * 255.).round().clamp(0., 255.) as u8;
    }
    let mut encoded = rgba.to_vec();
    for pixel in encoded.chunks_exact_mut(4) {
        pixel[0] = table[pixel[0] as usize];
        pixel[1] = table[pixel[1] as usize];
        pixel[2] = table[pixel[2] as usize];
    }
    let shown = path.display();
    let file = std::fs::File::create(path).map_err(|error| format!("{shown}: {error}"))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let header = encoder.write_header();
    let mut writer = header.map_err(|error| format!("{shown}: {error}"))?;
    writer
        .write_image_data(&encoded)
        .map_err(|error| format!("{shown}: {error}"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn env_list(name: &str, separator: char, default: &str) -> Vec<String> {
    let text = std::env::var(name).unwrap_or_else(|_| default.to_string());
    let mut items = Vec::new();
    for item in text.split(separator) {
        let item = item.trim();
        if !item.is_empty() {
            items.push(item.to_string());
        }
    }
    items
}

fn env_u32(name: &str, default: u32) -> u32 {
    let parsed = std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok());
    parsed.unwrap_or(default)
}

fn env_f32(name: &str, default: f32) -> f32 {
    let parsed = std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok());
    parsed.unwrap_or(default)
}

fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            out.push(character);
        } else {
            out.push('_');
        }
    }
    out
}

/// The synthetic root the smoke test renders: one sprite card, no archive.
const FAKE_ROOT: &str = "MOXTVersion: 37\r\nDedaultScale: 1\r\nPlaySpeed: 1\r\nDefaultAlpha: 1\r\n\
     Raytrace: 0\r\nFadeByDist: 0\r\nFadeStart: 0.000000\r\nFadeEnd: 0.000000\r\n\
     Vec: 0.100000, 0.200000, 0.300000\r\nVec: 1.000000, 2.000000, 3.000000\r\n\
     ShakeCam: 0\r\nGFXELEMENTCOUNT: 1\r\n\
     GFXELEMENTID: 100\r\nName: glow\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
     RepeatCount: 1\r\nRepeatDelay: 0\r\nTexFile: 粒子\\glow.dds\r\nBindEle: \r\nZEnable: 0\r\n\
     MatchGrnd: 0\r\nTexRow: 1\r\nTexCol: 1\r\nTexInterval: 50\r\nPriority: 2\r\nIsDummy: 0\r\n\
     DummyEle: \r\nWarp: 0\r\nWidth: 4.000000\r\nHeight: 4.000000\r\nRotFromView: 0\r\n\
     GrndNormOnly: 0\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\n\
     TimeSpan: -1\r\nPosition: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
     Rad_2D: 0\r\nCtrlMethodCount: 0\r\n";

/// A 4x4 opaque white TGA, built here rather than shipped: the smoke test has
/// to have a decodable map and takes no client file.
fn white_tga() -> Vec<u8> {
    let mut bytes = vec![0u8; 18];
    bytes[2] = 2; // uncompressed true-colour
    bytes[12] = 4; // width, little endian
    bytes[14] = 4; // height
    bytes[16] = 32; // bits per pixel
    bytes[17] = 8 | 0x20; // eight alpha bits, top-left origin
    for _ in 0..16 {
        bytes.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    }
    bytes
}

fn fake_record() -> Effect {
    Effect {
        index: 0,
        export_id: 1,
        scale: 1.,
        position: [0., 0., 0.],
        direction: [0., 0., -1.],
        up: [0., 1., 0.],
        speed: 1.,
        valid_time: 2,
        alpha: 1.,
        attenuation: false,
        path: "场景\\preview卡.gfx".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness itself, with no client files: one synthetic card through the
    /// real world and effect pass has to cover pixels. If this fails, the
    /// previews are not evidence of anything.
    #[test]
    fn effect_preview_smoke() {
        // The plain unit-test job runs on a machine with no Vulkan ICD, and a
        // test that needs a GPU must not turn that into a red build. The GPU
        // job in `pr-checks.yml` is where this actually runs.
        let Some((device, queue)) = preview_device() else {
            crate::pck::corpus_absent("Vulkan adapter");
            return;
        };
        // Roots are decoded text, so the smoke archive is a text reader. Its
        // texture reader answers every name with one 4x4 white TGA: a bucket
        // whose map will not load is painted with a *transparent* stand-in, so
        // a preview with no texture archive draws nothing at all -- the first
        // version of this test proved exactly that (quads=1 pixels=0).
        let root = FAKE_ROOT.to_string();
        let texture = white_tga();
        let read = move |_path: &str| Ok(texture.clone());
        let assets = crate::world_models::Assets::default();
        let mut world = World::with_reader(&device, PREVIEW_FORMAT, assets, read)
            .expect("a world with no models");
        world.attach_gfx_reader(move |path: &str| {
            if path.ends_with("preview卡.gfx") {
                Ok(root.clone())
            } else {
                Err(format!("the smoke archive has no root: {path}"))
            }
        });
        let scene = one_effect_scene(&fake_record());
        let installed = world.install_scene_effects(&scene);
        assert_eq!(installed, 1, "one placement installed");
        world.set_view_center(0., 0., 1.0e6);
        world.set_effects_enabled(true);
        world.set_effect_distance(0.);
        let shot = Shot {
            width: 128,
            height: 128,
            eye: [0., 1.5, 9.],
            target: [0., 0., 0.],
        };
        let out = std::env::temp_dir().join("pwe-effect-preview-smoke.png");
        let stats =
            render_shot(&mut world, &device, &queue, shot, &out, 24).expect("a rendered frame");
        eprintln!("smoke: {}", stats.describe());
        assert_eq!(stats.quads, 1, "the card reached the pass");
        assert!(stats.covered > 200, "the card has to cover pixels");
    }

    /// The shipped roots, one shot per placement, auto-framed so two effects of
    /// different sizes fill the frame the same way. The printed line carries the
    /// world-space box, which is what says whether an effect is oversized.
    #[test]
    #[ignore]
    fn effect_previews() {
        let default_element = PathBuf::from("google_drive");
        let element = env_path("PWE_PREVIEW_ELEMENT").unwrap_or(default_element);
        if !element.join("gfx.pck").is_file() {
            crate::pck::corpus_absent("`gfx.pck`");
            return;
        }
        let default_world = PathBuf::from("google_drive/world.ecwld");
        let world_path = env_path("PWE_PREVIEW_WORLD").unwrap_or(default_world);
        if !world_path.is_file() {
            crate::pck::corpus_absent("`world.ecwld`");
            return;
        }
        let default_out = PathBuf::from("target/effect-preview");
        let out = env_path("PWE_PREVIEW_OUT").unwrap_or(default_out);
        let size = env_u32("PWE_PREVIEW_SIZE", 384);
        let limit = env_u32("PWE_PREVIEW_MAX", 8) as usize;
        let fade = env_f32("PWE_PREVIEW_FADE", 0.);
        let mut frames = Vec::new();
        for value in env_list("PWE_PREVIEW_FRAMES", ',', "900") {
            if let Ok(parsed) = value.parse::<f32>() {
                frames.push(parsed);
            }
        }
        let wanted = env_list("PWE_PREVIEW_ROOTS", ';', "");
        let (device, queue) = preview_device().expect("a Vulkan adapter for the previews");
        let scene = crate::scene::Scene::load(&world_path).expect("parse the world file");
        eprintln!(
            "preview: {} blocks, roots {:?}, out {}",
            scene.blocks.len(),
            wanted,
            out.display()
        );
        let mut shots = 0usize;
        for block in &scene.blocks {
            for record in &block.effects {
                if !wanted.is_empty() && !wanted.iter().any(|key| record.path.contains(key)) {
                    continue;
                }
                if shots >= limit {
                    eprintln!("preview: stopping at the {limit} placement limit");
                    return;
                }
                shots += 1;
                // A wide first shot, so the real frame can be fitted to what the
                // root actually emits instead of to a guess at its size. The
                // probe runs to the largest requested time, so a sheet that
                // grows is framed at its largest state and the earlier frames
                // are not cropped.
                let wide = Shot {
                    width: 128,
                    height: 128,
                    eye: [
                        record.position[0] + 60.,
                        record.position[1] + 30.,
                        record.position[2] + 60.,
                    ],
                    target: record.position,
                };
                let latest = frames.iter().cloned().fold(0f32, f32::max);
                let probe_ticks = (latest / 33.).ceil() as u32 + 24;
                let (mut probe_world, installed) = preview_world(&device, &element, record, fade);
                let probe = render_shot(
                    &mut probe_world,
                    &device,
                    &queue,
                    wide,
                    &out.join("probe.png"),
                    probe_ticks,
                );
                let fitted = match probe {
                    Ok(stats) if stats.quads > 0 => (stats.centre(), stats.radius()),
                    _ => {
                        eprintln!(
                            "preview: {} at {:?} emitted nothing (installed {installed})",
                            record.path, record.position
                        );
                        continue;
                    }
                };
                let shot = framing_shot(fitted.0, fitted.1, size);
                for time in &frames {
                    let name = format!(
                        "{:03}-{}-{:04}ms.png",
                        record.export_id,
                        slug(&record.path),
                        *time as i64
                    );
                    let path = out.join(&name);
                    // One world per time: the clock below then reads as the
                    // milliseconds since the placement started.
                    let (mut world, _) = preview_world(&device, &element, record, fade);
                    let ticks = (*time / 33.).ceil() as u32 + 24;
                    match render_shot(&mut world, &device, &queue, shot, &path, ticks) {
                        Ok(stats) => {
                            // A shot with quads but no pixels is a texture the
                            // map reader refused, not an empty root: say which.
                            let textures = world.effect_texture_failures();
                            let note = match textures.first() {
                                Some(first) => {
                                    format!(" textures-failed={} first={first}", textures.len())
                                }
                                None => String::new(),
                            };
                            eprintln!(
                                "preview: {} at ({:.1},{:.1},{:.1}) scale={:.2} alpha={:.2} t={}ms {}{note}",
                                record.path,
                                record.position[0],
                                record.position[1],
                                record.position[2],
                                record.scale,
                                record.alpha,
                                *time as i64,
                                stats.describe()
                            );
                        }
                        Err(error) => {
                            eprintln!("preview: {} at {}ms: {error}", record.path, *time as i64)
                        }
                    }
                }
            }
        }
        eprintln!("preview: {shots} placements rendered into {}", out.display());
    }
}
