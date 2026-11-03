//! AutoFamily day/night sky sets and client BMP sky textures. Panorama projection
//! and celestial-disc geometry remain approximations, while set selection and
//! the one-second day/night resource transition follow the supplied client data.
use crate::{
    pck,
    sky_config::{AutoFogConfig, SkyMix, SkyTransition},
};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, fs::File, path::Path};
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    inv: [[f32; 4]; 4],
    time: [f32; 4],
    celestial: [f32; 4],
}
pub struct SkyGpu {
    pipeline: wgpu::RenderPipeline,
    normal: wgpu::BindGroup,
    reflected: wgpu::BindGroup,
    normal_buf: wgpu::Buffer,
    reflected_buf: wgpu::Buffer,
    transition: SkyTransition,
    auto_fog: Option<AutoFogConfig>,
}
pub(crate) const SHADER: &str = r#"
struct Params {inv:mat4x4<f32>,time:vec4<f32>,celestial:vec4<f32>};
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var skies:texture_2d_array<f32>;
@group(0) @binding(2) var smp:sampler;
struct O {@builtin(position) pos:vec4<f32>,@location(0) clip:vec2<f32>};
@vertex fn vs(@builtin(vertex_index) i:u32)->O {var o:O;let x=f32((i<<1u)&2u);let y=f32(i&2u);o.clip=vec2<f32>(x*2.-1.,1.-y*2.);o.pos=vec4<f32>(o.clip,0.999999,1.);return o;}
fn sky(dir:vec3<f32>, layer:i32)->vec3<f32>{
 let az=atan2(dir.x,-dir.z)/6.28318530718 + 0.5;
 let u=fract(az*2.);let side=i32(floor(az*2.))%2;
 let v=clamp(0.5-asin(clamp(dir.y,-1.,1.))/3.14159265,0.,1.);
 var a=textureSampleLevel(skies,smp,vec2<f32>(u,v),layer*3+side,0.).rgb;
 // The client panoramas do not line up perfectly when approximated as a
 // cylinder. Crossfade a narrow border to avoid a hard meridian seam.
 let other=1-side;
 if u<0.08 {let prev=textureSampleLevel(skies,smp,vec2<f32>(0.999,v),layer*3+other,0.).rgb;a=mix(prev,a,0.5+0.5*u/0.08);}
 if u>0.92 {let next=textureSampleLevel(skies,smp,vec2<f32>(0.001,v),layer*3+other,0.).rgb;a=mix(a,next,0.5*(u-0.92)/0.08);}
 let denom=max(dir.y,0.05);let roofuv=clamp(vec2<f32>(0.5)+dir.xz/(2.*denom),vec2<f32>(0.001),vec2<f32>(0.999));
 let b=textureSampleLevel(skies,smp,roofuv,layer*3+2,0.).rgb;
 return mix(a,b,smoothstep(0.70,0.97,dir.y));
}
@fragment fn fs(i:O)->@location(0) vec4<f32>{
 let v=p.inv*vec4<f32>(i.clip,1.,1.);let d=normalize(v.xyz/v.w);
 let hour=p.time.x;
 // `time.y/z/w` are the source set, destination set and native one-second
 // A3DSkySphere transition amount, resolved from AutoFamily sky.lst IDs.
 let color_from=sky(d,i32(p.time.y));let color_to=sky(d,i32(p.time.z));
 var color=mix(color_from,color_to,clamp(p.time.w,0.,1.));
 // Experimental sky-only celestial disc using client textures. Direction
 // follows the recovered sky clock and authored regional azimuth. Sizes and
 // exposure are provisional; material lighting and foliage are untouched.
 if p.celestial.w>0. {
  let c=normalize(p.celestial.xyz);
  let x=normalize(cross(vec3<f32>(0.,1.,0.),c)+vec3<f32>(0.00001,0.,0.));
  let y=cross(c,x);
  let z=dot(d,c);
  if z>0.98 && c.y>0.0 {
   let is_day=hour>=3.5 && hour<21.;
   let size=select(0.028,0.035,is_day);
   let uv=vec2<f32>(dot(d,x),-dot(d,y))/(2.*size*z)+vec2<f32>(0.5);
   if all(uv>=vec2<f32>(0.)) && all(uv<=vec2<f32>(1.)) {
    let disc=textureSampleLevel(skies,smp,uv,select(7,6,is_day),0.);
    let visibility=smoothstep(0.0,0.04,c.y)*p.celestial.w;
    if is_day {color=clamp(color+disc.rgb*visibility,vec3<f32>(0.),vec3<f32>(1.));}
    else {color=mix(color,disc.rgb,disc.a*visibility);}
   }
  }
 }
 return vec4<f32>(color,1.);
}
"#;
impl SkyGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        element: &Path,
    ) -> Result<Self, String> {
        let mut config_file = File::open(element.join("configs.pck")).map_err(|e| e.to_string())?;
        let config_entries: HashMap<_, _> = pck::directory(&mut config_file)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        let autoscene = config_entries
            .get("configs/autofamilyconfigs/autoscene.cfg")
            .ok_or("missing AutoFamily autoscene.cfg in configs.pck")?;
        let auto_bytes = pck::read_entry(&mut config_file, autoscene)?;
        let auto_config = crate::sky_config::parse_autoscene(&auto_bytes)?;
        let auto_fog = crate::sky_config::parse_autofog(&auto_bytes).ok();
        if auto_fog.is_none() {
            eprintln!("AutoFamily fog config could not be parsed; keeping viewer fog defaults");
        }
        let sky_list_key = auto_config.sky_list.to_ascii_lowercase();
        let sky_list = config_entries
            .get(&sky_list_key)
            .ok_or_else(|| format!("AutoFamily sky_list resource is missing: {sky_list_key}"))?;
        let sets =
            crate::sky_config::parse_sky_list(&pck::read_entry(&mut config_file, sky_list)?)?;
        let day = sets
            .iter()
            .find(|set| set.id == auto_config.day)
            .ok_or_else(|| {
                format!(
                    "AutoFamily day sky ID {} is absent from sky.lst",
                    auto_config.day
                )
            })?;
        let night = sets
            .iter()
            .find(|set| set.id == auto_config.night)
            .ok_or_else(|| {
                format!(
                    "AutoFamily night sky ID {} is absent from sky.lst",
                    auto_config.night
                )
            })?;

        let mut file = File::open(element.join("textures.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<_, _> = pck::directory(&mut file)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        let mut frames = Vec::new();
        // The three list paths are roof, panorama 01 and panorama 02. Keep the
        // GPU layer order expected by `sky()`: panorama 01, panorama 02, roof.
        for set in [day, night] {
            for relative in [&set.panorama_01, &set.panorama_02, &set.roof] {
                let key = format!("textures/sky/{}", relative.to_lowercase());
                let entry = entries
                    .get(&key)
                    .ok_or_else(|| format!("missing AutoFamily sky texture {key}"))?;
                let bytes = pck::read_entry(&mut file, entry)?;
                let img = image::load_from_memory(&bytes)
                    .map_err(|e| format!("{key}: {e}"))?
                    .to_rgba8();
                let img = if img.width() != 512 || img.height() != 512 {
                    image::imageops::resize(&img, 512, 512, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                frames.push(img);
            }
        }
        // Extra layers are client sun and moon; absent optional assets remain
        // transparent so the experimental toggle cannot prevent world loading.
        for (name, format) in [
            ("textures/nature/sun.bmp", image::ImageFormat::Bmp),
            ("textures/nature/moon.tga", image::ImageFormat::Tga),
        ] {
            let image = entries
                .get(name)
                .and_then(|entry| pck::read_entry(&mut file, entry).ok())
                .and_then(|bytes| image::load_from_memory_with_format(&bytes, format).ok())
                .map(|img| {
                    image::imageops::resize(
                        &img.to_rgba8(),
                        512,
                        512,
                        image::imageops::FilterType::Triangle,
                    )
                })
                .unwrap_or_else(|| image::RgbaImage::new(512, 512));
            frames.push(image);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("client sky images"),
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 8,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        for (i, img) in frames.iter().enumerate() {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: i as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                img,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(2048),
                    rows_per_image: Some(512),
                },
                wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let make_buf = || {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sky camera"),
                contents: bytemuck::bytes_of(&Uniform {
                    inv: Mat4::IDENTITY.to_cols_array_2d(),
                    time: [12., 0., 0., 0.],
                    celestial: [0., 1., 0., 0.],
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let normal_buf = make_buf();
        let reflected_buf = make_buf();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
            ],
        });
        let bind = |b: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky material"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: b.as_entire_binding(),
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
            })
        };
        let normal = bind(&normal_buf);
        let reflected = bind(&reflected_buf);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("client sky"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&pl),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        Ok(Self {
            pipeline,
            normal,
            reflected,
            normal_buf,
            reflected_buf,
            transition: SkyTransition::default(),
            auto_fog,
        })
    }
    pub fn auto_fog(&self) -> Option<AutoFogConfig> {
        self.auto_fog
    }
    pub fn update_transition(&mut self, night_factor: f32, delta_ms: u32) -> SkyMix {
        self.transition.update(night_factor, delta_ms)
    }

    pub fn update(
        &self,
        queue: &wgpu::Queue,
        direction: Vec3,
        w: u32,
        h: u32,
        hour: f32,
        celestial_direction: [f32; 3],
        celestial_alpha: f32,
        mix: SkyMix,
        reflected: bool,
    ) {
        let view = Mat4::look_to_rh(Vec3::ZERO, direction, Vec3::Y);
        let projection = Mat4::from_scale(Vec3::new(-1., 1., 1.))
            * Mat4::perspective_rh(70f32.to_radians(), w as f32 / h.max(1) as f32, 1., 16000.);
        // Reflect every world-space camera ray about the water plane. Mirroring
        // only the forward vector loses the inverted vertical screen basis.
        let inv = sky_inverse(projection, view, reflected);
        let uniform = Uniform {
            inv: inv.to_cols_array_2d(),
            time: [hour, mix.from as f32, mix.to as f32, mix.amount],
            celestial: [
                celestial_direction[0],
                celestial_direction[1],
                celestial_direction[2],
                celestial_alpha,
            ],
        };
        queue.write_buffer(
            if reflected {
                &self.reflected_buf
            } else {
                &self.normal_buf
            },
            0,
            bytemuck::bytes_of(&uniform),
        );
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, reflected: bool) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(
            0,
            if reflected {
                &self.reflected
            } else {
                &self.normal
            },
            &[],
        );
        pass.draw(0..3, 0..1);
    }
}

fn sky_inverse(projection: Mat4, view: Mat4, reflected: bool) -> Mat4 {
    let inv = (projection * view).inverse();
    if reflected {
        Mat4::from_scale(Vec3::new(1., -1., 1.)) * inv
    } else {
        inv
    }
}
#[cfg(test)]
mod reflection_tests {
    use super::*;
    #[test]
    fn sky_rays_follow_horizontally_corrected_camera_and_reflections() {
        let view = Mat4::look_to_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let proj = Mat4::from_scale(Vec3::new(-1., 1., 1.))
            * Mat4::perspective_rh(70f32.to_radians(), 1., 1., 100.);
        let ray = sky_inverse(proj, view, false) * glam::Vec4::new(0.5, 0.3, 1., 1.);
        let reflected = sky_inverse(proj, view, true) * glam::Vec4::new(0.5, 0.3, 1., 1.);
        assert!(ray.x > 0. && ray.y > 0. && ray.z > 0.);
        assert!(
            (ray.x - reflected.x).abs() < 1e-6
                && (ray.y + reflected.y).abs() < 1e-6
                && (ray.z - reflected.z).abs() < 1e-6
        );
    }
    #[test]
    fn reflection_mirrors_entire_ray_basis() {
        let projection = Mat4::perspective_rh(70f32.to_radians(), 1.5, 1., 100.);
        let direction = Vec3::new(0.3, 0.2, -1.).normalize();
        let view = Mat4::look_to_rh(Vec3::ZERO, direction, Vec3::Y);
        for (x, y) in [(-0.4, -0.7), (0.8, 0.3), (0., 0.)] {
            let p = glam::Vec4::new(x, y, 1., 1.);
            let n = sky_inverse(projection, view, false) * p;
            let r = sky_inverse(projection, view, true) * p;
            assert!(
                (n.x - r.x).abs() < 1e-5 && (n.y + r.y).abs() < 1e-5 && (n.z - r.z).abs() < 1e-5
            );
        }
    }
}
