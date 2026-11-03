//! Portable WGSL temporal upscaler and frame interpolator.
//!
//! These are the GPU side of the Graphics-tab modes in
//! [`crate::view_settings::upscaling`]. They are *in-house implementations*,
//! not the vendor SDKs: no NVIDIA NGX, Intel XeSS or FidelityFX binaries are
//! bundled, and no vendor neural network is reproduced. What is implemented is
//! the algorithm family the technologies share:
//!
//! * jittered sub-pixel rendering (`upscaling::jitter_ndc`),
//! * depth-based temporal reprojection with the previous frame's clip matrix,
//! * YCoCg neighbourhood (variance) clipping to suppress ghosting,
//! * depth/motion/luma disocclusion rejection and temporal accumulation,
//! * a contrast-adaptive RCAS-like sharpen pass,
//! * per-technology tuning profiles (`Upscaler::tuning`), and
//! * frame generation by reprojected interpolation between the two most recent
//!   upscaled frames at 2x/3x/4x pacing.
//!
//! The world is rendered into the existing low-resolution `RenderScale` target.
//! `UpscaleRuntime::upscale` replaces its bilinear blit with this chain and
//! keeps two full-resolution history textures plus two low-resolution depth
//! history textures; `warp` produces extra presented frames without rendering
//! the world again. All colour views are the renderer's sRGB format, so the GPU
//! decodes on read and encodes exactly once on write.

use crate::view_settings::upscaling::{self, Upscaler};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Shared uniform layout. WGSL `mat4x4<f32>` is column-major, matching glam's
/// `to_cols_array_2d()` and a Rust `[[f32; 4]; 4]` (both 64 bytes, align 16).
///
/// `frame.xy` is jitter in UV units and `frame.z` is the history-valid flag for
/// the temporal pass; the warp pass uses `frame.z` as its interpolation alpha.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    inv_vp: [[f32; 4]; 4],
    prev_vp: [[f32; 4]; 4],
    /// low_w, low_h, out_w, out_h
    sizes: [f32; 4],
    /// jitter.xy, history_valid / alpha, frame index
    frame: [f32; 4],
    /// max history weight, depth tolerance, profile id, reserved
    tuning: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SharpenParams {
    /// sharpness, unused, out_w, out_h
    params: [f32; 4],
}

const TEMPORAL_WGSL: &str = r#"
struct Params {
  inv_vp : mat4x4<f32>,
  prev_vp : mat4x4<f32>,
  sizes : vec4<f32>,
  frame : vec4<f32>,
  tuning : vec4<f32>,
}
@group(0) @binding(0) var current_color : texture_2d<f32>;
@group(0) @binding(1) var history_color : texture_2d<f32>;
@group(0) @binding(2) var current_depth : texture_depth_2d;
@group(0) @binding(3) var previous_depth : texture_depth_2d;
@group(0) @binding(4) var samp : sampler;
@group(0) @binding(5) var<uniform> p : Params;

@vertex fn vs(@builtin(vertex_index) id:u32) -> @builtin(position) vec4<f32> {
  let x = f32((id << 1u) & 2u);
  let y = f32(id & 2u);
  return vec4<f32>(x * 2. - 1., 1. - y * 2., 0., 1.);
}
fn ycocg(c : vec3<f32>) -> vec3<f32> {
  let co = c.r - c.b;
  let t = c.b + co * 0.5;
  let cg = c.g - t;
  return vec3<f32>(t + cg * 0.5, co, cg);
}
fn rgb_from_ycocg(c : vec3<f32>) -> vec3<f32> {
  let t = c.x - c.z * 0.5;
  let g = c.z + t;
  let b = t - c.y * 0.5;
  return vec3<f32>(b + c.y, g, b);
}
@fragment fn fs(@builtin(position) pos : vec4<f32>) -> @location(0) vec4<f32> {
  let out_size = p.sizes.zw;
  let low_size = p.sizes.xy;
  let uv = pos.xy / out_size;
  // The rendered frame carries a sub-pixel projection offset; sample where the
  // un-jittered point actually landed.
  let cur_uv = clamp(uv + p.frame.xy, vec2<f32>(0.), vec2<f32>(1.));
  let center = textureSampleLevel(current_color, samp, cur_uv, 0.).rgb;

  // 3x3 current-frame neighbourhood in YCoCg: the clip box for history.
  var mn = vec3<f32>(1000000.);
  var mx = vec3<f32>(-1000000.);
  let texel = vec2<f32>(1.) / low_size;
  for (var dy : i32 = -1; dy <= 1; dy = dy + 1) {
    for (var dx : i32 = -1; dx <= 1; dx = dx + 1) {
      let offset = vec2<f32>(f32(dx), f32(dy)) * texel;
      let sample_uv = clamp(cur_uv + offset, vec2<f32>(0.), vec2<f32>(1.));
      let c = ycocg(textureSampleLevel(current_color, samp, sample_uv, 0.).rgb);
      mn = min(mn, c);
      mx = max(mx, c);
    }
  }

  let low_px = vec2<i32>(clamp(cur_uv * low_size, vec2<f32>(0.), low_size - vec2<f32>(1.)));
  let d = textureLoad(current_depth, low_px, 0);
  let ndc = vec4<f32>(cur_uv.x * 2. - 1., 1. - cur_uv.y * 2., d, 1.);
  let world = p.inv_vp * ndc;
  let prev_clip = p.prev_vp * (world / world.w);
  let behind = prev_clip.w <= 0.000001;
  let prev_ndc = prev_clip.xyz / max(prev_clip.w, 0.000001);
  let prev_uv = vec2<f32>(prev_ndc.x * 0.5 + 0.5, 0.5 - prev_ndc.y * 0.5);
  let prev_safe = clamp(prev_uv, vec2<f32>(0.), vec2<f32>(1.));
  let in_bounds = all(prev_uv >= vec2<f32>(0.)) && all(prev_uv <= vec2<f32>(1.));

  // The client uses reversed depth (near 1, far 0); a large reprojected depth
  // difference means the point was occluded in the previous frame.
  let prev_px = vec2<i32>(clamp(prev_safe * low_size, vec2<f32>(0.), low_size - vec2<f32>(1.)));
  let pd = textureLoad(previous_depth, prev_px, 0);
  let depth_ok = abs(d - pd) < p.tuning.y * (0.25 + d * 4.);
  let motion = length((prev_uv - uv) * out_size);
  let motion_ok = motion < out_size.x * 0.35;

  let history_rgb = textureSampleLevel(history_color, samp, prev_safe, 0.).rgb;
  let clipped = clamp(ycocg(history_rgb), mn - vec3<f32>(0.02), mx + vec3<f32>(0.02));
  let clipped_rgb = max(rgb_from_ycocg(clipped), vec3<f32>(0.));
  let luma_diff = abs(dot(center - clipped_rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
  let reactive = clamp(luma_diff * 2. + select(0.5, 0., depth_ok), 0., 1.);

  var weight = 0.;
  if (p.frame.z > 0.5 && in_bounds && depth_ok && motion_ok && !behind) {
    weight = p.tuning.x * (1. - reactive);
  }
  let blend = clamp(weight, 0., 0.95);
  return vec4<f32>(center + (clipped_rgb - center) * blend, 1.);
}
"#;

const PRESENT_WGSL: &str = r#"
struct S { params : vec4<f32> }
@group(0) @binding(0) var frame : texture_2d<f32>;
@group(0) @binding(1) var<uniform> s : S;
@group(0) @binding(2) var samp : sampler;
@vertex fn vs(@builtin(vertex_index) id:u32) -> @builtin(position) vec4<f32> {
  let x = f32((id << 1u) & 2u);
  let y = f32(id & 2u);
  return vec4<f32>(x * 2. - 1., 1. - y * 2., 0., 1.);
}
// Contrast-adaptive unsharp mask, clamped to the local 5-tap min/max (RCAS-like).
@fragment fn fs(@builtin(position) pos : vec4<f32>) -> @location(0) vec4<f32> {
  let texel = vec2<f32>(1.) / s.params.zw;
  let uv = pos.xy / s.params.zw;
  let c = textureSampleLevel(frame, samp, uv, 0.).rgb;
  let l = textureSampleLevel(frame, samp, clamp(uv - vec2<f32>(texel.x, 0.), vec2<f32>(0.), vec2<f32>(1.)), 0.).rgb;
  let r = textureSampleLevel(frame, samp, clamp(uv + vec2<f32>(texel.x, 0.), vec2<f32>(0.), vec2<f32>(1.)), 0.).rgb;
  let u = textureSampleLevel(frame, samp, clamp(uv - vec2<f32>(0., texel.y), vec2<f32>(0.), vec2<f32>(1.)), 0.).rgb;
  let d = textureSampleLevel(frame, samp, clamp(uv + vec2<f32>(0., texel.y), vec2<f32>(0.), vec2<f32>(1.)), 0.).rgb;
  let mn = min(min(l, r), min(min(u, d), c));
  let mx = max(max(l, r), max(max(u, d), c));
  let high = 4. * c - (l + r + u + d);
  let sharpened = clamp(c + high * s.params.x * 0.25, mn, mx);
  return vec4<f32>(sharpened, 1.);
}
"#;

const WARP_WGSL: &str = r#"
struct Params {
  inv_vp : mat4x4<f32>,
  prev_vp : mat4x4<f32>,
  sizes : vec4<f32>,
  frame : vec4<f32>,
  tuning : vec4<f32>,
}
@group(0) @binding(0) var cur_frame : texture_2d<f32>;
@group(0) @binding(1) var prev_frame : texture_2d<f32>;
@group(0) @binding(2) var scene_depth : texture_depth_2d;
@group(0) @binding(3) var samp : sampler;
@group(0) @binding(4) var<uniform> p : Params;
@vertex fn vs(@builtin(vertex_index) id:u32) -> @builtin(position) vec4<f32> {
  let x = f32((id << 1u) & 2u);
  let y = f32(id & 2u);
  return vec4<f32>(x * 2. - 1., 1. - y * 2., 0., 1.);
}
// Camera-motion interpolation between the two most recent upscaled frames.
// Vendor FG uses optical flow / extrapolation; this portable path reprojects
// with the current depth buffer and limits the motion to a quarter screen.
@fragment fn fs(@builtin(position) pos : vec4<f32>) -> @location(0) vec4<f32> {
  let out_size = p.sizes.zw;
  let low_size = p.sizes.xy;
  let uv = pos.xy / out_size;
  let alpha = clamp(p.frame.z, 0., 1.);
  let low_px = vec2<i32>(clamp(uv * low_size, vec2<f32>(0.), low_size - vec2<f32>(1.)));
  let d = textureLoad(scene_depth, low_px, 0);
  let ndc = vec4<f32>(uv.x * 2. - 1., 1. - uv.y * 2., d, 1.);
  let world = p.inv_vp * ndc;
  let prev_clip = p.prev_vp * (world / world.w);
  let behind = prev_clip.w <= 0.000001;
  let prev_ndc = prev_clip.xyz / max(prev_clip.w, 0.000001);
  let prev_uv = vec2<f32>(prev_ndc.x * 0.5 + 0.5, 0.5 - prev_ndc.y * 0.5);
  let m = clamp(uv - prev_uv, vec2<f32>(-0.25), vec2<f32>(0.25));
  let cur_sample_uv = clamp(uv + (1. - alpha) * m, vec2<f32>(0.), vec2<f32>(1.));
  let prev_sample_uv = clamp(uv - alpha * m, vec2<f32>(0.), vec2<f32>(1.));
  let c1 = textureSampleLevel(cur_frame, samp, cur_sample_uv, 0.).rgb;
  let c0 = textureSampleLevel(prev_frame, samp, prev_sample_uv, 0.).rgb;
  let in_bounds = all(prev_uv >= vec2<f32>(0.)) && all(prev_uv <= vec2<f32>(1.));
  let valid = in_bounds && !behind;
  let warped = c0 + (c1 - c0) * alpha;
  let still = textureSampleLevel(cur_frame, samp, uv, 0.).rgb;
  return vec4<f32>(select(still, warped, valid), 1.);
}
"#;

fn color_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    let alternate = [format.remove_srgb_suffix()];
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("temporal upscaler history"),
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

fn depth_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("temporal upscaler depth history"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn color_binding(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn depth_binding(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_binding(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_binding(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    wgsl: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
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
    })
}

/// Runtime state of the temporal upscaler / frame interpolator.
pub struct UpscaleRuntime {
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    low_width: u32,
    low_height: u32,
    history: [wgpu::Texture; 2],
    depth_history: [wgpu::Texture; 2],
    history_write: usize,
    depth_write: usize,
    /// Most recent (`current_history`) and previous (`previous_history`) full
    /// resolution upscaled frames, used by the frame-generation warp.
    current_history: usize,
    previous_history: usize,
    frame_index: u32,
    frames_written: u32,
    prev_vp: Option<[[f32; 4]; 4]>,
    inv_current: [[f32; 4]; 4],
    mode: Upscaler,
    temporal_layout: wgpu::BindGroupLayout,
    present_layout: wgpu::BindGroupLayout,
    warp_layout: wgpu::BindGroupLayout,
    temporal_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    warp_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    temporal_uniform: wgpu::Buffer,
    present_uniform: wgpu::Buffer,
    warp_uniform: wgpu::Buffer,
}

impl UpscaleRuntime {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        low_width: u32,
        low_height: u32,
    ) -> Self {
        let temporal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("temporal upscaler layout"),
            entries: &[
                color_binding(0, true),
                color_binding(1, true),
                depth_binding(2),
                depth_binding(3),
                sampler_binding(4),
                uniform_binding(5),
            ],
        });
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("upscaler sharpen layout"),
            entries: &[
                color_binding(0, true),
                uniform_binding(1),
                sampler_binding(2),
            ],
        });
        let warp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame generation layout"),
            entries: &[
                color_binding(0, true),
                color_binding(1, true),
                depth_binding(2),
                sampler_binding(3),
                uniform_binding(4),
            ],
        });
        let temporal_pipeline = pipeline(
            device,
            format,
            &temporal_layout,
            "temporal upscaler WGSL",
            TEMPORAL_WGSL,
        );
        let present_pipeline = pipeline(
            device,
            format,
            &present_layout,
            "upscaler sharpen WGSL",
            PRESENT_WGSL,
        );
        let warp_pipeline = pipeline(
            device,
            format,
            &warp_layout,
            "frame generation WGSL",
            WARP_WGSL,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("upscaler linear sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniform = |label, bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let zero = Params {
            inv_vp: [[0.; 4]; 4],
            prev_vp: [[0.; 4]; 4],
            sizes: [1., 1., 1., 1.],
            frame: [0.; 4],
            tuning: [0.; 4],
        };
        let temporal_uniform = uniform("temporal upscaler params", bytemuck::bytes_of(&zero));
        let warp_uniform = uniform("frame generation params", bytemuck::bytes_of(&zero));
        let present_uniform = uniform(
            "upscaler sharpen params",
            bytemuck::bytes_of(&SharpenParams {
                params: [0.5, 0., width.max(1) as f32, height.max(1) as f32],
            }),
        );
        Self {
            format,
            width,
            height,
            low_width,
            low_height,
            history: [
                color_texture(device, format, width, height),
                color_texture(device, format, width, height),
            ],
            depth_history: [
                depth_texture(device, low_width, low_height),
                depth_texture(device, low_width, low_height),
            ],
            history_write: 0,
            depth_write: 0,
            current_history: 0,
            previous_history: 1,
            frame_index: 0,
            frames_written: 0,
            prev_vp: None,
            inv_current: [[0.; 4]; 4],
            mode: Upscaler::Off,
            temporal_layout,
            present_layout,
            warp_layout,
            temporal_pipeline,
            present_pipeline,
            warp_pipeline,
            sampler,
            temporal_uniform,
            present_uniform,
            warp_uniform,
        }
    }

    /// Frame index for the Halton jitter; gpu.rs must apply the same index to
    /// the projection matrix before rendering the low-resolution world.
    pub fn frame_index(&self) -> u32 {
        self.frame_index
    }

    /// Drop all temporal history. Used on resize, mode changes and first use.
    pub fn reset(&mut self) {
        self.prev_vp = None;
        self.frames_written = 0;
        self.frame_index = 0;
        self.history_write = 0;
        self.depth_write = 0;
        self.current_history = 0;
        self.previous_history = 1;
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        low_width: u32,
        low_height: u32,
    ) {
        self.width = width;
        self.height = height;
        self.low_width = low_width;
        self.low_height = low_height;
        self.history = [
            color_texture(device, self.format, width, height),
            color_texture(device, self.format, width, height),
        ];
        self.depth_history = [
            depth_texture(device, low_width, low_height),
            depth_texture(device, low_width, low_height),
        ];
        self.reset();
    }

    /// `can_interpolate` is false until two upscaled frames exist, so generated
    /// frames never sample uninitialised history.
    pub fn can_interpolate(&self) -> bool {
        self.frames_written >= 2 && self.prev_vp.is_some()
    }

    /// Replace the bilinear `RenderScale::blit` for one real frame.
    ///
    /// `view_proj` must be the jittered matrix the world was rendered with and
    /// `inv_view_proj` its inverse. `depth` is the low-resolution scene depth
    /// texture (`Renderer::depth`), which must have `TEXTURE_BINDING` usage.
    pub fn upscale(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color: &wgpu::TextureView,
        depth: &wgpu::Texture,
        out: &wgpu::TextureView,
        inv_view_proj: [[f32; 4]; 4],
        view_proj: [[f32; 4]; 4],
        mode: Upscaler,
        sharpness: f32,
    ) {
        if mode != self.mode {
            self.mode = mode;
            self.reset();
        }
        let read = 1 - self.history_write;
        let write = self.history_write;
        let depth_read = 1 - self.depth_write;
        let depth_write = self.depth_write;
        let jitter = upscaling::jitter_uv(self.frame_index, self.low_width, self.low_height);
        let (max_weight, depth_tolerance) = mode.tuning();
        let params = Params {
            inv_vp: inv_view_proj,
            prev_vp: self.prev_vp.unwrap_or(view_proj),
            sizes: [
                self.low_width.max(1) as f32,
                self.low_height.max(1) as f32,
                self.width.max(1) as f32,
                self.height.max(1) as f32,
            ],
            frame: [
                jitter[0],
                jitter[1],
                if self.prev_vp.is_some() { 1. } else { 0. },
                self.frame_index as f32,
            ],
            tuning: [max_weight, depth_tolerance, mode.profile(), 1.],
        };
        queue.write_buffer(&self.temporal_uniform, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(
            &self.present_uniform,
            0,
            bytemuck::bytes_of(&SharpenParams {
                params: [
                    sharpness.clamp(0., 1.),
                    0.,
                    self.width.max(1) as f32,
                    self.height.max(1) as f32,
                ],
            }),
        );

        let history_read = self.history[read].create_view(&Default::default());
        let history_write = self.history[write].create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let previous_depth = self.depth_history[depth_read].create_view(&Default::default());
        let temporal_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("temporal upscaler inputs"),
            layout: &self.temporal_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&history_read),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&previous_depth),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.temporal_uniform.as_entire_binding(),
                },
            ],
        });
        let present_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("upscaler sharpen inputs"),
            layout: &self.present_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&history_write),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.present_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        // Keep a copy of this frame's depth for next frame's disocclusion test.
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: depth,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            },
            wgpu::ImageCopyTexture {
                texture: &self.depth_history[depth_write],
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            },
            wgpu::Extent3d {
                width: self.low_width.max(1),
                height: self.low_height.max(1),
                depth_or_array_layers: 1,
            },
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("temporal upscale accumulation"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &history_write,
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
            pass.set_pipeline(&self.temporal_pipeline);
            pass.set_bind_group(0, &temporal_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("upscaler sharpen and present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out,
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
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &present_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));

        self.prev_vp = Some(view_proj);
        self.inv_current = inv_view_proj;
        self.current_history = write;
        self.previous_history = read;
        self.history_write = read;
        self.depth_write = depth_read;
        self.frame_index = self.frame_index.wrapping_add(1);
        self.frames_written = self.frames_written.saturating_add(1);
    }

    /// Present one generated frame for `alpha` in (0, 1), without rendering the
    /// world. Called `multiplier - 1` times after each real frame.
    pub fn warp(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        out: &wgpu::TextureView,
        depth: &wgpu::Texture,
        alpha: f32,
    ) {
        let Some(prev_vp) = self.prev_vp else {
            return;
        };
        let params = Params {
            inv_vp: self.inv_current,
            prev_vp,
            sizes: [
                self.low_width.max(1) as f32,
                self.low_height.max(1) as f32,
                self.width.max(1) as f32,
                self.height.max(1) as f32,
            ],
            frame: [0., 0., alpha.clamp(0., 1.), 0.],
            tuning: [0.; 4],
        };
        queue.write_buffer(&self.warp_uniform, 0, bytemuck::bytes_of(&params));
        let cur = self.history[self.current_history].create_view(&Default::default());
        let prev = self.history[self.previous_history].create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame generation inputs"),
            layout: &self.warp_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cur),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&prev),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.warp_uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("generated frame warp"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out,
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
        queue.submit(Some(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_binding_sets_match_the_layouts() {
        // A missing/extra binding here is a wgpu validation error at runtime,
        // so keep the WGSL declarations and the Rust layouts in lockstep.
        for (wgsl, count) in [
            (TEMPORAL_WGSL, 6usize),
            (PRESENT_WGSL, 3usize),
            (WARP_WGSL, 5usize),
        ] {
            let declared = (0..count).all(|b| wgsl.contains(&format!("@binding({b})")));
            assert!(declared, "a WGSL binding is missing");
            assert!(wgsl.contains("@vertex fn vs"));
            assert!(wgsl.contains("@fragment fn fs"));
        }
        assert!(TEMPORAL_WGSL.contains("texture_depth_2d"));
        assert!(WARP_WGSL.contains("texture_depth_2d"));
    }

    #[test]
    fn uniform_layout_is_aligned_for_wgsl_matrices() {
        assert_eq!(std::mem::size_of::<Params>(), 192);
        assert_eq!(std::mem::size_of::<SharpenParams>(), 16);
        assert_eq!(std::mem::offset_of!(Params, sizes), 128);
        assert_eq!(std::mem::offset_of!(Params, frame), 144);
        assert_eq!(std::mem::offset_of!(Params, tuning), 160);
    }

    #[test]
    fn profiles_are_distinct_and_bounded() {
        for mode in Upscaler::ALL {
            let (weight, tolerance) = mode.tuning();
            assert!((0. ..=0.95).contains(&weight));
            assert!(tolerance > 0. && tolerance < 0.1);
        }
        assert_ne!(Upscaler::Fsr2.profile(), Upscaler::Fsr41.profile());
        assert_ne!(Upscaler::Dlss.profile(), Upscaler::Xess.profile());
    }
}
