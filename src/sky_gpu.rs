//! Native day/night sky dome: recovered geometry, projection and sky clock.
//!
//! The mesh tables, the texture-set switch and the celestial billboards all come
//! from `elementclient.exe` and are unit tested in [`crate::sky_geometry`],
//! [`crate::sky_time`] and [`crate::sky_celestial`]; this module only uploads
//! them. The recovered parts are the mesh rows and their texture runs, the
//! planar cap disc, the sun/moon billboards and the sky clock.
//! The band the two native buffers leave open (38.16-72.5 degrees) is filled by
//! continuing the cap's own recovered planar lookup over the whole upper
//! hemisphere. That is the one fill that cannot produce an artifact: it is the
//! surface the cap already draws, evaluated outside the 17.5 degree cap disc,
//! and `clear\roof.bmp` is authored to tile (opposite borders differ by less
//! than one pixel step), so the repeat sampler carries it past one tile exactly
//! where the client's wrap addressing would. Retail draws *something* there
//! with the same texture (`[A3DSky+0x54]`, `0x799f88`, vtable `+0x1c`) and that
//! object is still unidentified; using the cap's projection for it is a
//! viewer-only composition and is labelled as such in the settings status.
//!
//! The curtain keeps its authored rim fade (`alpha` 0 at 39.27, 1 at 19.64) and
//! is composited *over* the roof sheet, so the skirt fades into the sky instead
//! of into the pass clear colour. Earlier fills faded the band into that
//! background, which is what drew a dark ring under the panorama rim.
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

/// Viewer shine quad for the sun, as a multiple of the native billboard's half
/// extent. `CECSunMoon::Init` builds a separate sun-shine stream over
/// `sunshine.bmp` and a flare chain over `moonflare.bmp` / `sunflare.bmp`, and
/// that chain is what makes the retail sun and moon haloes read large; neither
/// quad size is recovered (P5.03 section 3.2), so the viewer draws the sun's
/// `sunshine.bmp` on a quad this many times the recovered billboard. The moon
/// has no shine in the client's render block, so `moonflare.bmp` is loaded and
/// left for the flare chain (`moon.w` is 0). The disc itself stays at
/// [`DISC_SCALE`] x 1.146 degrees.
pub const GLOW_SCALE: f32 = 12.0;

/// Per-frame sky inputs, all recovered values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    /// Authored hour, `0..24`. Used for diagnostics only.
    pub hour: f32,
    /// Day/night switch weight from [`crate::sky_time::night_weight`]. Kept for
    /// diagnostics; the shader uses [`Self::tint`] because one authored set now
    /// covers both phases.
    pub night_weight: f32,
    /// Sun: `[fade, elevation, azimuth, shine strength 0..1]`. The shine is the
    /// recovered stage-1 texture factor of `sunshine.sdr`
    /// (`sky_celestial::glow_texture_factor`, full strength 5), normalised by
    /// that full strength.
    pub sun: [f32; 4],
    /// Moon: `[fade, elevation, azimuth, shine strength 0..1]`. The client's
    /// moon block (`0x4b4049`) halves its `+0x48` colour and scales nothing, so
    /// the moon has no shine quad: the field is carried for the shader's shared
    /// path and stays 0 until the flare chain exists.
    pub moon: [f32; 4],
    /// The pass clear colour. The roof sheet now covers every direction above
    /// the curtain, so this only shows through a partly transparent sample; it
    /// stays the frame's atmospheric fog colour rather than a fixed blue.
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
    /// Whether `sunshine.bmp` (the sun's shine quad) and `moonflare.bmp` (held
    /// for the flare chain) decoded out of the package. Without `sunshine.bmp`
    /// the sun is still submitted at its native size, and the settings tab says
    /// the shine quad is the part that is missing.
    glow_layers: [bool; 2],
}

pub(crate) const SHADER: &str = r#"
struct Params {inv:mat4x4<f32>,time:vec4<f32>,celestial:vec4<f32>,celestial2:vec4<f32>,background:vec4<f32>,tint:vec4<f32>,sets:vec4<f32>};
// Mirrors the Rust `GLOW_SCALE`: the shine quad is this many billboards wide.
const GLOW_SCALE:f32=12.0;
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var skies:texture_2d_array<f32>;
@group(0) @binding(2) var smp:sampler;
// The roof sheet samples past one tile; `clear\roof.bmp` tiles, and the client's
// engine addresses textures by wrap, so the repeat sampler is the one that
// continues the cap projection instead of smearing its border pixels.
@group(0) @binding(3) var smpw:sampler;
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
 let flat=max(length(d.xz),0.000001);
 // Curtain: cylinder of radius 50, so the ray meets it at 50*tan(elevation).
 let y=50.0*d.y/flat;
 // Roof sheet: the cap's own planar lookup (A3DSkySphere::Init 0x79a810,
 // `uv = 0.5 + d.xz * 50 / (2 * 50 * sin(step))`), evaluated at every elevation
 // instead of only inside the 17.5 degree cap disc. On the cap rim it is the
 // cap's own rim texel, so the two surfaces are pixel-continuous there, and
 // because the radius is one smooth function of the direction it can neither
 // hold a single circle (radial streaks) nor bleed a border pixel along a ring.
 let roof=textureSampleLevel(skies,smpw,vec2<f32>(0.5)+d.xz*1.66275358,base+2,0.).rgb*tn;
 if d.y*50.0>=47.685848 || y>39.2700005 {
  return vec4<f32>(roof,1.0);
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
 // The authored skirt fade is an alpha ramp, so it fades over whatever is
 // behind it: with the roof sheet behind, the rim row dissolves into sky
 // instead of into the pass clear colour.
 return vec4<f32>(mix(roof,tex.rgb*tn,curtain_alpha(sy)),1.0);
}
// One native billboard: centre at direction*100, half extent 1.0, so the disc
// subtends 1.146 degrees, plus the sun's additive shine quad (the moon's weight
// is 0 until the flare chain exists). Gated on elevation and fade exactly like
// 0x4b3e00.
fn celestial(color:vec3<f32>,d:vec3<f32>,params:vec4<f32>,disc:i32,glow:i32)->vec3<f32>{
 let alpha=params.x;let el=params.y;let az=params.z;
 // Recovered stage-1 texture factor of `sunshine.sdr`, normalised by its full
 // strength (5.0) so it can tint an additive quad. 0 at full night.
 let shine=clamp(params.w,0.0,1.0);
 if alpha<=0.0 || el<=0.0 {return color;}
 let c=vec3<f32>(cos(az)*cos(el),sin(el),sin(az)*cos(el));
 let z=dot(d,c);
 if z<=0.0 {return color;}
 let right=normalize(cross(vec3<f32>(0.,1.,0.),c)+vec3<f32>(0.000001,0.,0.));
 let up=cross(c,right);
 let axis=vec2<f32>(dot(d,right),-dot(d,up));
 var out=color;
 // Shine quad: `sunshine.bmp`, on a quad GLOW_SCALE times the billboard.
 // Viewer-only size (see GLOW_SCALE). `shine` is 0 for the moon, whose
 // `moonflare.bmp` belongs to the flare chain rather than to this quad.
 let guv=axis/(2.0*0.01*GLOW_SCALE*z)+vec2<f32>(0.5);
 if guv.x>=0.0 && guv.x<=1.0 && guv.y>=0.0 && guv.y<=1.0 {
  out=clamp(out+textureSampleLevel(skies,smp,guv,glow,0.).rgb*(alpha*shine),vec3<f32>(0.),vec3<f32>(1.));
 }
 // p.tint.w is the viewer disc scale (1 is the native 1.146 degree quad).
 let scale=max(p.tint.w,0.25);
 let uv=axis/(2.0*0.01*scale*z)+vec2<f32>(0.5);
 if uv.x<0.0 || uv.x>1.0 || uv.y<0.0 || uv.y>1.0 {return out;}
 let t=textureSampleLevel(skies,smp,uv,disc,0.);
 if disc==9 {return clamp(out+t.rgb*alpha,vec3<f32>(0.),vec3<f32>(1.));}
 return mix(out,t.rgb,t.a*alpha);
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
 color=celestial(color,d,p.celestial,9,11);
 color=celestial(color,d,p.celestial2,10,12);
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
        // The client does not hardcode a sky folder: `CECConfigs::Init` reads
        // `sky_image01..03` from `configs/element_client.cfg` `[DefEnv]`, and
        // the archive proves a filename pattern is unsafe - only 16 of 59 sky
        // folders carry a canonical 01/02/roof triple, the rest use names like
        // `roog.bmp` or `sky_spire.bmp`. The retail config names
        // `Sky\clear\Roof.bmp`, `Sky\clear\01.bmp` and `Sky\clear\02.bmp`.
        //
        // The atlas still lays each group out as `[01, 02, roof]` at
        // `set * 3`, because that is the order `sky_geometry` samples
        // (`Layer::Front = 0`, `Back = 1`, `Roof = 2`); only the folder is
        // configuration-driven. One authored set covers both phases, so all
        // three groups hold the same art and a `sky_transition` swap is a
        // visual no-op until a second authored set exists.
        let configured = sky_folders(element);
        let mut frames = Vec::new();
        for folder in configured {
            for name in ATLAS_ORDER {
                let key = format!("textures/sky/{folder}/{name}");
                let key = if entries.contains_key(&key) {
                    key
                } else {
                    // A configured folder without the canonical triple must not
                    // stop the world loading: fall back to the shipped clear set
                    // for that slot and say so.
                    let fallback = format!("textures/sky/clear/{name}");
                    eprintln!("configured sky texture {key} absent; using {fallback}");
                    fallback
                };
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
        // Layer 11 is the sun's shine quad and layer 12 the moon's
        // `moonflare.bmp`, which the client only uses inside the screen-space
        // flare chain (`0x4b374b..`). That chain is not implemented and the
        // client's moon block scales nothing, so the moon's shine weight is 0;
        // the texture is still decoded and bound so the layer exists when the
        // chain is built. The quad's size is the viewer's `GLOW_SCALE`, and only
        // the texture is the client's.
        let glow_textures = [
            ("textures/nature/sunshine.bmp", image::ImageFormat::Bmp),
            ("textures/nature/moonflare.bmp", image::ImageFormat::Bmp),
        ];
        let mut glow_layers = [false; 2];
        for (i, (name, format)) in glow_textures.into_iter().enumerate() {
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
            glow_layers[i] = decoded.is_some();
            frames.push(decoded.unwrap_or_else(|| image::RgbaImage::new(512, 512)));
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("client sky images"),
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 13,
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
        // The roof sheet is the one sky texture that is sampled past one tile;
        // it is authored to tile (`clear\roof.bmp`'s opposite borders differ by
        // less than one pixel step), so wrapping is what continues the cap
        // instead of smearing its border pixel along the 72.5 degree rim.
        let roof_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky roof sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&roof_sampler),
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
            glow_layers,
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
    /// Whether the shine texture of `sun` decoded from `textures.pck`.
    pub fn shine_texture_present(&self, sun: bool) -> bool {
        self.glow_layers[usize::from(!sun)]
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
/// Atlas order inside one group: `Layer::Front`, `Layer::Back`, `Layer::Roof`.
/// `sky_geometry` samples these by index, so the order is not free.
const ATLAS_ORDER: [&str; 3] = ["01.bmp", "02.bmp", "roof.bmp"];

/// The three `sky_image` values the retail client ships, used when
/// `configs/element_client.cfg` is absent or unreadable.
pub(crate) const SKY_DEFAULTS: [&str; 3] = [
    "Sky\\clear\\Roof.bmp",
    "Sky\\clear\\01.bmp",
    "Sky\\clear\\02.bmp",
];

/// Read `sky_image01..03` out of `configs/element_client.cfg` (`[DefEnv]`).
///
/// The file is a plain INI with CRLF endings and `key = value` entries. Only
/// `[DefEnv]` supplies these keys; other sections are ignored so a later
/// section cannot silently win.
pub(crate) fn sky_images_from_config(text: &str) -> [String; 3] {
    let mut out: [Option<String>; 3] = [None, None, None];
    let mut in_defenv = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_defenv = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case("DefEnv");
            continue;
        }
        if !in_defenv {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let slot = if key.eq_ignore_ascii_case("sky_image01") {
            0
        } else if key.eq_ignore_ascii_case("sky_image02") {
            1
        } else if key.eq_ignore_ascii_case("sky_image03") {
            2
        } else {
            continue;
        };
        if !value.is_empty() {
            out[slot] = Some(value.to_string());
        }
    }
    [
        out[0]
            .clone()
            .unwrap_or_else(|| SKY_DEFAULTS[0].to_string()),
        out[1]
            .clone()
            .unwrap_or_else(|| SKY_DEFAULTS[1].to_string()),
        out[2]
            .clone()
            .unwrap_or_else(|| SKY_DEFAULTS[2].to_string()),
    ]
}

/// Turn a configured client path into a pack key.
///
/// Configs use Windows separators; the pack index is lowercased, so normalise
/// to `textures/sky/<folder>/<file>`.
pub(crate) fn sky_pack_key(configured: &str) -> String {
    let normalised = configured.replace('\\', "/").to_lowercase();
    let trimmed = normalised.trim_start_matches("./");
    if trimmed.starts_with("textures/") {
        trimmed.to_string()
    } else {
        format!("textures/{trimmed}")
    }
}

/// The folder each of the three atlas groups should load.
///
/// Derived from `sky_image01..03`: the retail config names
/// `Sky\clear\Roof.bmp`, `Sky\clear\01.bmp`, `Sky\clear\02.bmp`, so all
/// three resolve to `clear`. If a configured image names a different folder
/// than its siblings, each group keeps its own folder so the configuration is
/// honoured rather than silently collapsed.
fn sky_folders(element: &Path) -> [String; 3] {
    let configured = read_sky_images(element);
    let mut out: [String; 3] = Default::default();
    for (slot, path) in configured.iter().enumerate() {
        // Only the `textures/sky/<folder>/<file>` shape names a folder; a bare
        // filename (some sets ship one) keeps the shipped `clear` art.
        let key = sky_pack_key(path);
        out[slot] = match key.rsplit_once('/') {
            Some((dir, _)) if dir.starts_with("textures/sky/") => dir
                .rsplit_once('/')
                .map_or_else(|| "clear".to_string(), |(_, f)| f.to_string()),
            _ => "clear".to_string(),
        };
    }
    out
}

/// Best-effort read of `configs/element_client.cfg`; any failure falls back to
/// the shipped defaults rather than failing the load.
fn read_sky_images(element: &Path) -> Vec<String> {
    let defaults = || SKY_DEFAULTS.map(str::to_string).to_vec();
    let Ok(mut cfg_file) = File::open(element.join("configs.pck")) else {
        return defaults();
    };
    let Ok(dir) = pck::directory(&mut cfg_file) else {
        return defaults();
    };
    let entries: HashMap<_, _> = dir
        .into_iter()
        .map(|e| (e.name.to_lowercase(), e))
        .collect();
    match entries.get("configs/element_client.cfg") {
        Some(entry) => pck::read_entry(&mut cfg_file, entry)
            .map(|b| sky_images_from_config(&String::from_utf8_lossy(&b)).to_vec())
            .unwrap_or_else(|_| defaults()),
        None => defaults(),
    }
}

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
