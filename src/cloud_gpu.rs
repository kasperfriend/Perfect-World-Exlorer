//! World-space cloud billboards built from the native MOXB groups.
//!
//! Native `0x646450` submits grouped cloud indices as triangle lists, and
//! `0x646720` maps the first vertex tag into one of sixteen cells in the
//! client's 4-by-4 cumulus atlas. We use that recovered atlas mapping on
//! camera-facing point billboards. This remains an approximation: native
//! triangle construction, camera-relative geometry, size/color curves, and
//! secondary water-view admission are still being reverse engineered.
use crate::cloud_surface::CloudSurface;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{fs::File, path::Path};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    center_width: [f32; 4],
    height_night: [f32; 4],
    day_color: [f32; 4],
    atlas_uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    matrix: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    fog_range: [f32; 4],
    fog_color: [f32; 4],
    clip: [f32; 4],
    scene: [f32; 4], // daylight, global opacity, reserved, reserved
}

const SHADER: &str = r#"
struct Params {
    matrix: mat4x4<f32>, right: vec4<f32>, up: vec4<f32>,
    fog_range: vec4<f32>, fog_color: vec4<f32>, clip: vec4<f32>, scene: vec4<f32>,
};
struct Cloud { center_width: vec4<f32>, height_night: vec4<f32>, day_color: vec4<f32>, atlas_uv: vec4<f32> };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> clouds: array<Cloud>;
@group(0) @binding(2) var cumulus: texture_2d<f32>;
@group(0) @binding(3) var cloud_sampler: sampler;
struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec3<f32>,
    @location(2) view_depth: f32,
    @location(3) world_y: f32,
};
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}
@vertex fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) n: u32) -> VOut {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));
    let uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0));
    let c = clouds[n];
    let corner = corners[v];
    let world = c.center_width.xyz
        + p.right.xyz * (corner.x * c.center_width.w)
        + p.up.xyz * (corner.y * c.height_night.x);
    let clip = p.matrix * vec4<f32>(world, 1.0);
    var o: VOut;
    o.position = clip;
    o.uv = mix(c.atlas_uv.xy, c.atlas_uv.zw, uvs[v]);
    o.tint = srgb_to_linear(mix(c.height_night.yzw, c.day_color.xyz, p.scene.x));
    o.view_depth = clip.w;
    o.world_y = world.y;
    return o;
}
fn fogged(color: vec3<f32>, depth: f32) -> vec3<f32> {
    if p.fog_range.z < 0.5 { return color; }
    let span = p.fog_range.y - p.fog_range.x;
    let f = select(
        clamp((depth - p.fog_range.x) / select(span, 0.001, abs(span) < 0.001), 0.0, 1.0),
        select(0.0, 1.0, depth >= p.fog_range.y),
        abs(span) < 0.001);
    let encoded = select(color * 12.92, 1.055 * pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, color > vec3<f32>(0.0031308));
    let mixed = mix(encoded, p.fog_color.rgb, f);
    return select(mixed / 12.92, pow((max(mixed, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4)), mixed > vec3<f32>(0.04045));
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    if abs(p.clip.y) > 0.5 && (i.world_y - p.clip.x) * p.clip.y < 0.0 { discard; }
    let tex = textureSample(cumulus, cloud_sampler, i.uv);
    if tex.a < 0.003 { discard; }
    let rgb = fogged(tex.rgb * i.tint, i.view_depth);
    return vec4<f32>(rgb, tex.a * p.scene.y);
}
"#;

fn unpack_rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 255) as f32 / 255.,
        ((c >> 8) & 255) as f32 / 255.,
        (c & 255) as f32 / 255.,
    ]
}

/// Shipped groups overwhelmingly use 20, 50, or 70 in the fourth parameter.
/// Two outlier values (14,175 and 889,206,080) occur in the supplied file;
/// do not let them inflate every sprite. The modal 50-unit fallback and the
/// half-parameter conversion remain approximations pending native size tracing.
fn group_scale(parameter: f32) -> f32 {
    let class = if [20.0, 50.0, 70.0].contains(&parameter) {
        parameter
    } else {
        50.0
    };
    class * 0.5
}

/// The native helper at 0x646720 subtracts one from the first vertex tag,
/// wraps it into a 4x4 grid, and assigns a quarter-texture UV interval. Keep a
/// one-texel inset to avoid filtering adjacent atlas cells together.
fn atlas_uv(tag: u32) -> [f32; 4] {
    const CELLS: u32 = 4;
    const INSET: f32 = 1.0 / 1024.0;
    let cell = tag.saturating_sub(1).min(15);
    let x = (cell % CELLS) as f32 * 0.25;
    let y = (cell / CELLS) as f32 * 0.25;
    [x + INSET, y + INSET, x + 0.25 - INSET, y + 0.25 - INSET]
}

/// Expand each native group index into one camera-facing cloud sprite. The
/// atlas-cell selection is recovered from native code; billboard dimensions
/// and the group scale are still provisional visual approximations.
fn billboard_instances(surface: &CloudSurface) -> Vec<Instance> {
    let mut out = Vec::new();
    for cell in surface.cells.iter().flatten() {
        for group in &cell.groups {
            let scale = group_scale(group.parameters[3]);
            for &index in &group.indices {
                let Some(vertex) = cell.vertices.get(index as usize) else {
                    continue;
                };
                let day = unpack_rgb(vertex.colors[0]);
                let night = unpack_rgb(vertex.colors[2]);
                out.push(Instance {
                    center_width: [
                        vertex.position[0],
                        vertex.position[1],
                        vertex.position[2],
                        scale,
                    ],
                    height_night: [scale, night[0], night[1], night[2]],
                    day_color: [day[0], day[1], day[2], 1.0],
                    atlas_uv: atlas_uv(vertex.tags[0]),
                });
            }
        }
    }
    out
}

fn cumulus_texture(element: &Path) -> Result<image::RgbaImage, String> {
    let mut file = File::open(element.join("textures.pck")).map_err(|e| e.to_string())?;
    let entry = crate::pck::directory(&mut file)?
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("textures/cumulus01.tga"))
        .ok_or("textures.pck lacks textures/cumulus01.tga")?;
    image::load_from_memory(&crate::pck::read_entry(&mut file, &entry)?)
        .map(|image| image.to_rgba8())
        .map_err(|e| format!("cumulus01.tga: {e}"))
}

pub struct CloudGpu {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    params: wgpu::Buffer,
    _instances: wgpu::Buffer,
    _texture: wgpu::Texture,
    _sampler: wgpu::Sampler,
    instance_count: u32,
}

impl CloudGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        element: &Path,
        surface: &CloudSurface,
    ) -> Result<Self, String> {
        let instances = billboard_instances(surface);
        if instances.is_empty() {
            return Err("MOXB contains no indexed cloud elements".into());
        }
        let image = cumulus_texture(element)?;
        let (width, height) = (image.width(), image.height());
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("native cumulus cloud texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            // Keep one mip level: generating a whole-atlas mip chain would
            // blend neighboring cloud cells at distance.
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
            image.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cumulus linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cloud camera and fog"),
            contents: bytemuck::bytes_of(&Params {
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
                right: [1., 0., 0., 0.],
                up: [0., 1., 0., 0.],
                fog_range: [0., 1., 0., 0.],
                fog_color: [0., 0., 0., 1.],
                clip: [0.; 4],
                scene: [1., 0.8, 0., 0.],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("MOXB cloud elements"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cloud billboard layout"),
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
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud billboard bindings"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instance_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native cloud surface billboards"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cloud billboard pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native cloud billboard pipeline"),
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
                // Match the reversed-Z world/depth buffer used by terrain:
                // depth clears to 0 and nearer fragments compare greater.
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        Ok(Self {
            pipeline,
            bind,
            params,
            _instances: instance_buffer,
            _texture: texture,
            _sampler: sampler,
            instance_count: instances.len().min(u32::MAX as usize) as u32,
        })
    }

    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        matrix: Mat4,
        yaw: f32,
        pitch: f32,
        enabled: bool,
        daylight: f32,
        fog: crate::fog_matrix::Fog,
        clip: [f32; 4],
    ) {
        if !enabled || self.instance_count == 0 {
            return;
        }
        let (s, c) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let right = Vec3::new(c, 0., s);
        let forward = Vec3::new(s * cp, sp, -c * cp);
        let up = right.cross(forward).normalize_or_zero();
        let params = Params {
            matrix: matrix.to_cols_array_2d(),
            right: [right.x, right.y, right.z, 0.],
            up: [up.x, up.y, up.z, 0.],
            fog_range: [fog.start, fog.end, if fog.enabled { 1. } else { 0. }, 0.],
            fog_color: [fog.color[0], fog.color[1], fog.color[2], 1.],
            clip,
            scene: [daylight.clamp(0., 1.), 0.8, 0., 0.],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world cloud surface"),
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
        pass.draw(0..6, 0..self.instance_count);
        drop(pass);
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_surface::{CloudCell, CloudGroup, CloudSurface, CloudVertex};

    use super::{atlas_uv, billboard_instances, group_scale};

    #[test]
    fn anomalous_group_parameters_cannot_create_huge_billboards() {
        assert_eq!(group_scale(20.0), 10.0);
        assert_eq!(group_scale(50.0), 25.0);
        assert_eq!(group_scale(70.0), 35.0);
        assert_eq!(group_scale(14_175.0), 25.0);
        assert_eq!(group_scale(889_206_080.0), 25.0);
    }

    #[test]
    fn each_group_index_expands_to_one_billboard() {
        let surface = CloudSurface {
            width: 1,
            height: 1,
            cells: vec![Some(CloudCell {
                index: 0,
                vertices: vec![CloudVertex {
                    position: [1., 2., 3.],
                    tags: [2, 4],
                    colors: [0x00ff_ffff, 0x00ff_ffff, 0x0080_8080, 0x0080_8080],
                }],
                groups: vec![CloudGroup {
                    parameters: [0., 0., 0., 50.],
                    indices: vec![0, 0],
                }],
            })],
        };
        let quads = billboard_instances(&surface);
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].center_width, [1., 2., 3., 25.]);
        assert_eq!(quads[0].height_night[0], 25.);
        assert_eq!(quads[0].atlas_uv, atlas_uv(2));
    }

    #[test]
    fn cloud_tags_select_the_native_four_by_four_atlas_cells() {
        let inset = 1.0 / 1024.0;
        assert_eq!(atlas_uv(1), [inset, inset, 0.25 - inset, 0.25 - inset]);
        assert_eq!(
            atlas_uv(5),
            [inset, 0.25 + inset, 0.25 - inset, 0.5 - inset]
        );
        assert_eq!(
            atlas_uv(16),
            [0.75 + inset, 0.75 + inset, 1.0 - inset, 1.0 - inset]
        );
    }
}
