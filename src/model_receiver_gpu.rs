//! Raster receiver for ordinary pre-posed model meshes. Not used for baked BMD,
//! terrain or the approved tree fallback. Callers must supply verified material,
//! fog, clipping and sampler inputs; this does not schedule ECM actions/effects.
//! Uses an encoded UNORM attachment so SRCALPHA blending happens in the client's
//! encoded space. An sRGB scene must expose a compatible non-sRGB attachment view.
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub fog: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Params {
    pub matrix: [[f32; 4]; 4],
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub point_ambient: [f32; 4],
    pub point_diffuse: [f32; 4],
    pub direction: [f32; 4],
    pub point_position: [f32; 4],
    pub attenuation: [f32; 4],
    pub eye_direction: [f32; 4],
    pub specular: [f32; 4],
    /// power, point enabled, specular variant enabled, reserved.
    pub lighting: [f32; 4],
    pub fog_color: [f32; 4],
    /// Table fog: start, end, enabled, reserved. Disabled uses supplied vertex fog.
    pub fog_range: [f32; 4],
    /// World clip plane; zero disables clipping.
    pub clip_plane: [f32; 4],
    /// Two sided, reverse front face, D3DCMPFUNC (0 disables), alpha reference.
    pub raster: [f32; 4],
}
pub struct Mesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    vertices: usize,
}
impl Mesh {
    pub fn new(
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> Result<Self, String> {
        if vertices.is_empty()
            || indices.is_empty()
            || indices.len() % 3 != 0
            || indices.iter().any(|&i| i as usize >= vertices.len())
        {
            return Err("invalid model receiver triangle data".into());
        }
        validate_vertices(vertices)?;
        Ok(Self {
            vertex: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("posed model receiver vertices"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            index: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("model receiver triangles"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: indices.len() as u32,
            vertices: vertices.len(),
        })
    }
    pub fn update(&self, queue: &wgpu::Queue, vertices: &[Vertex]) -> Result<(), String> {
        if vertices.len() != self.vertices {
            return Err("model receiver update changes vertex count".into());
        }
        validate_vertices(vertices)?;
        queue.write_buffer(&self.vertex, 0, bytemuck::cast_slice(vertices));
        Ok(())
    }
}
fn validate_vertices(vertices: &[Vertex]) -> Result<(), String> {
    if bytemuck::cast_slice::<Vertex, f32>(vertices)
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite model receiver vertex".into());
    }
    Ok(())
}
pub struct Binding {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}
impl Binding {
    pub fn update(&self, queue: &wgpu::Queue, params: &Params) -> Result<(), String> {
        validate_params(params)?;
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(params));
        Ok(())
    }
}
fn validate_params(params: &Params) -> Result<(), String> {
    if bytemuck::cast_slice::<Params, f32>(std::slice::from_ref(params))
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite model receiver constants".into());
    }
    Ok(())
}
pub struct Receiver {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}
impl Receiver {
    /// Ordinary MODULATE + SRCALPHA/INVSRCALPHA only. Other texture/shader/
    /// blend overrides must have their own verified pipeline, not be coerced here.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth: Option<(wgpu::TextureFormat, wgpu::CompareFunction)>,
        depth_write: bool,
    ) -> Result<Self, String> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err(
                "model receiver requires encoded UNORM attachment, not sRGB/linear blending".into(),
            );
        }
        let source = format!("{}\n{}", crate::model_light::WGSL, WGSL);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ordinary model receiving shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32];
        let buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        }];
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: blend,
                alpha: blend,
            }),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let depth_stencil = depth.map(|(format, depth_compare)| wgpu::DepthStencilState {
            format,
            depth_write_enabled: depth_write,
            depth_compare,
            stencil: Default::default(),
            bias: Default::default(),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("encoded ordinary model receiver"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil,
            multisample: Default::default(),
            multiview: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Ok(Self { pipeline, layout })
    }
    pub fn binding(
        &self,
        device: &wgpu::Device,
        texture: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        params: &Params,
    ) -> Result<Binding, String> {
        validate_params(params)?;
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model receiving material/light constants"),
            contents: bytemuck::bytes_of(params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Ok(Binding { uniform, bind })
    }
    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mesh: &'a Mesh,
        binding: &'a Binding,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &binding.bind, &[]);
        pass.set_vertex_buffer(0, mesh.vertex.slice(..));
        pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..mesh.count, 0, 0..1);
    }
}
pub const WGSL: &str = r#"
struct Params {
 matrix:mat4x4<f32>,ambient:vec4<f32>,diffuse:vec4<f32>,point_ambient:vec4<f32>,point_diffuse:vec4<f32>,
 direction:vec4<f32>,point_position:vec4<f32>,attenuation:vec4<f32>,eye_direction:vec4<f32>,specular:vec4<f32>,lighting:vec4<f32>,
 fog_color:vec4<f32>,fog_range:vec4<f32>,clip_plane:vec4<f32>,raster:vec4<f32>
};
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var image:texture_2d<f32>;
@group(0) @binding(2) var image_sampler:sampler;
struct Input {@location(0) position:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) fog:f32};
struct Output {@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) diffuse:vec4<f32>,@location(2) specular:vec3<f32>,@location(3) fog:f32,@location(4) world:vec3<f32>};
@vertex fn vs(v:Input)->Output {
 var o:Output;o.position=p.matrix*vec4<f32>(v.position,1.);o.world=v.position;o.uv=v.uv;o.fog=clamp(v.fog,0.,1.);o.specular=vec3<f32>(0.);
 if p.lighting.z>0.5 {
  o.diffuse=model_specular_variant_diffuse(v.position,v.normal,p.direction.xyz,p.point_position.xyz,p.attenuation.xyz,p.ambient,p.diffuse,p.point_diffuse,p.point_ambient,p.lighting.y);
  o.specular=model_directional_specular(v.normal,p.direction.xyz,p.eye_direction.xyz,p.lighting.x,p.specular.rgb);
 } else {
  o.diffuse=model_vertex_light_rgba(v.position,v.normal,p.direction.xyz,p.point_position.xyz,p.attenuation.xyz,p.ambient,p.diffuse,p.point_diffuse,p.point_ambient,p.lighting.y);
 }
 return o;
}
@fragment fn fs(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
 if p.raster.x<0.5 && front==(p.raster.y>0.5) {discard;}
 if dot(vec4<f32>(v.world,1.),p.clip_plane)<0. {discard;}
 let t=textureSample(image,image_sampler,v.uv);
 let alpha=clamp(t.a*v.diffuse.a,0.,1.);
 let reference=p.raster.w;
 var alpha_pass=true;
 switch u32(p.raster.z) {
  case 1u: {alpha_pass=false;}
  case 2u: {alpha_pass=alpha<reference;}
  case 3u: {alpha_pass=alpha==reference;}
  case 4u: {alpha_pass=alpha<=reference;}
  case 5u: {alpha_pass=alpha>reference;}
  case 6u: {alpha_pass=alpha!=reference;}
  case 7u: {alpha_pass=alpha>=reference;}
  default: {}
 }
 if !alpha_pass {discard;}
 let encoded=clamp(t.rgb*v.diffuse.rgb+v.specular,vec3<f32>(0.),vec3<f32>(1.));
 var fog=v.fog;
 if p.fog_range.z>0.5 {
  let depth=1./max(v.position.w,0.0000001);
  let span=p.fog_range.y-p.fog_range.x;
  if abs(span)<0.001 {fog=select(1.,0.,depth>=p.fog_range.y);}
  else {fog=clamp((p.fog_range.y-depth)/span,0.,1.);}
 }
 return vec4<f32>(mix(p.fog_color.rgb,encoded,fog),alpha);
}
"#;
