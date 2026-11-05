//! Batched receiver for the runtime effect quads.
//!
//! The static decal pass in `gfx_decal_gpu` builds one vertex buffer and one
//! bind group per quad, which is right for a handful of character decals and
//! wrong for a particle system: a single retail torch root can hold three
//! emitters at a quota of a hundred particles each, and a scene block can hold
//! dozens of those roots. This pass therefore groups the frame's quads by
//! `(RenderLayer, depth test, blend pair, texture)` and draws one buffer per
//! group, growing the buffers instead of reallocating them.
//!
//! Render state follows `A3DGFXRenderSlotMan`, the same manager the verified
//! static path reproduces: colour and alpha modulate, lighting off, culling off,
//! depth writes off, and the authored D3D8 blend pair selected per element. The
//! element's `RenderLayer` decides the draw order, which is what the field is
//! for; within a layer, depth-tested quads go first so the additive ones blend
//! against settled geometry.

use crate::{gfx_player::EffectQuad, model_queue, model_texture_gpu};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    matrix: [[f32; 4]; 4],
    clip_plane: [f32; 4],
    /// `xyz` is the camera position. The root's `FadeByDist` ramp is already
    /// folded into the vertex colours on the CPU (`gfx_player::distance_fade`),
    /// so the shader does not read this field; it is uploaded because the
    /// client's GFX constants carry the eye position alongside the matrix.
    camera: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    source: u32,
    destination: u32,
    depth_test: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BucketKey {
    layer: i32,
    pipeline: PipelineKey,
    texture: String,
}

struct TextureEntry {
    // The view keeps the texture alive for the bind groups that reference it.
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Bucket {
    pipeline: PipelineKey,
    binding: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    /// Vertices the buffer holds; the CPU side is refilled every frame.
    capacity: usize,
}

/// How many distinct (layer, state, texture) groups are cached before the pass
/// drops them all. Retail roots reuse a few hundred at most.
const BUCKET_CACHE_LIMIT: usize = 1024;

pub struct Draw {
    device_format: wgpu::TextureFormat,
    depth: Option<(wgpu::TextureFormat, wgpu::CompareFunction)>,
    binding_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    textures: HashMap<String, TextureEntry>,
    buckets: HashMap<BucketKey, Bucket>,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    /// This frame's groups in draw order, with their vertex counts.
    order: Vec<(BucketKey, u32)>,
    staging: Vec<Vertex>,
    failed: Vec<String>,
}

fn blend_factor(code: u32) -> Result<wgpu::BlendFactor, String> {
    Ok(match model_queue::blend_factor(code)? {
        model_queue::Factor::Zero => wgpu::BlendFactor::Zero,
        model_queue::Factor::One => wgpu::BlendFactor::One,
        model_queue::Factor::SrcColor => wgpu::BlendFactor::Src,
        model_queue::Factor::InvSrcColor => wgpu::BlendFactor::OneMinusSrc,
        model_queue::Factor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        model_queue::Factor::InvSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        model_queue::Factor::DestAlpha => wgpu::BlendFactor::DstAlpha,
        model_queue::Factor::InvDestAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        model_queue::Factor::DestColor => wgpu::BlendFactor::Dst,
        model_queue::Factor::InvDestColor => wgpu::BlendFactor::OneMinusDst,
    })
}

impl Draw {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        depth: Option<(wgpu::TextureFormat, wgpu::CompareFunction)>,
    ) -> Result<Self, String> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err("GFX effects require an encoded UNORM target".into());
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("native GFX effect sampler"),
            // Effect textures tile: `TileMode` and the two `TexSpeed` values
            // scroll past the edge of the authored cell.
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GFX effect frame constants"),
            contents: bytemuck::bytes_of(&Params {
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
                clip_plane: [0.; 4],
                camera: [0.; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let binding_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GFX effect binding layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GFX effect pipeline layout"),
            bind_group_layouts: &[&binding_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native GFX effect shader"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        Ok(Self {
            device_format: format,
            depth,
            binding_layout,
            pipeline_layout,
            shader,
            pipelines: HashMap::new(),
            textures: HashMap::new(),
            buckets: HashMap::new(),
            uniform,
            sampler,
            order: Vec::new(),
            staging: Vec::new(),
            failed: Vec::new(),
        })
    }

    /// Textures that failed to load, for the status line.
    pub fn failures(&self) -> &[String] {
        &self.failed
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.order.iter().map(|(_, count)| *count as usize).sum()
    }

    /// Refill every group from this frame's quads. `read` resolves an archive
    /// path to bytes and is only called for textures the pass has not seen.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quads: &[EffectQuad],
        matrix: [[f32; 4]; 4],
        clip_plane: [f32; 4],
        camera: [f32; 4],
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        if matrix
            .iter()
            .flatten()
            .chain(clip_plane.iter())
            .chain(camera.iter())
            .any(|value| !value.is_finite())
        {
            return Err("non-finite GFX effect frame constants".into());
        }
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Params {
                matrix,
                clip_plane,
                camera,
            }),
        );

        // Group and order: `RenderLayer` first, then depth-tested quads, then
        // the blend pair and the texture so a group reuses one pipeline.
        let mut groups: Vec<(BucketKey, Vec<Vertex>)> = Vec::new();
        let mut index: HashMap<BucketKey, usize> = HashMap::new();
        for quad in quads {
            let key = BucketKey {
                layer: quad.layer,
                pipeline: PipelineKey {
                    source: quad.source_blend,
                    destination: quad.destination_blend,
                    depth_test: quad.depth_test,
                },
                texture: quad.texture.clone(),
            };
            let position = match index.get(&key) {
                Some(position) => *position,
                None => {
                    index.insert(key.clone(), groups.len());
                    groups.push((key.clone(), Vec::new()));
                    groups.len() - 1
                }
            };
            let vertices = &mut groups[position].1;
            let color = quad.color;
            // Two triangles per quad, in the client's corner order.
            for slot in [0usize, 1, 3, 0, 3, 2] {
                vertices.push(Vertex {
                    position: quad.positions[slot],
                    uv: quad.uv[slot],
                    color,
                });
            }
        }
        groups.sort_by(|(a, _), (b, _)| {
            a.layer
                .cmp(&b.layer)
                .then_with(|| a.pipeline.depth_test.cmp(&b.pipeline.depth_test).reverse())
                .then_with(|| a.pipeline.source.cmp(&b.pipeline.source))
                .then_with(|| a.pipeline.destination.cmp(&b.pipeline.destination))
                .then_with(|| a.texture.cmp(&b.texture))
        });

        if self.buckets.len() + groups.len() > BUCKET_CACHE_LIMIT {
            self.buckets.clear();
        }
        self.order.clear();
        self.order.reserve(groups.len());
        for (key, vertices) in &groups {
            if vertices.is_empty() {
                continue;
            }
            if !self.textures.contains_key(&key.texture) {
                match read(&key.texture) {
                    Ok(bytes) => {
                        match model_texture_gpu::upload(
                            device,
                            queue,
                            &bytes,
                            model_texture_gpu::UploadMode::Auto,
                        ) {
                            Ok(texture) => {
                                let view = texture.create_view(&Default::default());
                                self.textures
                                    .insert(key.texture.clone(), TextureEntry { texture, view });
                            }
                            Err(error) => {
                                self.note_failure(&key.texture, &error);
                                continue;
                            }
                        }
                    }
                    Err(error) => {
                        self.note_failure(&key.texture, &error);
                        continue;
                    }
                }
            }
            let entry = match self.textures.get(&key.texture) {
                Some(entry) => entry,
                None => continue,
            };
            if !self.pipelines.contains_key(&key.pipeline) {
                let pipeline = self.pipeline(device, key.pipeline)?;
                self.pipelines.insert(key.pipeline, pipeline);
            }
            if !self.buckets.contains_key(key) {
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("GFX effect texture binding"),
                    layout: &self.binding_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&entry.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                let capacity = vertices.len().next_power_of_two();
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("GFX effect quad batch"),
                    size: (capacity * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.buckets.insert(
                    key.clone(),
                    Bucket {
                        pipeline: key.pipeline,
                        binding,
                        buffer,
                        capacity,
                    },
                );
            }
            let bucket = match self.buckets.get_mut(key) {
                Some(bucket) => bucket,
                None => continue,
            };
            if vertices.len() > bucket.capacity {
                bucket.capacity = vertices.len().next_power_of_two();
                bucket.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("GFX effect quad batch"),
                    size: (bucket.capacity * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.staging.clear();
            self.staging.extend_from_slice(vertices);
            queue.write_buffer(&bucket.buffer, 0, bytemuck::cast_slice(&self.staging));
            self.order.push((key.clone(), vertices.len() as u32));
        }
        Ok(())
    }

    fn note_failure(&mut self, texture: &str, error: &str) {
        let message = format!("{texture}: {error}");
        if !self.failed.contains(&message) {
            self.failed.push(message);
        }
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for (key, count) in &self.order {
            let (Some(bucket), Some(pipeline)) =
                (self.buckets.get(key), self.pipelines.get(&key.pipeline))
            else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bucket.binding, &[]);
            pass.set_vertex_buffer(0, bucket.buffer.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }

    fn pipeline(
        &self,
        device: &wgpu::Device,
        key: PipelineKey,
    ) -> Result<wgpu::RenderPipeline, String> {
        let component = wgpu::BlendComponent {
            src_factor: blend_factor(key.source)?,
            dst_factor: blend_factor(key.destination)?,
            operation: wgpu::BlendOperation::Add,
        };
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x4];
        let buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        }];
        Ok(
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("native GFX effect receiver"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: "vs",
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: "fs",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.device_format,
                        blend: Some(wgpu::BlendState {
                            color: component,
                            alpha: component,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: self.depth.map(|(format, compare)| wgpu::DepthStencilState {
                    format,
                    // The slot manager never writes depth for an effect quad.
                    depth_write_enabled: false,
                    depth_compare: if key.depth_test {
                        compare
                    } else {
                        wgpu::CompareFunction::Always
                    },
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
            }),
        )
    }
}

const WGSL: &str = r#"
struct Params { matrix:mat4x4<f32>, clip_plane:vec4<f32>, camera:vec4<f32> };
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var image:texture_2d<f32>;
@group(0) @binding(2) var image_sampler:sampler;
struct Input {
 @location(0) position:vec3<f32>,
 @location(1) uv:vec2<f32>,
 @location(2) color:vec4<f32>,
};
struct Output {
 @builtin(position) position:vec4<f32>,
 @location(0) uv:vec2<f32>,
 @location(1) color:vec4<f32>,
 @location(2) world:vec3<f32>,
};
@vertex fn vs(v:Input)->Output {
 var o:Output;
 o.position=p.matrix*vec4<f32>(v.position,1.);
 o.world=v.position;
 o.uv=v.uv;
 o.color=v.color;
 return o;
}
@fragment fn fs(v:Output)->@location(0) vec4<f32> {
 if dot(vec4<f32>(v.world,1.),p.clip_plane)<0. {discard;}
 return textureSample(image,image_sampler,v.uv)*v.color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(
        texture: &str,
        source: u32,
        destination: u32,
        layer: i32,
        depth_test: bool,
        color: [f32; 4],
    ) -> EffectQuad {
        EffectQuad {
            texture: texture.into(),
            source_blend: source,
            destination_blend: destination,
            depth_test,
            layer,
            positions: [[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.], [1., 1., 0.]],
            uv: [[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
            color,
        }
    }

    /// A solid 64-grey 4x4 texture: the blend arithmetic is the only variable.
    fn grey_png() -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([64, 64, 64, 255]),
        ));
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    struct Harness {
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: wgpu::Texture,
        readback: wgpu::Buffer,
    }

    fn harness() -> Option<Harness> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            },
            None,
        ))
        .ok()?;
        let size = wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect readback"),
            size: 256 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Some(Harness {
            device,
            queue,
            target,
            readback,
        })
    }

    /// Render `quads` over `background` and return the first pixel's channels.
    fn render(
        draw: &mut Draw,
        harness: &Harness,
        background: f64,
        quads: &[EffectQuad],
    ) -> [f32; 4] {
        let png = grey_png();
        let mut read = |_: &str| Ok(png.clone());
        draw.update(
            &harness.device,
            &harness.queue,
            quads,
            glam::Mat4::IDENTITY.to_cols_array_2d(),
            [0.; 4],
            [0., 0., 0., 100.],
            &mut read,
        )
        .unwrap();
        let view = harness.target.create_view(&Default::default());
        let mut encoder = harness.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("effect test pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: background,
                            g: background,
                            b: background,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            draw.draw(&mut pass);
        }
        let size = wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        };
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &harness.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &harness.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(4),
                },
            },
            size,
        );
        harness.queue.submit(Some(encoder.finish()));
        let slice = harness.readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        harness.device.poll(wgpu::Maintain::Wait);
        receiver.recv().unwrap().unwrap();
        let bytes = slice.get_mapped_range();
        let pixel = [
            bytes[0] as f32 / 255.,
            bytes[1] as f32 / 255.,
            bytes[2] as f32 / 255.,
            bytes[3] as f32 / 255.,
        ];
        drop(bytes);
        harness.readback.unmap();
        pixel
    }

    #[test]
    fn gpu_effect_batches_honour_blend_mode_layer_and_depth_test() {
        let Some(harness) = harness() else {
            eprintln!("effect raster test skipped: no WGPU adapter");
            return;
        };
        let mut draw = Draw::new(
            &harness.device,
            &harness.queue,
            wgpu::TextureFormat::Rgba8Unorm,
            None,
        )
        .unwrap();

        // Additive over a 0.25 background: texture grey 64/255 times colour 1.
        // is 0.25, added to 0.25 gives 0.5. An alpha-blended pipeline would
        // have replaced the background instead, so this pins the blend pair.
        let additive = render(
            &mut draw,
            &harness,
            0.25,
            &[quad("a.png", 5, 2, 0, false, [1., 1., 1., 1.])],
        );
        assert!(
            (additive[0] - 0.5).abs() <= 0.02,
            "additive blend mismatch: {additive:?}"
        );

        // SrcAlpha/InvSrcAlpha with colour 1 and alpha 0.5 over the same
        // background: 1*0.5 + 0.25*0.5 = 0.625.
        let alpha = render(
            &mut draw,
            &harness,
            0.25,
            &[quad("a.png", 5, 6, 0, false, [1., 1., 1., 0.5])],
        );
        assert!(
            (alpha[0] - 0.625).abs() <= 0.02,
            "alpha blend mismatch: {alpha:?}"
        );

        // Two quads in different layers must both draw, and the pass must group
        // four quads over two textures and three states into three batches.
        let png = grey_png();
        let mut read = |_: &str| Ok(png.clone());
        draw.update(
            &harness.device,
            &harness.queue,
            &[
                quad("a.png", 5, 6, 1, false, [0.5, 0.5, 0.5, 1.]),
                quad("a.png", 5, 6, 1, false, [0.5, 0.5, 0.5, 1.]),
                quad("b.png", 5, 2, 0, false, [0.25, 0.25, 0.25, 1.]),
                quad("a.png", 5, 6, 0, true, [0.25, 0.25, 0.25, 1.]),
            ],
            glam::Mat4::IDENTITY.to_cols_array_2d(),
            [0.; 4],
            [0., 0., 0., 100.],
            &mut read,
        )
        .unwrap();
        assert_eq!(draw.texture_count(), 2);
        assert_eq!(draw.order.len(), 3, "four quads collapse into three groups");
        assert_eq!(draw.vertex_count(), 24);
        assert_eq!(draw.order[0].0.layer, 0);
        assert!(draw.order[0].0.pipeline.depth_test, "depth-tested first");
        assert_eq!(draw.order[2].0.layer, 1);
        assert_eq!(draw.order[2].1, 12);
        // The second frame reused the cached buffers and grew nothing.
        assert!(draw.buckets.values().all(|bucket| bucket.capacity >= 12));
    }

    #[test]
    fn gpu_effect_rejects_non_unorm_targets_and_reports_missing_textures() {
        let Some(harness) = harness() else {
            eprintln!("effect target test skipped: no WGPU adapter");
            return;
        };
        let error = Draw::new(
            &harness.device,
            &harness.queue,
            wgpu::TextureFormat::Rgba16Float,
            None,
        )
        .unwrap_err();
        assert!(error.contains("encoded UNORM"), "{error}");

        let mut draw = Draw::new(
            &harness.device,
            &harness.queue,
            wgpu::TextureFormat::Rgba8Unorm,
            None,
        )
        .unwrap();
        let mut read = |_: &str| -> Result<Vec<u8>, String> { Err("absent".into()) };
        draw.update(
            &harness.device,
            &harness.queue,
            &[quad("missing.png", 5, 2, 0, false, [1.; 4])],
            glam::Mat4::IDENTITY.to_cols_array_2d(),
            [0., 0., 0., 1.],
            [0., 0., 0., 100.],
            &mut read,
        )
        .unwrap();
        // A missing texture drops that group and is reported, never fatal: the
        // rest of the root still plays.
        assert_eq!(draw.order.len(), 0);
        assert_eq!(draw.failures(), ["missing.png: absent"]);
        assert_eq!(draw.vertex_count(), 0);

        let error = draw
            .update(
                &harness.device,
                &harness.queue,
                &[],
                [[f32::NAN; 4]; 4],
                [0., 0., 0., 1.],
                [0., 0., 0., 100.],
                &mut read,
            )
            .unwrap_err();
        assert!(error.contains("non-finite"), "{error}");
    }

    #[test]
    fn gpu_effect_grows_buffers_when_a_burst_arrives() {
        let Some(harness) = harness() else {
            eprintln!("effect growth test skipped: no WGPU adapter");
            return;
        };
        let mut draw = Draw::new(
            &harness.device,
            &harness.queue,
            wgpu::TextureFormat::Rgba8Unorm,
            None,
        )
        .unwrap();
        let png = grey_png();
        let mut read = |_: &str| Ok(png.clone());
        let projection = glam::Mat4::IDENTITY.to_cols_array_2d();
        draw.update(
            &harness.device,
            &harness.queue,
            &[quad("a.png", 5, 2, 0, false, [1.; 4])],
            projection,
            [0.; 4],
            [0.; 4],
            &mut read,
        )
        .unwrap();
        let small = draw.buckets.values().next().unwrap().capacity;
        let burst: Vec<EffectQuad> = (0..200)
            .map(|_| quad("a.png", 5, 2, 0, false, [1.; 4]))
            .collect();
        draw.update(
            &harness.device,
            &harness.queue,
            &burst,
            projection,
            [0.; 4],
            [0.; 4],
            &mut read,
        )
        .unwrap();
        let large = draw.buckets.values().next().unwrap().capacity;
        assert!(large >= 200 * 6, "{small} -> {large}");
        assert_eq!(draw.vertex_count(), 200 * 6);
        // Emptying the frame keeps the group but draws nothing.
        draw.update(
            &harness.device,
            &harness.queue,
            &[],
            projection,
            [0.; 4],
            [0.; 4],
            &mut read,
        )
        .unwrap();
        assert_eq!(draw.order.len(), 0);
        assert_eq!(draw.vertex_count(), 0);
    }

    #[test]
    fn grouping_order_is_deterministic_without_a_device() {
        // The sort key is pure, so the draw order can be checked headless.
        let pipeline = |source, destination, depth_test| PipelineKey {
            source,
            destination,
            depth_test,
        };
        let mut sorted = vec![
            BucketKey {
                layer: 1,
                pipeline: pipeline(5, 2, false),
                texture: "b".into(),
            },
            BucketKey {
                layer: 0,
                pipeline: pipeline(5, 6, true),
                texture: "a".into(),
            },
            BucketKey {
                layer: 0,
                pipeline: pipeline(5, 2, false),
                texture: "a".into(),
            },
        ];
        sorted.sort_by(|a, b| {
            a.layer
                .cmp(&b.layer)
                .then_with(|| a.pipeline.depth_test.cmp(&b.pipeline.depth_test).reverse())
                .then_with(|| a.pipeline.source.cmp(&b.pipeline.source))
                .then_with(|| a.pipeline.destination.cmp(&b.pipeline.destination))
                .then_with(|| a.texture.cmp(&b.texture))
        });
        assert!(sorted[0].pipeline.depth_test);
        assert_eq!(sorted[1].pipeline.destination, 2);
        assert_eq!(sorted[2].layer, 1);
        assert_eq!(blend_factor(5).unwrap(), wgpu::BlendFactor::SrcAlpha);
        assert_eq!(blend_factor(2).unwrap(), wgpu::BlendFactor::One);
        assert!(blend_factor(99).is_err());
    }
}
