//! Native day/night sky dome: recovered geometry, projection and sky clock.
//!
//! The mesh tables, the texture-set switch and the celestial billboards all come
//! from `elementclient.exe` and are unit tested in [`crate::sky_geometry`],
//! [`crate::sky_time`] and [`crate::sky_celestial`]; this module only uploads
//! them. The recovered parts are the mesh rows and their texture runs, the
//! planar cap disc, the sun/moon billboards and the sky clock.
//! Retail covers the band the two native buffers leave open (38.16-72.5
//! degrees) with an object this build does not draw
//! (`[A3DSky+0x54]`, `0x799f88`, vtable `+0x1c`), so the band is a gradient from
//! the frame's atmospheric fog colour to the cap's own rim colour at 72.5
//! degrees: both ends equal the neighbouring surface exactly, which is why no
//! ring or disc edge is visible.
//!
//! One authored set covers both phases. `element_client.cfg` names `Sky\clear`
//! for all three `sky_image` slots, so the day/night change is the recovered
//! regional light colour modulating that one set, not a second texture set.
//! The two texture runs the island split switches between crossfade over 3.4
//! degrees instead of meeting at a hard seam.
use crate::pck;
use glam::{Mat4, Vec3};
use std::{collections::HashMap, fs::File, path::Path};
use wgpu::util::DeviceExt;

/// Viewer disc scale. The recovered billboards subtend 1.146 degrees, which is
/// what the client draws, but the retail sun and moon read larger because of the
/// sunshine/flare pass this build does not have (`sunshine.bmp`,
/// `moonflare.bmp`, `sunflare.bmp`, `ps/solarflare_14`). 1 restores the native
/// size; the shipped 2 doubles the quad and its glow.
pub const DISC_SCALE: f32 = 2.0;

/// Per-frame sky inputs, all recovered values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    /// Authored hour, `0..24`. Used for diagnostics only.
    pub hour: f32,
    /// Day/night switch weight from [`crate::sky_time::night_weight`]. Kept for
    /// diagnostics; the shader uses [`Self::tint`] because one authored set now
    /// covers both phases.
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
    /// Colour multiplier applied to the sky texture samples: the recovered
    /// regional light colour, so the single authored set darkens through dusk
    /// and night instead of swapping to a second texture set.
    pub tint: [f32; 3],
}

impl Default for SkyFrame {
    fn default() -> Self {
        Self {
            hour: 12.0,
            night_weight: 0.0,
            sun: [0.0; 4],
            moon: [0.0; 4],
            background: [0.18, 0.32, 0.48],
            tint: [1.0; 3],
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
    tint: [f32; 4],
    /// `x` committed set, `y` incoming set, `z` incoming weight, `w` hour. The
    /// set crossfade of `sky_transition` rides here (P5.07): both indices name
    /// one whole three-texture set in the same array.
    sets: [f32; 4],
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
struct Params {inv:mat4x4<f32>,time:vec4<f32>,celestial:vec4<f32>,celestial2:vec4<f32>,background:vec4<f32>,tint:vec4<f32>,sets:vec4<f32>};
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
fn sample_sky(d:vec3<f32>,base:i32,bg:vec3<f32>,tint:vec3<f32>)->vec4<f32>{
 let tn=clamp(tint,vec3<f32>(0.0),vec3<f32>(1.0));
 // Cap: sphere of radius 50, planar disc over the top 17.5 degrees
 // (A3DSkySphere::Init 0x79a810).
 if d.y*50.0>=47.685848 {
  let uv=vec2<f32>(0.5,0.5)+vec2<f32>(d.x,d.z)*1.66275358;
  return vec4<f32>(textureSampleLevel(skies,smp,uv,base+2,0.).rgb*tn,1.0);
 }
 let flat=max(length(d.xz),0.000001);
 // Curtain: cylinder of radius 50, so the ray meets it at 50*tan(elevation).
 let y=50.0*d.y/flat;
 // 38.16..72.5 degrees has no native geometry. Retail covers that band with an
 // object this build does not draw (0x799f88), so the cap's own planar lookup
 // is continued outward (radius 0.5 on the 72.5 degree rim, 0.75 at the
 // curtain rim) and faded into the pass background: the band meets the opaque
 // cap exactly at its top and the curtain's own fade at its bottom, and because
 // the radius sweeps instead of holding one circle it cannot smear radially.
 if y>39.2700005 {
  let t=smoothstep(0.618,0.9537,d.y);
  let radius=0.5+0.25*(1.0-t);
  let uv=vec2<f32>(0.5)+d.xz/flat*radius;
  let band=textureSampleLevel(skies,smp,uv,base+2,0.).rgb*tn;
  return vec4<f32>(band,smoothstep(0.618,0.70,d.y));
 }
 // The curtain draw passes two textures per set (01 and 02), so both cover the
 // dome and their stage blend is not recovered. The viewer keeps one as the
 // primary per half space, so each side still shows real art at full contrast,
 // and crossfades 3.4 degrees of azimuth at the azimuth-90 run switch and 7
 // degrees at the mirror plane where the two textures would meet in a hard
 // vertical line.
 let az=atan2(abs(d.z),d.x);
 let quarter=1.5707963;
 let sy=min(y,39.2700005);
 let owns_front=az>=quarter;
 let near=1.0-smoothstep(0.0,0.06,abs(az-quarter));
 let uf=clamp(2.0-2.0*az/3.14159265,0.0,1.0);
 let ub=clamp(1.0-2.0*az/3.14159265,0.0,1.0);
 let vf=curtain_v(sy,true);
 let vb=curtain_v(sy,false);
 let uv_own=vec2<f32>(select(ub,uf,owns_front),select(vb,vf,owns_front));
 let uv_other=vec2<f32>(select(uf,ub,owns_front),select(vf,vb,owns_front));
 let a0=textureSampleLevel(skies,smp,uv_own,base,0.);
 let a1=textureSampleLevel(skies,smp,uv_own,base+1,0.);
 let b0=textureSampleLevel(skies,smp,uv_other,base,0.);
 let b1=textureSampleLevel(skies,smp,uv_other,base+1,0.);
 let run0=mix(a0,b0,0.5*near);
 let run1=mix(a1,b1,0.5*near);
 let plane=1.0-smoothstep(0.0,0.12,abs(d.z));
 let front=d.z>=0.0;
 let tex=mix(select(run1,run0,front),select(run0,run1,front),0.5*plane);
 return vec4<f32>(tex.rgb*tn,tex.a*curtain_alpha(sy));
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
 // p.tint.w is the viewer disc scale (1 is the native 1.146 degree quad).
 let scale=max(p.tint.w,0.25);
 let uv=vec2<f32>(dot(d,right),-dot(d,up))/(2.0*0.01*scale*z)+vec2<f32>(0.5);
 if uv.x<0.0 || uv.x>1.0 || uv.y<0.0 || uv.y>1.0 {return color;}
 let t=textureSampleLevel(skies,smp,uv,layer,0.);
 if layer==9 {return clamp(color+t.rgb*alpha,vec3<f32>(0.),vec3<f32>(1.));}
 return mix(color,t.rgb,t.a*alpha);
}
@fragment fn fs(i:O)->@location(0) vec4<f32>{
 let v=p.inv*vec4<f32>(i.clip,1.,1.);let d=normalize(v.xyz/v.w);
 // One authored set covers both phases (`sky_image01..03` in
 // element_client.cfg name Sky\clear for all three); the night look is the
 // recovered regional light colour, not a second texture set. Layers 0..2 are
 // that set, 9 and 10 the client sun and moon discs.
 // The set indices arrive as 0..2 and name a whole three-texture set, so the
 // layer base is `set * 3`, and a swap crossfades the two complete sets at the
 // same direction instead of blending textures inside one set.
 let base=i32(clamp(p.sets.x,0.,2.))*3;
 let incoming=i32(clamp(p.sets.y,0.,2.))*3;
 let swap=clamp(p.sets.z,0.,1.);
 var sky=sample_sky(d,base,p.background.rgb,p.tint.rgb);
 if swap>0. && incoming!=base {
  let other=sample_sky(d,incoming,p.background.rgb,p.tint.rgb);
  sky=vec4<f32>(mix(sky.rgb,other.rgb,swap),mix(sky.a,other.a,swap));
 }
 var color=mix(p.background.rgb,sky.rgb,clamp(sky.a,0.,1.));
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
        // `element_client.cfg` names `Sky\clear` for all three `sky_image`
        // slots, so the authored set is loaded once and the shader uses it for
        // both phases. The extra folders stay loaded so the layer indices (0..2
        // set, 9 sun, 10 moon) do not move.
        // One authored set covers both phases: `element_client.cfg` names
        // `Sky\clear` for all three `sky_image` slots, and the packed
        // `night2/01.bmp` carries a painted moon that would sit next to the
        // recovered billboard. The three groups stay so the `sky_transition`
        // swap lane keeps real layer indices; they are the same art, so a swap
        // is a visual no-op until a second authored set exists.
        let names = ["clear", "clear", "clear"];
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
                    tint: [1.; 4],
                    // Committed set 0 with weight zero: the shader leaves the
                    // hour blend untouched until `begin_frame` uploads a frame.
                    sets: [0., 0., 0., 12.],
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
    /// Upload one frame's sky state for one view.
    ///
    /// Every argument comes from the single per-frame snapshot. `reflected`
    /// only selects which uniform buffer holds it; a reflected view must never
    /// pass a different hour, set or blend (P5.08). `sets` is
    /// `(committed, incoming, weight)` from `sky_transition::frame_sets`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        direction: Vec3,
        w: u32,
        h: u32,
        frame: SkyFrame,
        sets: (i32, i32, f32),
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
            tint: [
                frame.tint[0],
                frame.tint[1],
                frame.tint[2],
                // `p.tint.w` carries the disc scale into `celestial`.
                DISC_SCALE,
            ],
            sets: [sets.0 as f32, sets.1 as f32, sets.2, frame.hour],
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

/// Inverse of the sky view-projection, without any eye translation.
///
/// `A3DSkySphere` recentres its vertices on the eye and only compares a cached
/// position/radius tuple (`0x79acbd..0x79acd4`), so altitude reaches the sphere
/// placement but never its ray directions. Dropping the translation here makes
/// that structural rather than incidental: the camera position cannot leak into
/// the sky, and a reflected view is the same ray mirrored about the water plane.
fn sky_inverse(projection: Mat4, view: Mat4, reflected: bool) -> Mat4 {
    let mut rotation = view;
    rotation.w_axis = glam::Vec4::W;
    let inv = (projection * rotation).inverse();
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
    fn ray(projection: Mat4, view: Mat4, clip: glam::Vec2, reflected: bool) -> Vec3 {
        let v = sky_inverse(projection, view, reflected) * glam::Vec4::new(clip.x, clip.y, 1., 1.);
        (v.truncate() / v.w).normalize()
    }
    #[test]
    fn altitude_never_reaches_the_sky() {
        // A3DSkySphere recentres on the eye and only compares a cached
        // position/radius tuple (0x79acbd..0x79acd4), so the sky follows the
        // camera in X/Z/Y but its ray directions cannot depend on altitude
        // (P5.08). Dropping the view translation makes that structural.
        let projection = Mat4::from_scale(Vec3::new(-1., 1., 1.))
            * Mat4::perspective_rh(70f32.to_radians(), 16. / 9., 1., 16000.);
        let direction = Vec3::new(0.2, 0.35, -1.).normalize();
        for clip in [
            glam::Vec2::new(0., 0.),
            glam::Vec2::new(-0.7, 0.4),
            glam::Vec2::new(0.9, -0.85),
        ] {
            let reference = ray(
                projection,
                Mat4::look_to_rh(Vec3::new(512., 0., -2048.), direction, Vec3::Y),
                clip,
                false,
            );
            for altitude in [-2048., -12., 0., 160., 4096., 20_000.] {
                let lifted = ray(
                    projection,
                    Mat4::look_to_rh(Vec3::new(512., altitude, -2048.), direction, Vec3::Y),
                    clip,
                    false,
                );
                assert!(
                    (reference - lifted).length() < 1e-6,
                    "altitude {altitude} moved the sky ray at {clip:?}"
                );
            }
            // Horizontal movement must not move it either: only rotation matters.
            let moved = ray(
                projection,
                Mat4::look_to_rh(Vec3::new(-900., 0., 1200.), direction, Vec3::Y),
                clip,
                false,
            );
            assert!((reference - moved).length() < 1e-6);
        }
    }
    #[test]
    fn the_uniform_carries_the_transition_lane() {
        // The reflected and normal uploads must be built from the same values;
        // the shader reads sets.xyz for the sky swap.
        let src = include_str!("sky_gpu.rs");
        let update = &src[src.find("pub fn update(").expect("update")..];
        let update = &update[..update.find("pub fn draw<").expect("draw")];
        assert_eq!(update.matches("sets: [sets.0 as f32").count(), 1);
        // The translation drop lives in `sky_inverse`, not in `update`.
        assert!(src.contains("rotation.w_axis = glam::Vec4::W"));
        assert!(
            src.contains("p.sets.z"),
            "the shader must read the sets lane"
        );
        // A weather lane would need an authored runtime (P5.05); the shader must
        // not sample anything for weather until that exists.
        let shader = &src[src.find("pub(crate) const SHADER").expect("shader")..];
        let shader = &shader[..shader.find("\"#;").expect("shader end")];
        assert!(
            !shader.contains("p.weather"),
            "no weather uniform until an authored runtime exists (P5.05)"
        );
    }
}
