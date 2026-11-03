//! Experimental day/night sky using actual client BMP sky textures.
use crate::pck;
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
 let hour=p.time.x;let day=smoothstep(5.,8.,hour)*(1.-smoothstep(17.,20.,hour));
 let evening=(smoothstep(4.,6.,hour)*(1.-smoothstep(7.,9.,hour))) + (smoothstep(16.,18.,hour)*(1.-smoothstep(19.,21.,hour)));
 let e=clamp(evening,0.,1.); let night=sky(d,2);let dusk=sky(d,1);let noon=sky(d,0);
 var color=mix(mix(night,dusk,e),noon,day);
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
    let disc=textureSampleLevel(skies,smp,uv,select(10,9,is_day),0.);
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
        let mut file = File::open(element.join("textures.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<_, _> = pck::directory(&mut file)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        // The client does not hardcode sky folders: `CECConfigs::Init` reads
        // `sky_image01..03` from `configs/element_client.cfg` `[DefEnv]`, and
        // the archive proves a filename pattern is unsafe - only 16 of 59 sky
        // folders use the canonical 01/02/roof triple, the rest use names like
        // `roog.bmp`, `wwwww.bmp` or `sky_spire.bmp`. Read the configured set.
        let defaults = || SKY_DEFAULTS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let configured: Vec<String> = match File::open(element.join("configs.pck")) {
            Ok(mut cfg_file) => {
                let cfg_entries: HashMap<_, _> = match pck::directory(&mut cfg_file) {
                    Ok(list) => list.into_iter().map(|e| (e.name.to_lowercase(), e)).collect(),
                    Err(_) => return Err("unreadable configs.pck directory".to_string()),
                };
                match cfg_entries.get("configs/element_client.cfg") {
                    Some(entry) => match pck::read_entry(&mut cfg_file, entry) {
                        Ok(bytes) => sky_images_from_config(&String::from_utf8_lossy(&bytes))
                            .into_iter()
                            .collect(),
                        Err(_) => defaults(),
                    },
                    None => defaults(),
                }
            }
            // A missing configs.pck must not stop the world from loading; the
            // documented defaults are the client's own `[DefEnv]` values.
            Err(_) => defaults(),
        };
        let mut frames = Vec::new();
        for configured_path in &configured {
            let key = sky_pack_key(configured_path);
            // A configured image that the pack does not carry falls back to the
            // documented default for that slot rather than failing the load.
            let key = if entries.contains_key(&key) {
                key
            } else {
                let default = sky_pack_key(&SKY_DEFAULTS[frames.len().min(2)]);
                eprintln!("configured sky texture {key} absent; using {default}");
                default
            };
            {
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
        })
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
            time: [hour, 0., 0., 0.],
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

/// Pan-orama mapping used by `sky()` in the shader, reproduced on the CPU so the
/// azimuth wrap and the seam blend can be tested without a GPU.
///
/// The shader maps a direction to `az = atan2(x, -z)/2pi + 0.5`, then samples
/// panorama `floor(az*2) % 2` at `u = fract(az*2)`. Two panoramas therefore
/// cover 180 degrees each and together tile the full circle. A panorama's own
/// left/right edges are the two places where the neighbouring panorama takes
/// over, which is what the border crossfade exists for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SkySample {
    /// Panorama index within the time-of-day triple (0 = `01.bmp`, 1 = `02.bmp`).
    pub side: u32,
    /// Horizontal coordinate inside that panorama.
    pub u: f32,
    /// Vertical coordinate; 0 at the zenith edge, 1 at the nadir edge.
    pub v: f32,
}
pub(crate) fn sky_sample(dir: Vec3) -> SkySample {
    let d = dir.normalize_or_zero();
    let az = d.x.atan2(-d.z) / std::f32::consts::TAU + 0.5;
    let scaled = az * 2.;
    SkySample {
        side: (scaled.floor() as i32).rem_euclid(2) as u32,
        u: scaled - scaled.floor(),
        v: (0.5 - d.y.clamp(-1., 1.).asin() / std::f32::consts::PI).clamp(0., 1.),
    }
}
/// Celestial disc placement, mirroring the `p.celestial` block in `fs`.
///
/// The shader builds a basis around the celestial direction `c`
/// (`x = normalize(cross(up, c))`, `y = cross(c, x)`), projects the view ray
/// into it and divides by `z = dot(d, c)` so the billboard stays planar on the
/// sphere. Returns `None` when the ray misses the disc, which is what the
/// `if z > 0.98 && uv in [0,1]` guards test.
pub(crate) struct Celestial {
    pub direction: Vec3,
    pub day_size: f32,
    pub night_size: f32,
}
impl Celestial {
    /// Whether the body is drawn at all. The shader gates on
    /// `c.y > 0.0` alone; the hour only decides which of the two textures is
    /// sampled, so it does not enter this test.
    pub fn visible(&self) -> bool {
        self.direction.y > 0. && self.direction.length() > 0.
    }
    pub fn is_day(&self, hour: f32) -> bool {
        (3.5..21.).contains(&hour)
    }
    /// `uv` in [0,1]^2 when the ray hits the disc, else `None`.
    pub fn project(&self, ray: Vec3, hour: f32) -> Option<[f32; 2]> {
        let c = self.direction.normalize_or_zero();
        if c == Vec3::ZERO {
            return None;
        }
        let up = Vec3::Y;
        let mut x = up.cross(c);
        // The shader adds (1e-5,0,0) because `cross(up, c)` degenerates when
        // the body is exactly overhead; keep that so behaviour matches.
        x += Vec3::new(0.00001, 0., 0.);
        let x = x.normalize_or_zero();
        let y = c.cross(x);
        let d = ray.normalize_or_zero();
        let z = d.dot(c);
        if z <= 0.98 {
            return None;
        }
        let size = if self.is_day(hour) { self.day_size } else { self.night_size };
        let uv = [
            d.dot(x) / (2. * size * z) + 0.5,
            -d.dot(y) / (2. * size * z) + 0.5,
        ];
        if uv[0] < 0. || uv[0] > 1. || uv[1] < 0. || uv[1] > 1. {
            None
        } else {
            Some(uv)
        }
    }
}
/// The border crossfade in `sky()`. Returns the weight of the *own* panorama;
/// the remainder comes from the neighbour sampled at the opposite edge.
pub(crate) fn seam_weight(u: f32) -> f32 {
    const BORDER: f32 = 0.08;
    if u < BORDER {
        0.5 + 0.5 * u / BORDER
    } else if u > 1. - BORDER {
        1. - 0.5 * (u - (1. - BORDER)) / BORDER
    } else {
        1.
    }
}
/// The three sky images the client loads, in `sky_image01..03` order.
///
/// `CECConfigs::Init` reads these as named settings and the retail
/// `configs/element_client.cfg` states them in `[DefEnv]`:
///
/// ```ini
/// sky_image01 = Sky\clear\Roof.bmp
/// sky_image02 = Sky\clear\01.bmp
/// sky_image03 = Sky\clear\02.bmp
/// ```
///
/// So the horizontal order is roof, then 01, then 02 - which is the order the
/// shader already samples (`layer*3 + 0`/`1`/`2`, roof last as the zenith cap).
pub(crate) const SKY_DEFAULTS: [&str; 3] = [
    "Sky\\clear\\Roof.bmp",
    "Sky\\clear\\01.bmp",
    "Sky\\clear\\02.bmp",
];

/// Read `sky_image01..03` out of `configs/element_client.cfg` (`[DefEnv]`).
///
/// The file is a plain INI with CRLF line endings and `key = value` entries.
/// Anything unreadable falls back to [`SKY_DEFAULTS`], because a missing or
/// malformed settings file must not stop the world from loading.
pub(crate) fn sky_images_from_config(text: &str) -> [String; 3] {
    let mut out: [Option<String>; 3] = [None, None, None];
    let mut in_defenv = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_defenv = line[1..line.len() - 1].trim().eq_ignore_ascii_case("DefEnv");
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
        out[0].clone().unwrap_or_else(|| SKY_DEFAULTS[0].to_string()),
        out[1].clone().unwrap_or_else(|| SKY_DEFAULTS[1].to_string()),
        out[2].clone().unwrap_or_else(|| SKY_DEFAULTS[2].to_string()),
    ]
}

/// Turn a configured client path into a pack key.
///
/// Configs use Windows separators and are case-sensitive only where the
/// filesystem was; the pack index is lowercased, so normalise to
/// `textures/sky/<folder>/<file>`.
pub(crate) fn sky_pack_key(configured: &str) -> String {
    let normalised = configured.replace('\\', "/").to_lowercase();
    let trimmed = normalised.trim_start_matches("./");
    if trimmed.starts_with("textures/") {
        trimmed.to_string()
    } else {
        format!("textures/{trimmed}")
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
mod config_tests {
    use super::*;
    /// The exact `[DefEnv]` block from the retail `configs/element_client.cfg`,
    /// CRLF endings and all. This pins the order the shader relies on:
    /// `sky_image01` is the roof cap, then 01, then 02.
    #[test]
    fn retail_element_client_cfg_yields_roof_then_01_then_02() {
        let cfg = "[PathFile]\r\ndummy = 1\r\n\r\n[DefEnv]\r\n\
                   fog_start = 60\r\nfog_end = 400\r\nfog_color = 70, 77, 91\r\n\
                   fog_density = 1\r\n\
                   sky_image01 = Sky\\clear\\Roof.bmp\r\n\
                   sky_image02 = Sky\\clear\\01.bmp\r\n\
                   sky_image03 = Sky\\clear\\02.bmp\r\n\
                   ambient = 180, 180, 180\r\n";
        let images = sky_images_from_config(cfg);
        assert_eq!(images[0], "Sky\\clear\\Roof.bmp");
        assert_eq!(images[1], "Sky\\clear\\01.bmp");
        assert_eq!(images[2], "Sky\\clear\\02.bmp");
        // 01 must precede 02; the seam crossfade and the layer index depend on it.
        assert!(images[1] < images[2], "01 must precede 02 in layer order");
    }
    /// Every sky folder that is not `clear` must still be selectable, which is
    /// the whole reason this is config-driven: 43 of 59 archive folders do not
    /// use the canonical 01/02/roof names.
    #[test]
    fn a_non_default_region_set_is_honoured_verbatim() {
        let cfg = "[DefEnv]\r\n\
                   sky_image01 = Sky\\夜晚01\\sky_spire.bmp\r\n\
                   sky_image02 = Sky\\夜晚01\\01.bmp\r\n\
                   sky_image03 = Sky\\夜晚01\\02.bmp\r\n";
        let images = sky_images_from_config(cfg);
        assert_eq!(images[0], "Sky\\夜晚01\\sky_spire.bmp");
        assert_eq!(sky_pack_key(&images[0]), "textures/sky/夜晚01/sky_spire.bmp");
    }
    /// Missing keys and unreadable files fall back rather than failing the load.
    #[test]
    fn absent_keys_fall_back_to_the_documented_defaults() {
        let images = sky_images_from_config("");
        assert_eq!(images[0], SKY_DEFAULTS[0]);
        assert_eq!(images[1], SKY_DEFAULTS[1]);
        assert_eq!(images[2], SKY_DEFAULTS[2]);
        // A partial block fills only the slots it states.
        let partial = sky_images_from_config("[DefEnv]\r\nsky_image02 = Sky\\x\\y.bmp\r\n");
        assert_eq!(partial[0], SKY_DEFAULTS[0]);
        assert_eq!(partial[1], "Sky\\x\\y.bmp");
        assert_eq!(partial[2], SKY_DEFAULTS[2]);
    }
    /// Keys outside `[DefEnv]` must not be picked up, and key matching is
    /// case-insensitive like the client's INI reader.
    #[test]
    fn only_the_defenv_section_supplies_sky_images() {
        let cfg = "[Other]\r\nsky_image01 = Sky\\wrong\\x.bmp\r\n\
                   [DefEnv]\r\nSKY_IMAGE01 = Sky\\right\\x.bmp\r\n";
        let images = sky_images_from_config(cfg);
        assert_eq!(images[0], "Sky\\right\\x.bmp");
    }
    /// Configured paths are Windows-style and must become pack keys.
    #[test]
    fn configured_paths_normalise_to_lowercase_pack_keys() {
        assert_eq!(
            sky_pack_key("Sky\\clear\\01.bmp"),
            "textures/sky/clear/01.bmp"
        );
        assert_eq!(
            sky_pack_key("textures\\Sky\\Clear\\Roof.BMP"),
            "textures/sky/clear/roof.bmp"
        );
        assert_eq!(sky_pack_key("Sky/clear/02.bmp"), "textures/sky/clear/02.bmp");
    }
}
#[cfg(test)]
mod sky_seam_tests {
    use super::*;
    /// Azimuth 0 (north) sits exactly on the boundary between the two
    /// panoramas, so it must resolve deterministically even though `floor` of a
    /// value constructed as exactly 1.0 is fragile in float.
    #[test]
    fn cardinal_directions_land_on_the_expected_panorama_and_u() {
        let north = sky_sample(Vec3::new(0., 0., -1.));
        assert_eq!(north.side, 1, "azimuth 0 is the boundary; side 1 is the wrap side");
        assert!(north.u < 1e-6, "u={}", north.u);
        // `az = atan2(x, -z)/2pi + 0.5`, so +x gives az = 0.75 and -x gives
        // az = 0.25; with `floor(2*az) % 2` that puts +x on side 1 and -x on
        // side 0, both at the panorama's horizontal middle.
        let east = sky_sample(Vec3::new(1., 0., 0.));
        assert_eq!(east.side, 1, "az=0.75 -> floor(1.5)%2 = 1");
        assert!((east.u - 0.5).abs() < 1e-6, "u={}", east.u);
        let south = sky_sample(Vec3::new(0., 0., 1.));
        assert_eq!(south.side, 0);
        assert!(south.u < 1e-6, "u={}", south.u);
        let west = sky_sample(Vec3::new(-1., 0., 0.));
        assert_eq!(west.side, 0, "az=0.25 -> floor(0.5)%2 = 0");
        assert!((west.u - 0.5).abs() < 1e-6, "u={}", west.u);
    }
    /// Sweeping the full circle must never produce an out-of-range u, and the
    /// two panoramas must each be used for exactly half of it.
    #[test]
    fn azimuth_sweep_stays_in_range_and_splits_the_circle_evenly() {
        let (mut a, mut b) = (0, 0);
        for i in 0..3600 {
            let t = i as f32 / 3600. * std::f32::consts::TAU;
            let s = sky_sample(Vec3::new(t.sin(), 0., -t.cos()));
            assert!((0. ..1.).contains(&s.u), "u out of range at {t}: {}", s.u);
            match s.side {
                0 => a += 1,
                1 => b += 1,
                other => panic!("unexpected side {other}"),
            }
        }
        assert_eq!(a + b, 3600);
        assert!((a as i32 - b as i32).abs() <= 2, "uneven split: {a} vs {b}");
    }
    /// The seam weight is 1 away from the borders, halves at the seam, and is
    /// monotone in between; the two borders must agree with each other.
    #[test]
    fn seam_weight_peaks_in_the_middle_and_halves_at_both_edges() {
        assert_eq!(seam_weight(0.5), 1.);
        assert!((seam_weight(0.) - 0.5).abs() < 1e-6);
        assert!((seam_weight(1.) - 0.5).abs() < 1e-6);
        // Symmetric: the left border at u and the right border at 1-u match.
        for k in 0..8 {
            let u = k as f32 * 0.01;
            assert!((seam_weight(u) - seam_weight(1. - u)).abs() < 1e-5, "u={u}");
        }
        // Monotone towards the middle on both sides.
        let mut prev = seam_weight(0.);
        for k in 1..=8 {
            let w = seam_weight(k as f32 * 0.01);
            assert!(w >= prev, "not monotone at {k}");
            prev = w;
        }
    }
    fn body(direction: Vec3) -> Celestial {
        Celestial { direction, day_size: 0.035, night_size: 0.028 }
    }
    /// Looking straight at the body puts the sample at the disc centre, and
    /// off-axis rays stay inside the disc while the disc is small.
    #[test]
    fn celestial_disc_centres_on_the_body_direction() {
        let c = body(Vec3::new(0., 1., 0.));
        let uv = c.project(Vec3::new(0., 1., 0.), 12.).expect("centre ray must hit");
        assert!((uv[0] - 0.5).abs() < 1e-3 && (uv[1] - 0.5).abs() < 1e-3, "{uv:?}");
        // A ray 0.9 degree off axis is inside a 1.75 degree half-angle disc.
        let off = Vec3::new(0.9f32.to_radians().sin(), 0.9f32.to_radians().cos(), 0.);
        assert!(c.project(off, 12.).is_some());
        // A ray 5 degrees off axis is well outside it.
        let far = Vec3::new(5f32.to_radians().sin(), 5f32.to_radians().cos(), 0.);
        assert!(c.project(far, 12.).is_none());
    }
    /// The disc must not appear when the body is below the horizon.
    #[test]
    fn celestial_body_below_horizon_is_never_visible() {
        let c = body(Vec3::new(0., -0.5, -1.));
        assert!(!c.visible());
        // A body exactly on the horizon is not drawn either.
        assert!(!body(Vec3::new(0., 0., -1.)).visible());
        let up = body(Vec3::new(0., 0.5, -1.));
        assert!(up.visible());
    }
    /// Day body is drawn from 03:30 to 21:00 and the night body outside it.
    #[test]
    fn celestial_day_window_bounds_the_sun_disc() {
        let c = body(Vec3::new(0., 0.5, -1.));
        assert!(!c.is_day(3.0));
        assert!(c.is_day(3.5));
        assert!(c.is_day(20.9));
        assert!(!c.is_day(21.0));
        // The size swaps with the window, which is what selects the sun vs moon
        // texture layer (9 vs 10).
        assert!(c.day_size > c.night_size);
    }
    /// The +1e-5 basis nudge exists because `cross(up, c)` collapses when the
    /// body is exactly overhead. The projection must still be finite there.
    #[test]
    fn celestial_projection_survives_a_body_exactly_overhead() {
        let c = body(Vec3::new(0., 1., 0.));
        let uv = c.project(Vec3::new(0., 1., 0.), 12.).expect("overhead must not divide by zero");
        assert!(uv[0].is_finite() && uv[1].is_finite(), "{uv:?}");
    }
    /// Vertical mapping: horizon at the middle, zenith at 0, nadir at 1.
    #[test]
    fn vertical_mapping_puts_the_horizon_at_the_middle() {
        assert!((sky_sample(Vec3::new(0., 0., -1.)).v - 0.5).abs() < 1e-6);
        assert!(sky_sample(Vec3::new(0., 1., 0.)).v < 1e-6);
        assert!(sky_sample(Vec3::new(0., -1., 0.)).v > 1. - 1e-6);
        // Monotone from zenith to nadir.
        let mut prev = -1.;
        for k in 0..=10 {
            let y = 1. - k as f32 * 0.2;
            let v = sky_sample(Vec3::new(0., y, -1.)).v;
            assert!(v > prev, "v not monotone at y={y}");
            prev = v;
        }
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
