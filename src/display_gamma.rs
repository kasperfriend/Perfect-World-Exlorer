//! Whole-frame emulation of the client's fullscreen D3D gamma ramp.
//! Runs after world, minimap and egui are composed; never alters the desktop.
#[path = "gamma_curve.rs"]
mod gamma_curve;
use bytemuck::{Pod, Zeroable};
pub use gamma_curve::enabled;
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    setting: f32,
    srgb: f32,
    _pad: [f32; 2],
    ramp: [[f32; 4]; 64],
}
const SHADER: &str = r#"
struct Params {setting:f32,srgb:f32,pad:vec2<f32>,ramp:array<vec4<f32>,64>};
@group(0) @binding(0) var frame:texture_2d<f32>;
@group(0) @binding(1) var<uniform> p:Params;
@vertex fn vs(@builtin(vertex_index) id:u32)->@builtin(position) vec4<f32> {
 let x=f32((id<<1u)&2u);let y=f32(id&2u);
 return vec4<f32>(x*2.-1.,1.-y*2.,0.,1.);
}
@fragment fn fs(@builtin(position) pos:vec4<f32>)->@location(0) vec4<f32> {
 let c=textureLoad(frame,vec2<i32>(pos.xy),0).rgb;
 var out=client_gamma_ramp(c);
 if p.srgb>0.5 {out=select(out/12.92,pow((out+0.055)/1.055,vec3<f32>(2.4)),out>vec3<f32>(0.04045));}
 return vec4<f32>(out,1.);
}
"#;
pub struct DisplayGamma {
    format: wgpu::TextureFormat,
    frame: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}
impl DisplayGamma {
    fn frame(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> wgpu::Texture {
        let alternate = [format.remove_srgb_suffix()];
        let views = if format.is_srgb() {
            &alternate[..]
        } else {
            &[]
        };
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fullscreen frame before display gamma"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: views,
        })
    }
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> Self {
        let frame = Self::frame(device, format, w, h);
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("display gamma setting"),
            contents: bytemuck::bytes_of(&Params {
                setting: 41.,
                srgb: if format.is_srgb() { 1. } else { 0. },
                _pad: [0.; 2],
                ramp: gamma_curve::table(41.),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display gamma layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen gamma ramp"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", gamma_curve::WGSL, SHADER).into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display gamma pipeline"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fullscreen gamma composite"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[],
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
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        });
        Self {
            format,
            frame,
            pipeline,
            layout,
            uniform,
        }
    }
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        self.frame = Self::frame(device, self.format, w, h);
    }
    pub fn view(&self) -> wgpu::TextureView {
        self.frame.create_view(&Default::default())
    }
    pub fn raw_view(&self) -> wgpu::TextureView {
        self.frame.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format.remove_srgb_suffix()),
            ..Default::default()
        })
    }
    pub fn present(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        setting: f32,
    ) {
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Params {
                setting: gamma_curve::setting(setting),
                srgb: if self.format.is_srgb() { 1. } else { 0. },
                _pad: [0.; 2],
                ramp: gamma_curve::table(setting),
            }),
        );
        let raw = self.frame.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format.remove_srgb_suffix()),
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display gamma source"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&raw),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("whole-frame display gamma"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
