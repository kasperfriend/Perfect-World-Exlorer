//! Camera-relative world-space precipitation using the client's rain/snow
//! masks and native 33 ms object clock. Particle generation is a deterministic
//! visual model, not a recovered copy of the client's particle RNG/lifetime.
//! Parsed water-area masks suppress the view when the camera is submerged.
use crate::client_weather::{WeatherClock, WeatherSelection};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, fs::File, path::Path};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    matrix: [[f32; 4]; 4],
    camera: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    fog_range: [f32; 4],
    fog_color: [f32; 4],
    clip: [f32; 4],
    clock_weather: [f32; 4], // wrapped ticks, water-area count, rain, snow
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WaterAreaGpu {
    bounds: [f32; 4], // center x, surface height, center z, cell spacing
    data: [u32; 4],   // columns, rows, byte offset into mask storage, reserved
}

fn pack_water_masks(areas: &[crate::water::WaterArea]) -> (Vec<WaterAreaGpu>, Vec<u32>) {
    let mut metadata = Vec::with_capacity(areas.len().max(1));
    let mut bytes = Vec::new();
    for area in areas {
        let offset = bytes.len().min(u32::MAX as usize) as u32;
        bytes.extend_from_slice(&area.mask);
        metadata.push(WaterAreaGpu {
            bounds: [area.center[0], area.height, area.center[2], area.grid_size],
            data: [
                area.columns as u32,
                area.rows as u32,
                offset,
                area.mask.len().min(u32::MAX as usize) as u32,
            ],
        });
    }
    if metadata.is_empty() {
        metadata.push(WaterAreaGpu::zeroed());
    }
    let words = if bytes.is_empty() {
        vec![0]
    } else {
        bytes
            .chunks(4)
            .map(|chunk| {
                let mut word = [0u8; 4];
                word[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(word)
            })
            .collect()
    };
    (metadata, words)
}

const SHADER: &str = r#"
struct Params {
    matrix: mat4x4<f32>, camera: vec4<f32>, right: vec4<f32>, up: vec4<f32>,
    forward: vec4<f32>, fog_range: vec4<f32>, fog_color: vec4<f32>,
    clip: vec4<f32>, clock_weather: vec4<f32>,
};
struct WaterArea { bounds: vec4<f32>, data: vec4<u32> };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var images: texture_2d_array<f32>;
@group(0) @binding(2) var smp: sampler;
@group(0) @binding(3) var<storage, read> water_areas: array<WaterArea>;
@group(0) @binding(4) var<storage, read> water_mask: array<u32>;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) view_depth: f32,
    @location(4) world_y: f32,
};
fn hash(n: f32) -> f32 { return fract(sin(n * 127.1 + 311.7) * 43758.5453); }
fn camera_in_mapped_water(camera: vec3<f32>) -> bool {
    let count = u32(p.clock_weather.y);
    for (var i = 0u; i < count; i = i + 1u) {
        let area = water_areas[i];
        let columns = area.data.x;
        let rows = area.data.y;
        let grid = area.bounds.w;
        if columns == 0u || rows == 0u || grid <= 0.0 { continue; }
        let half_x = f32(columns) * grid * 0.5;
        let half_z = f32(rows) * grid * 0.5;
        if abs(camera.x - area.bounds.x) >= half_x || abs(camera.z - area.bounds.z) >= half_z { continue; }
        let column = u32(floor((camera.x - (area.bounds.x - half_x)) / grid));
        let row = u32(floor(((area.bounds.z + half_z) - camera.z) / grid));
        if column >= columns || row >= rows { continue; }
        let cell_index = row * columns + column;
        if cell_index >= area.data.w { continue; }
        let byte_index = area.data.z + cell_index;
        let packed = water_mask[byte_index / 4u];
        let value = (packed >> ((byte_index % 4u) * 8u)) & 255u;
        if value != 0u && camera.y < area.bounds.y { return true; }
    }
    return false;
}
@vertex fn vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> Out {
    let snow = instance >= 320u;
    let local_id = f32(select(instance, instance - 320u, snow));
    let kind = select(0u, 1u, snow);
    let enabled = select(p.clock_weather.z, p.clock_weather.w, snow);
    let seed = local_id + f32(kind) * 8192.0;
    let phase = hash(seed + 7.0);
    let x = (hash(seed + 1.0) * 2.0 - 1.0) * 28.0;
    let z = 4.0 + hash(seed + 3.0) * 72.0;
    let speed = select(0.18 + hash(seed + 11.0) * 0.15, 0.015 + hash(seed + 13.0) * 0.03, snow);
    let seconds = p.clock_weather.x * 0.033;
    let fall = fract(phase + seconds * speed) * 110.0 - 55.0;
    let center = p.camera.xyz + p.right.xyz * x + p.forward.xyz * z + vec3<f32>(0.0, fall, 0.0);
    // Native CECWorld render admission tests the camera against the terrain
    // water mask and skips precipitation for a submerged view.
    let visible = enabled > 0.5 && !camera_in_mapped_water(p.camera.xyz);
    // The generated quad consists of six triangle-list vertices. Compute the
    // corners from the vertex ID instead of indexing an immutable local WGSL
    // array: naga 0.19 correctly rejects non-constant indexing of that value.
    let right = vertex == 1u || vertex == 4u || vertex == 5u;
    let top = vertex >= 2u;
    let corner = vec2<f32>(select(-1.0, 1.0, right), select(-1.0, 1.0, top));
    let uv_top = vertex == 0u || vertex == 1u || vertex == 4u;
    let uv = vec2<f32>(select(0.0, 1.0, right), select(0.0, 1.0, uv_top));
    let half_width = select(0.035, 0.12, snow);
    let half_height = select(1.1 + hash(seed + 17.0) * 1.3, 0.12, snow);
    let vertical = select(vec3<f32>(0.0, corner.y * half_height, 0.0), p.up.xyz * (corner.y * half_height), snow);
    let world = center + p.right.xyz * (corner.x * half_width) + vertical;
    let clip = p.matrix * vec4<f32>(world, 1.0);
    var o: Out;
    o.position = select(clip, vec4<f32>(2.0, 2.0, 2.0, 1.0), !visible);
    o.uv = uv;
    o.color = select(vec3<f32>(0.66, 0.78, 0.91), vec3<f32>(0.91, 0.95, 1.0), snow);
    o.kind = kind;
    o.view_depth = clip.w;
    o.world_y = world.y;
    return o;
}
fn fogged(color: vec3<f32>, depth: f32) -> vec3<f32> {
    if p.fog_range.z < 0.5 { return color; }
    let span = p.fog_range.y - p.fog_range.x;
    let f = select(clamp((depth - p.fog_range.x) / select(span, 0.001, abs(span) < 0.001), 0.0, 1.0),
                   select(0.0, 1.0, depth >= p.fog_range.y), abs(span) < 0.001);
    let encoded = select(color * 12.92, 1.055 * pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, color > vec3<f32>(0.0031308));
    let mixed = mix(encoded, p.fog_color.rgb, f);
    return select(mixed / 12.92, pow((max(mixed, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4)), mixed > vec3<f32>(0.04045));
}
@fragment fn fs(i: Out) -> @location(0) vec4<f32> {
    if abs(p.clip.y) > 0.5 && (i.world_y - p.clip.x) * p.clip.y < 0.0 { discard; }
    let tex = textureSample(images, smp, i.uv, i32(i.kind));
    let alpha = tex.a * select(0.52, 0.82, i.kind == 1u);
    if alpha < 0.003 { discard; }
    return vec4<f32>(fogged(tex.rgb * i.color, i.view_depth), alpha);
}
"#;

/// The client's snow BMP is grayscale on black without alpha. Convert the
/// luminance to an alpha mask before filtering to avoid opaque square cards.
fn snow_luminance_mask(mut pixels: image::RgbaImage) -> image::RgbaImage {
    for pixel in pixels.pixels_mut() {
        let [r, g, b, _] = pixel.0;
        let alpha = ((77 * r as u16 + 150 * g as u16 + 29 * b as u16 + 128) >> 8) as u8;
        *pixel = image::Rgba([255, 255, 255, alpha]);
    }
    pixels
}

fn weather_images(element: &Path) -> [image::RgbaImage; 2] {
    const SIZE: u32 = 64;
    let mut file = File::open(element.join("textures.pck")).ok();
    let entries = file
        .as_mut()
        .and_then(|file| crate::pck::directory(file).ok())
        .unwrap_or_default();
    let load = |name: &str, kind: usize, file: &mut Option<File>| {
        let bytes = entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .and_then(|entry| {
                file.as_mut()
                    .and_then(|file| crate::pck::read_entry(file, entry).ok())
            });
        let decoded = bytes
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| img.to_rgba8());
        if let Some(mut img) = decoded {
            if kind == 1 {
                img = snow_luminance_mask(img);
            }
            if img.width() == SIZE && img.height() == SIZE {
                return img;
            }
            return image::imageops::resize(
                &img,
                SIZE,
                SIZE,
                image::imageops::FilterType::Triangle,
            );
        }
        eprintln!("Native {name} weather texture unavailable; using a simple fallback mask");
        image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
            let dx = x as f32 - 31.5;
            let dy = y as f32 - 31.5;
            let alpha = if kind == 0 {
                if x >= 29 && x <= 34 {
                    220
                } else {
                    0
                }
            } else if dx * dx + dy * dy <= 220. {
                220
            } else {
                0
            };
            image::Rgba([255, 255, 255, alpha])
        })
    };
    [
        load("textures/rain.tga", 0, &mut file),
        load("textures/snow.bmp", 1, &mut file),
    ]
}

pub struct WeatherGpu {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    _texture: wgpu::Texture,
    _sampler: wgpu::Sampler,
    _water_areas: wgpu::Buffer,
    _water_mask: wgpu::Buffer,
    water_area_count: u32,
    clock: WeatherClock,
    ticks: u32,
}
impl WeatherGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        element: &Path,
        format: wgpu::TextureFormat,
        water_areas: &[crate::water::WaterArea],
    ) -> Self {
        let images = weather_images(element);
        let mut pixels = Vec::with_capacity(64 * 64 * 4 * 2);
        for image in images {
            pixels.extend_from_slice(image.as_raw());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("client rain and snow textures"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(64 * 4),
                rows_per_image: Some(64),
            },
            wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 2,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("client rain and snow texture array"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("weather mask sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world weather particle layout"),
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
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world weather parameters"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (water_area_data, water_mask_data) = pack_water_masks(water_areas);
        let water_areas_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native precipitation water-area bounds"),
            contents: bytemuck::cast_slice(&water_area_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let water_mask_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native precipitation water masks"),
            contents: bytemuck::cast_slice(&water_mask_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world weather particle bindings"),
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: water_areas_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: water_mask_buffer.as_entire_binding(),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world weather particle shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world weather particle pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world weather particle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                // The world camera uses a reversed-Z projection and clears
                // depth to 0, so translucent world particles must pass when
                // their depth is GREATER than the opaque depth in front.
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        Self {
            pipeline,
            bind,
            uniform,
            _texture: texture,
            _sampler: sampler,
            _water_areas: water_areas_buffer,
            _water_mask: water_mask_buffer,
            water_area_count: water_areas.len().min(u32::MAX as usize) as u32,
            clock: WeatherClock::default(),
            ticks: 0,
        }
    }

    /// Advance the native fixed-step weather clock exactly once per world frame.
    pub fn advance(&mut self, delta_ms: u32) {
        self.ticks = self.ticks.wrapping_add(self.clock.advance(delta_ms));
    }

    /// Submit a depth-tested world-space precipitation view. The generated
    /// particles fall through a camera-centred volume and the parsed water
    /// mask suppresses a submerged camera view. Native RNG/lifetimes and
    /// terrain collision remain outstanding.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        matrix: Mat4,
        camera: Vec3,
        yaw: f32,
        pitch: f32,
        fog: crate::fog_matrix::Fog,
        clip: [f32; 4],
        selection: WeatherSelection,
    ) {
        if !selection.rain && !selection.snow {
            return;
        }
        let (s, c) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let right = Vec3::new(c, 0., s);
        let forward = Vec3::new(s * cp, sp, -c * cp);
        let up = right.cross(forward).normalize_or_zero();
        let params = Params {
            matrix: matrix.to_cols_array_2d(),
            camera: [camera.x, camera.y, camera.z, 1.],
            right: [right.x, right.y, right.z, 0.],
            up: [up.x, up.y, up.z, 0.],
            forward: [forward.x, forward.y, forward.z, 0.],
            fog_range: fog.range(),
            fog_color: fog.color4(),
            clip,
            clock_weather: [
                (self.ticks % 1_000_000) as f32,
                self.water_area_count as f32,
                selection.rain as u8 as f32,
                selection.snow as u8 as f32,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&params));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("regional world-space precipitation"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..6, 0..480);
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_water_masks, snow_luminance_mask};
    use crate::water::WaterArea;

    #[test]
    fn water_masks_pack_with_byte_offsets_for_shader_lookup() {
        let area = |center: f32, mask: Vec<u8>| WaterArea {
            export_id: 1,
            color: 0,
            simple: false,
            height: 12.0,
            grid_size: 4.0,
            columns: mask.len(),
            rows: 1,
            center: [center, 0.0, 0.0],
            mask,
            shore_waves: Vec::new(),
        };
        let (metadata, words) =
            pack_water_masks(&[area(0.0, vec![0, 1, 2]), area(8.0, vec![3, 4])]);
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].data[2], 0);
        assert_eq!(metadata[1].data[2], 3); // second mask intentionally starts mid-word
        assert_eq!(metadata[0].data[3], 3);
        assert_eq!(metadata[1].data[3], 2);
        assert_eq!(words.as_slice(), &[0x0302_0100, 0x0000_0004]);
    }

    #[test]
    fn snow_bmp_luminance_becomes_antialiased_transparency() {
        let image = image::RgbaImage::from_raw(
            3,
            1,
            vec![0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255],
        )
        .unwrap();
        let masked = snow_luminance_mask(image);
        assert_eq!(masked.get_pixel(0, 0).0, [255, 255, 255, 0]);
        assert_eq!(masked.get_pixel(1, 0).0, [255, 255, 255, 128]);
        assert_eq!(masked.get_pixel(2, 0).0, [255, 255, 255, 255]);
    }
}
