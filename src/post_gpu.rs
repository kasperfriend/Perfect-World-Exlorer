//! Fullscreen glow based on the client's fullglow1_14/fullglow2_14 shader arithmetic,
//! plus client-region-driven underwater fog, spacewarp and optional display gamma.
#[path = "gamma_curve.rs"]
mod gamma_curve;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
/// Glow constants and interpolation traced through 0x410bb0 / 0x4111f0.
/// Special mode 3 is selected when the camera is underwater (0x41eb69,
/// 0x4214ad). Modes 4/5 can be previewed without adding player/gameplay state.
#[derive(Default, Debug)]
pub struct WorldGlow {
    mode: u8,
    values: [f32; 2],
    from: [f32; 2],
    target: [f32; 2],
    elapsed: f32,
    duration: f32,
    mono: f32,
    mono_from: f32,
    mono_target: f32,
    mono_elapsed: f32,
    mono_duration: f32,
    mono_color: [f32; 3],
}
impl WorldGlow {
    pub fn update(&mut self, enabled: bool, daylight: f32, underwater: bool, dt: f32) -> [f32; 6] {
        self.update_with_mode(enabled, daylight, underwater, 0, dt)
    }
    /// Zero selects world/day/night/underwater automatically. Four and five
    /// preview the client special-mode parameters without gameplay triggers.
    pub fn update_with_mode(
        &mut self,
        enabled: bool,
        daylight: f32,
        underwater: bool,
        override_mode: u8,
        dt: f32,
    ) -> [f32; 6] {
        if !enabled {
            *self = Self::default();
            return [0.; 6];
        }
        let mode = if matches!(override_mode, 4 | 5) {
            override_mode
        } else if underwater {
            3
        } else if 1. - daylight < 0.5 {
            1
        } else {
            2
        };
        if self.mode != mode {
            self.mode = mode;
            self.from = self.values;
            self.elapsed = 0.;
            self.mono_elapsed = 0.;
            match mode {
                1 => {
                    self.target = [0.25, 0.25];
                    self.duration = 5.;
                    self.mono_from = self.mono;
                    self.mono_target = 0.;
                    self.mono_duration = 5.;
                }
                2 => {
                    self.target = [0.35, 0.35];
                    self.duration = 5.;
                    self.mono_from = self.mono;
                    self.mono_target = 0.;
                    self.mono_duration = 5.;
                }
                3 => {
                    self.target = [0.5, 0.6];
                    self.duration = 2.;
                    self.mono = 0.;
                    self.mono_from = 0.;
                    self.mono_target = 0.5;
                    self.mono_duration = 1.5;
                    self.mono_color = [187. / 255., 234. / 255., 187. / 255.];
                }
                4 => {
                    // 0x410cdb: level 1, power .7, mono 0->1 in 2s.
                    self.target = [1., 0.7];
                    self.duration = 2.;
                    self.mono = 0.;
                    self.mono_from = 0.;
                    self.mono_target = 1.;
                    self.mono_duration = 2.;
                    self.mono_color = [226. / 255., 1., 1.];
                }
                5 => {
                    // 0x410d28: level .1, power .25 in 1s; mono fades 5s.
                    self.target = [0.1, 0.25];
                    self.duration = 1.;
                    self.mono_from = self.mono;
                    self.mono_target = 0.;
                    self.mono_duration = 5.;
                }
                _ => unreachable!(),
            }
        }
        let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
        self.elapsed = (self.elapsed + dt).min(self.duration);
        let t = self.elapsed / self.duration;
        for i in 0..2 {
            self.values[i] = self.from[i] + (self.target[i] - self.from[i]) * t;
        }
        self.mono_elapsed = (self.mono_elapsed + dt).min(self.mono_duration);
        self.mono = self.mono_from
            + (self.mono_target - self.mono_from) * (self.mono_elapsed / self.mono_duration);
        [
            self.values[0],
            self.values[1],
            self.mono_color[0],
            self.mono_color[1],
            self.mono_color[2],
            self.mono,
        ]
    }
}
#[cfg(test)]
mod glow_tests {
    use super::WorldGlow;
    #[test]
    fn special_modes_use_distinct_targets_tints_and_transition_durations() {
        let mut g = WorldGlow::default();
        let a = g.update_with_mode(true, 1., false, 4, 1.);
        assert_eq!(&a[..2], &[0.5, 0.35]);
        assert_eq!(&a[2..5], &[226. / 255., 1., 1.]);
        assert_eq!(a[5], 0.5);
        let b = g.update_with_mode(true, 1., false, 4, 1.);
        assert_eq!(&b[..2], &[1., 0.7]);
        assert_eq!(b[5], 1.);
        let c = g.update_with_mode(true, 1., false, 5, 0.5);
        assert!((c[0] - 0.55).abs() < 1e-6 && (c[1] - 0.475).abs() < 1e-6);
        assert!((c[5] - 0.9).abs() < 1e-6);
        let d = g.update_with_mode(true, 1., false, 5, 0.5);
        assert!(
            (d[0] - 0.1).abs() < 1e-6 && (d[1] - 0.25).abs() < 1e-6 && (d[5] - 0.8).abs() < 1e-6
        );
        assert_eq!(g.update_with_mode(false, 1., false, 4, 1.), [0.; 6]);
    }
    #[test]
    fn normal_modes_transition_from_current_level() {
        let mut g = WorldGlow::default();
        assert_eq!(&g.update(true, 1., false, 0.)[..2], &[0., 0.]);
        assert_eq!(&g.update(true, 1., false, 5.)[..2], &[0.25, 0.25]);
        assert_eq!(&g.update(true, 0., false, 2.5)[..2], &[0.30, 0.30]);
        assert!((g.update(true, 0., false, 2.5)[0] - 0.35).abs() < 1e-6);
        assert_eq!(g.update(false, 0., false, 1.), [0.; 6]);
    }
    #[test]
    fn underwater_mode_uses_independent_transitions_and_fades_on_exit() {
        let mut g = WorldGlow::default();
        g.update(true, 1., false, 5.);
        let a = g.update(true, 1., true, 1.);
        assert!((a[0] - 0.375).abs() < 1e-6);
        assert!((a[1] - 0.425).abs() < 1e-6);
        assert!((a[5] - 1. / 3.).abs() < 1e-6);
        assert_eq!(&a[2..5], &[187. / 255., 234. / 255., 187. / 255.]);
        let b = g.update(true, 1., true, 1.);
        assert_eq!(&b[..2], &[0.5, 0.6]);
        assert_eq!(b[5], 0.5);
        let c = g.update(true, 1., false, 2.5);
        assert!((c[0] - 0.375).abs() < 1e-6);
        assert!((c[1] - 0.425).abs() < 1e-6);
        assert_eq!(c[5], 0.25);
        assert_eq!(g.update(true, 1., false, 2.5)[5], 0.);
    }
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    screen: [f32; 4],
    effect: [f32; 4],
    fog: [f32; 4],
    fog_color: [f32; 4],
    gamma: [f32; 4],
    mono: [f32; 4],
}
const SHADER: &str = r#"
struct Params {screen:vec4<f32>,effect:vec4<f32>,fog:vec4<f32>,fog_color:vec4<f32>,gamma:vec4<f32>,mono:vec4<f32>};
@group(0) @binding(0) var scene:texture_2d<f32>;
@group(0) @binding(1) var blur:texture_2d<f32>;
@group(0) @binding(2) var smp:sampler;
@group(0) @binding(3) var depth:texture_depth_2d;
@group(0) @binding(4) var<uniform> p:Params;
struct Out {@builtin(position) pos:vec4<f32>, @location(0) uv:vec2<f32>};
@vertex fn vs(@builtin(vertex_index) id:u32)->Out {
 var o:Out; let x=f32((id<<1u)&2u);let y=f32(id&2u);
 o.uv=vec2<f32>(x,y);o.pos=vec4<f32>(x*2.-1.,1.-y*2.,0.,1.);return o;
}
// The client first stretch-copies the full-resolution back target into a
// quarter-size render target, then applies the horizontal five-tap pass.
@fragment fn downsample(i:Out)->@location(0) vec4<f32> {return vec4<f32>(textureSample(scene,smp,i.uv).rgb,1.);}
// fullglow1_14: center*c1.r + two inner*c1.a + two outer*c0.a.
@fragment fn glow_h(i:Out)->@location(0) vec4<f32> {
 // 0x40edd0 uses 4/full_width, not 1/floor(full_width/4).
 // These differ for window sizes that are not divisible by four.
 let dx=vec2<f32>(4.0/p.screen.x,0.);
 let c=textureSample(blur,smp,i.uv).rgb*0.310;
 let a=(textureSample(blur,smp,i.uv-dx).rgb+textureSample(blur,smp,i.uv+dx).rgb)*0.238;
 let b=(textureSample(blur,smp,i.uv-dx*2.).rgb+textureSample(blur,smp,i.uv+dx*2.).rgb)*0.107;
 return vec4<f32>(c+a+b,1.);
}
@fragment fn fog_scene(i:Out)->@location(0) vec4<f32> {
 let uv=i.uv;let underwater=p.effect.x>0.;
 var out=textureSample(scene,smp,uv).rgb;
 {
   let pixel=vec2<i32>(clamp(uv*p.screen.xy,vec2<f32>(0.),p.screen.xy-vec2<f32>(1.)));
   let d=textureLoad(depth,pixel,0);
   let far=p.screen.w;let distance=far/max(1.+d*(far-1.),0.0001);
   // Client defaults to D3DFOG_LINEAR (3); depth is view-space z.
   let fog_span=p.fog.y-p.fog.x;
   var fog=select(clamp((distance-p.fog.x)/select(fog_span,0.001,abs(fog_span)<0.001),0.,1.),select(0.,1.,distance>=p.fog.y),abs(fog_span)<0.001);
   if !underwater && d<=0.0000001 { fog=0.; }
   // Scene and intermediate blur are sampled as encoded UNORM values.
   out=mix(out,p.fog_color.rgb,fog);
 }
 return vec4<f32>(out,1.);
}
// Emulate the 256x256 ARGB procedural bump texture built by 0x410f10.
// The client converts -32*sin/cos to integer before subtracting from 128.
fn bump_texel(p:vec2<f32>)->vec2<f32> {
 let theta=12.56637061436*((p.x+p.y)/256.0-1.0);
 return (vec2<f32>(128.0)-trunc(vec2<f32>(cos(theta),sin(theta))*-32.0))/255.0;
}
fn bump_bilinear(uv:vec2<f32>)->vec2<f32> {
 // Half-texel convention of normalized linear texture sampling.
 let p=fract(uv)*256.0-vec2<f32>(0.5);let base=floor(p);let f=fract(p);
 let a=mix(bump_texel(base),bump_texel(base+vec2<f32>(1.,0.)),f.x);
 let b=mix(bump_texel(base+vec2<f32>(0.,1.)),bump_texel(base+vec2<f32>(1.,1.)),f.x);
 return mix(a,b,f.y)*2.0-vec2<f32>(1.0);
}
// 0x40fd33 executes spacewarp, copies its result back, then 0x40fd64
// starts glow. Blur must therefore consume the quantized warped scene.
@fragment fn warp_scene(i:Out)->@location(0) vec4<f32> {
 let uv=i.uv;
 let bump_uv=vec2<f32>(uv.x*3.,uv.y*3.*p.screen.y/p.screen.x+p.screen.z*0.5);
 let normal=bump_bilinear(bump_uv);
 // Texture stage 1 is CLAMP at 0x41029b–0x4102a4, not a 0.001 UV inset.
 return vec4<f32>(textureSample(scene,smp,uv+normal*vec2<f32>(-0.005,0.005)).rgb,1.);
}
@fragment fn finish(i:Out)->@location(0) vec4<f32> {
 let uv=i.uv;
 let original=textureSample(scene,smp,uv).rgb;
 var out=original;
 if p.effect.y>0.5 {
   // Matching vertical vertex coordinates at 0x40f04b–0x40f053.
   let dy=vec2<f32>(0.,4.0/p.screen.y);
   let c=textureSample(blur,smp,uv).rgb*0.310;
   let a=(textureSample(blur,smp,uv-dy).rgb+textureSample(blur,smp,uv+dy).rgb)*0.238;
   let b=(textureSample(blur,smp,uv-dy*2.).rgb+textureSample(blur,smp,uv+dy*2.).rgb)*0.107;
   let glow=c+a+b;
   // fullglow2_14: lrp(original, blur, glow_level) + glow_power*blur².
   let strength=p.effect.z;
   if p.mono.w>0. {
     // monofullglow_14 uses blur*original (not blur squared) before
     // saturating the luma dot and mixing the monochrome hue.
     let base=mix(original,glow,clamp(p.gamma.z*strength,0.,1.))+p.gamma.w*strength*glow*original;
     let luminance=clamp(dot(base,vec3<f32>(0.35,0.6,0.25)),0.,1.);
     out=mix(base,luminance*p.mono.rgb,p.mono.w);
   } else {
     out=mix(original,glow,clamp(p.gamma.z*strength,0.,1.))+p.gamma.w*strength*glow*glow;
   }
 }
 // Emulate the client's 256-entry 16-bit D3D8 display ramp without
 // changing the desktop display. Linear interpolation models LUT lookup.
 if p.gamma.x>0.5 {
   out=client_gamma_ramp(out,p.gamma.y);
 }
 // Present encoded D3D-style output through an sRGB render target exactly once.
 if p.fog.w>0.5 {out=select(out/12.92,pow((max(out,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),out>vec3<f32>(0.04045));}
 return vec4<f32>(out,1.);
}
"#;
fn texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
    label: &str,
) -> wgpu::Texture {
    let alternate = [format.remove_srgb_suffix()];
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
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
        view_formats: if format.is_srgb() { &alternate } else { &[] },
    })
}
pub struct PostGpu {
    format: wgpu::TextureFormat,
    scene: wgpu::Texture,
    fogged: wgpu::Texture,
    warped: wgpu::Texture,
    downsample: wgpu::Texture,
    blur: wgpu::Texture,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    fog_pipeline: wgpu::RenderPipeline,
    warp_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    horizontal: wgpu::RenderPipeline,
    finish: wgpu::RenderPipeline,
}
impl PostGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> Self {
        let scene = texture(device, format, w, h, "post scene");
        let fogged = texture(device, format.remove_srgb_suffix(), w, h, "fogged scene");
        let warped = texture(
            device,
            format.remove_srgb_suffix(),
            w,
            h,
            "underwater warped scene",
        );
        let downsample = texture(
            device,
            format.remove_srgb_suffix(),
            (w / 4).max(1),
            (h / 4).max(1),
            "glow quarter-size copy",
        );
        let blur = texture(
            device,
            format.remove_srgb_suffix(),
            (w / 4).max(1),
            (h / 4).max(1),
            "glow blur",
        );
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("post parameters"),
            contents: bytemuck::bytes_of(&Params {
                screen: [1.; 4],
                effect: [0.; 4],
                fog: [0.; 4],
                fog_color: [0.; 4],
                gamma: [0.; 4],
                mono: [0.; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post bindings"),
            entries: &[
                tex(0),
                tex(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            label: Some("client-inspired glow and underwater"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", gamma_curve::WGSL, SHADER).into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let make = |entry, target_format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
            })
        };
        Self {
            format,
            scene,
            fogged,
            warped,
            downsample,
            blur,
            layout,
            sampler,
            uniform,
            fog_pipeline: make("fog_scene", format.remove_srgb_suffix()),
            warp_pipeline: make("warp_scene", format.remove_srgb_suffix()),
            downsample_pipeline: make("downsample", format.remove_srgb_suffix()),
            horizontal: make("glow_h", format.remove_srgb_suffix()),
            finish: make("finish", format),
        }
    }
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        self.scene = texture(device, self.format, w, h, "post scene");
        self.fogged = texture(
            device,
            self.format.remove_srgb_suffix(),
            w,
            h,
            "fogged scene",
        );
        self.warped = texture(
            device,
            self.format.remove_srgb_suffix(),
            w,
            h,
            "underwater warped scene",
        );
        self.downsample = texture(
            device,
            self.format.remove_srgb_suffix(),
            (w / 4).max(1),
            (h / 4).max(1),
            "glow quarter-size copy",
        );
        self.blur = texture(
            device,
            self.format.remove_srgb_suffix(),
            (w / 4).max(1),
            (h / 4).max(1),
            "glow blur",
        );
    }
    pub fn scene_view(&self) -> wgpu::TextureView {
        self.scene.create_view(&Default::default())
    }
    /// Raw view permits legacy encoded-space alpha blending for shoreline waves.
    pub fn scene_raw_view(&self) -> wgpu::TextureView {
        self.scene.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format.remove_srgb_suffix()),
            ..Default::default()
        })
    }
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        w: u32,
        h: u32,
        time: f32,
        far: f32,
        underwater: f32,
        fog: crate::environment::Fog,
        atmospheric_fog: bool,
        screen_fog: bool,
        bloom: bool,
        bloom_strength: f32,
        glow_parameters: [f32; 6],
        client_gamma: bool,
        gamma_setting: f32,
    ) {
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Params {
                screen: [w as f32, h as f32, time, far],
                effect: [
                    underwater,
                    if bloom { 1. } else { 0. },
                    bloom_strength,
                    if atmospheric_fog { 1. } else { 0. },
                ],
                fog: [
                    fog.start,
                    fog.end,
                    fog.density,
                    if self.format.is_srgb() { 1. } else { 0. },
                ],
                fog_color: [fog.color[0], fog.color[1], fog.color[2], 1.],
                gamma: [
                    if client_gamma { 1. } else { 0. },
                    gamma_curve::setting(gamma_setting),
                    glow_parameters[0],
                    glow_parameters[1],
                ],
                mono: [
                    glow_parameters[2],
                    glow_parameters[3],
                    glow_parameters[4],
                    glow_parameters[5],
                ],
            }),
        );
        let scene = self.scene.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format.remove_srgb_suffix()),
            ..Default::default()
        });
        let fogged = self.fogged.create_view(&Default::default());
        let warped = self.warped.create_view(&Default::default());
        let downsample = self.downsample.create_view(&Default::default());
        let blur = self.blur.create_view(&Default::default());
        // Separate horizontal and composite bind groups avoid binding the blur texture
        // as both render target and shader resource in the same pass.
        let bind = |input: &wgpu::TextureView, other: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post textures"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(other),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            })
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        let apply_fog = screen_fog && (underwater > 0. || atmospheric_fog);
        if apply_fog {
            let group = bind(&scene, &scene);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene fog before glow"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &fogged,
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
            pass.set_pipeline(&self.fog_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        let before_warp = if apply_fog { &fogged } else { &scene };
        if underwater > 0. {
            let group = bind(before_warp, before_warp);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("underwater distortion before glow"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &warped,
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
            pass.set_pipeline(&self.warp_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        let input = if underwater > 0. {
            &warped
        } else {
            before_warp
        };
        if bloom {
            {
                let group = bind(input, input);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("glow quarter-size stretch copy"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &downsample,
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
                pass.set_pipeline(&self.downsample_pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            let group = bind(input, &downsample);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glow horizontal"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &blur,
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
            pass.set_pipeline(&self.horizontal);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let group = bind(input, &blur);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("underwater and glow"),
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
            pass.set_pipeline(&self.finish);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
