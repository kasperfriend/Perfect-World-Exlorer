//! Native day/night sky dome: recovered geometry, projection and sky clock.
//!
//! The mesh tables, the texture-set switch and the celestial billboards all come
//! from `elementclient.exe` and are unit tested in [`crate::sky_geometry`],
//! [`crate::sky_time`] and [`crate::sky_celestial`]; this module only uploads
//! them. Nothing here invents a blend: the panorama seam is a hard switch
//! between the front and back texture, the band the native buffers leave open
//! (38.16-72.5 degrees) is filled with the pass background at the client's own
//! rim alpha, and the day/night change is the client's linear switch timer.
//! Two viewer-only compositions keep that open band from reading as a ring: the
//! background is the frame's atmospheric fog colour rather than a fixed blue,
//! and the roof disc's rim fades into it over
//! [`crate::sky_geometry::CAP_RIM_FADE_DEGREES`]
//! because retail draws an unidentified object from `[A3DSky+0x54]`
//! (`0x799f88`, vtable `+0x1c`) over that edge and this build does not.
use crate::pck;
use glam::{Mat4, Vec3};
use std::{collections::HashMap, fs::File, path::Path};
use wgpu::util::DeviceExt;

/// Per-frame sky inputs, all recovered values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    /// Authored hour, `0..24`. Used for diagnostics only.
    pub hour: f32,
    /// Day/night texture-set weight from [`crate::sky_time::night_weight`].
    pub night_weight: f32,
    /// Sun: `[fade, elevation, azimuth, unused]`.
    pub sun: [f32; 4],
    /// Moon: `[fade, elevation, azimuth, unused]`.
    pub moon: [f32; 4],
    /// Colour the pass shows where neither native buffer covers the sky: the
    /// 38.16-72.5 degree band the two meshes leave open. The retail client fills
    /// that band with an object this build does not draw (0x799f88), so the
    /// frame's own atmospheric fog colour is used instead of a fixed blue.
    pub background: [f32; 3],
}

impl Default for SkyFrame {
    fn default() -> Self {
        Self {
            hour: 12.0,
            night_weight: 0.0,
            sun: [0.0; 4],
            moon: [0.0; 4],
            background: [0.18, 0.32, 0.48],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    inv: [[f32; 4]; 4],
    time: [f32; 4],
    celestial: [f32; 4],
    celestial2: [f32; 4],
    background: [f32; 4],
}

pub struct SkyGpu {
    pipeline: wgpu::RenderPipeline,
    normal: wgpu::BindGroup,
    reflected: wgpu::BindGroup,
    normal_buf: wgpu::Buffer,
    reflected_buf: wgpu::Buffer,
    /// Whether `sun.bmp` and `moon.tga` decoded out of the package. A missing
    /// disc texture leaves the layer transparent, so the billboard is submitted
    /// and still invisible; the settings tab reports that instead of the disc
    /// silently disappearing.
    disc_layers: [bool; 2],
}

pub(crate) const SHADER: &str = r#"
struct Params {inv:mat4x4<f32>,time:vec4<f32>,celestial:vec4<f32>,celestial2:vec4<f32>,background:vec4<f32>};
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var skies:texture_2d_array<f32>;
@group(0) @binding(2) var smp:sampler;
struct O {@builtin(position) pos:vec4<f32>,@location(0) clip:vec2<f32>};
@vertex fn vs(@builtin(vertex_index) i:u32)->O {var o:O;let x=f32((i<<1u)&2u);let y=f32(i&2u);o.clip=vec2<f32>(x*2.-1.,1.-y*2.);o.pos=vec4<f32>(o.clip,0.999999,1.);return o;}

// Curtain rows and their two texture runs, A3DSkySphere::Init 0x79a330.
fn row_y(i:i32)->f32{
 if i==0 {return 39.2700005;}
 if i==1 {return 19.6350002;}
 if i==2 {return 6.5580897;}
 if i==3 {return 0.0;}
 return -10.6029005;
}
fn row_v(i:i32,front:bool)->f32{
 if i>=4 {return 0.5;}
 if front {
  if i==0 {return 0.05;}
  if i==1 {return 0.275;}
  if i==2 {return 0.42485;}
  return 0.5;
 }
 if i==0 {return 0.95;}
 if i==1 {return 0.725;}
 if i==2 {return 0.57515;}
 return 0.5;
}
fn curtain_v(y:f32,front:bool)->f32{
 if y>=39.2700005 {return row_v(0,front);}
 for (var i:i32=0;i<4;i=i+1) {
  let y0=row_y(i);let y1=row_y(i+1);
  if y>=y1 {return mix(row_v(i,front),row_v(i+1,front),(y-y0)/(y1-y0));}
 }
 return row_v(4,front);
}
// Native vertex alpha: opaque from the second row down, zero on the rim row.
fn curtain_alpha(y:f32)->f32{
 if y>=39.2700005 {return 0.0;}
 if y<=19.6350002 {return 1.0;}
 return (39.2700005-y)/19.6350002;
}
fn sample_sky(d:vec3<f32>,base:i32)->vec4<f32>{
 // Cap: sphere of radius 50, planar disc over the top 17.5 degrees
 // (A3DSkySphere::Init 0x79a810).
 if d.y*50.0>=47.685848 {
  let uv=vec2<f32>(0.5,0.5)+vec2<f32>(d.x,d.z)*1.66275358;
  // Viewer-only rim softening: the native disc is opaque up to its 72.5 degree
  // rim, where the mesh stops and the open band begins. Retail covers that hard
  // circle with an object drawn at 0x799f88 that this build does not render, so
  // the rim fades into the pass background from 72.5 to 82 degrees instead
  // (sin of both, matching sky_geometry::cap_alpha).
  return vec4<f32>(textureSampleLevel(skies,smp,uv,base+2,0.).rgb,smoothstep(0.953717,0.990268,d.y));
 }
 // Curtain: cylinder of radius 50, so the ray meets it at 50*tan(elevation).
 // One 180 degree half per draw, the second draw mirrors z; the island split
 // at azimuth 90 reverses the texture run, it never blends.
 let y=50.0*d.y/max(length(d.xz),0.000001);
 let front=d.z>=0.0;
 let az=atan2(abs(d.z),d.x);
 let quarter=1.5707963;
 let u=select(2.0-2.0*az/3.14159265,1.0-2.0*az/3.14159265,az<quarter);
 let sy=min(y,39.2700005);
 let uv=vec2<f32>(clamp(u,0.0,1.0),curtain_v(sy,az>=quarter));
 let tex=textureSampleLevel(skies,smp,uv,base+select(1,0,front),0.);
 return vec4<f32>(tex.rgb,tex.a*curtain_alpha(sy));
}
// One native billboard: centre at direction*100, half extent 1.0, so the disc
// subtends 1.146 degrees. Gated on elevation and fade exactly like 0x4b3e00.
fn celestial(color:vec3<f32>,d:vec3<f32>,params:vec4<f32>,layer:i32)->vec3<f32>{
 let alpha=params.x;let el=params.y;let az=params.z;
 if alpha<=0.0 || el<=0.0 {return color;}
 let c=vec3<f32>(cos(az)*cos(el),sin(el),sin(az)*cos(el));
 let z=dot(d,c);
 if z<=0.0 {return color;}
 let right=normalize(cross(vec3<f32>(0.,1.,0.),c)+vec3<f32>(0.000001,0.,0.));
 let up=cross(c,right);
 let uv=vec2<f32>(dot(d,right),-dot(d,up))/(2.0*0.01*z)+vec2<f32>(0.5);
 if uv.x<0.0 || uv.x>1.0 || uv.y<0.0 || uv.y>1.0 {return color;}
 let t=textureSampleLevel(skies,smp,uv,layer,0.);
 if layer==9 {return clamp(color+t.rgb*alpha,vec3<f32>(0.),vec3<f32>(1.));}
 return mix(color,t.rgb,t.a*alpha);
}
@fragment fn fs(i:O)->@location(0) vec4<f32>{
 let v=p.inv*vec4<f32>(i.clip,1.,1.);let d=normalize(v.xyz/v.w);
 let w=clamp(p.time.y,0.,1.);
 // Day set is layers 0..2, night set 6..8 (`01.bmp`, `02.bmp`, `Roof.bmp`).
 let day=sample_sky(d,0);let night=sample_sky(d,6);
 var color=mix(day.rgb,night.rgb,w);
 // The pass clears to this background and the sky rim alpha fades into it,
 // which is how the client hides the band its two buffers leave open.
 color=mix(p.background.rgb,color,clamp(mix(day.a,night.a,w),0.,1.));
 color=celestial(color,d,p.celestial,9);
 color=celestial(color,d,p.celestial2,10);
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
        let mut file = File::open(element.join("textures.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<_, _> = pck::directory(&mut file)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        // Three sky phases ship in the client config (`sky_image01..03`); the
        // recovered day/night rule switches between the first and the last.
        let names = ["clear", "evening", "night2"];
        let mut frames = Vec::new();
        for folder in names {
            for name in ["01.bmp", "02.bmp", "roof.bmp"] {
                let key = format!("textures/sky/{folder}/{name}");
                let entry = entries
                    .get(&key)
                    .ok_or_else(|| format!("missing client sky texture {key}"))?;
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
        // Extra layers are the client sun and moon discs; absent optional assets
        // stay transparent so the world still loads.
        let disc_textures = [
            ("textures/nature/sun.bmp", image::ImageFormat::Bmp),
            ("textures/nature/moon.tga", image::ImageFormat::Tga),
        ];
        let mut disc_layers = [false; 2];
        for (i, (name, format)) in disc_textures.into_iter().enumerate() {
            let decoded = entries
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
                });
            disc_layers[i] = decoded.is_some();
            frames.push(decoded.unwrap_or_else(|| image::RgbaImage::new(512, 512)));
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("client sky images"),
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 11,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        for (i, img) in frames.iter().enumerate() {
            // Clamp to edge horizontally and vertically except along u, where
            // the client relies on the texture run ending exactly at the seam.
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
            label: Some("sky panorama sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
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
                    celestial: [0.; 4],
                    celestial2: [0.; 4],
                    background: [0.18, 0.32, 0.48, 1.],
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
            disc_layers,
        })
    }
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        direction: Vec3,
        w: u32,
        h: u32,
        frame: SkyFrame,
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
            time: [frame.hour, frame.night_weight, 0., 0.],
            celestial: frame.sun,
            celestial2: frame.moon,
            background: [
                frame.background[0],
                frame.background[1],
                frame.background[2],
                1.,
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
    /// Whether the client disc texture for `sun` decoded from `textures.pck`.
    pub fn disc_texture_present(&self, sun: bool) -> bool {
        self.disc_layers[usize::from(!sun)]
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
    #[test]
    fn sky_frame_defaults_to_the_recovered_day_state() {
        let frame = SkyFrame::default();
        assert_eq!(frame.hour, 12.0);
        assert_eq!(frame.night_weight, 0.0);
        assert_eq!(crate::sky_time::night_weight(frame.hour), Some(0.0));
    }
}
