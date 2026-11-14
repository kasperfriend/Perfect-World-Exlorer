//! Windows + DX12 implementation of the FSR upscaler. See the module-level
//! documentation in `super` for the overall design and its limits.

use glam::Mat4;
use std::ffi::c_void;
use winapi::shared::dxgiformat::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R32_FLOAT,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use winapi::um::d3d12 as d3d12_ty;

const FFX_UPSCALER_VERSION: u32 = (4 << 22) | (1 << 12) | 2;
const FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE: u64 = 0x00010000;
const FFX_API_DISPATCH_DESC_TYPE_UPSCALE: u64 = 0x00010001;
const FFX_API_QUERY_DESC_TYPE_UPSCALE_GETJITTERPHASECOUNT: u64 = 0x00010004;
const FFX_API_QUERY_DESC_TYPE_UPSCALE_GETJITTEROFFSET: u64 = 0x00010005;
const FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12: u64 = 0x00000002;
const FFX_API_QUERY_DESC_TYPE_GET_VERSIONS: u64 = 4;
const FFX_API_DESC_TYPE_OVERRIDE_VERSION: u64 = 5;
const FFX_API_RESOURCE_STATE_UNORDERED_ACCESS: u32 = 1 << 1;
const FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ: u32 = (1 << 2) | (1 << 3);
const FFX_API_RESOURCE_USAGE_READ_ONLY: u32 = 0;
const FFX_API_RESOURCE_USAGE_UAV: u32 = 1 << 1;
const FFX_API_RESOURCE_USAGE_DEPTHTARGET: u32 = 1 << 2;
const FFX_API_RESOURCE_TYPE_TEXTURE2D: u32 = 2;
const FFX_API_SURFACE_FORMAT_R16G16_FLOAT: u32 = 18;
const FFX_API_SURFACE_FORMAT_R32_FLOAT: u32 = 28;
const FFX_API_SURFACE_FORMAT_B8G8R8A8_SRGB: u32 = 15;
const FFX_API_SURFACE_FORMAT_R8G8B8A8_SRGB: u32 = 12;
const FFX_UPSCALE_ENABLE_DEPTH_INVERTED: u32 = 1 << 3;
const FFX_UPSCALE_ENABLE_NON_LINEAR_COLORSPACE: u32 = 1 << 8;
const FFX_UPSCALE_FLAG_NON_LINEAR_COLOR_SRGB: u32 = 1 << 1;

// D3D12 states the frame loop leaves the native targets in before the FFX
// command list runs, and the states it restores them to afterwards so wgpu's
// own tracker stays truthful on the following frame.
const D3D12_STATE_COMMON: u32 = d3d12_ty::D3D12_RESOURCE_STATE_COMMON;
const D3D12_STATE_COPY_DEST: u32 = d3d12_ty::D3D12_RESOURCE_STATE_COPY_DEST;
const D3D12_STATE_RENDER_TARGET: u32 = d3d12_ty::D3D12_RESOURCE_STATE_RENDER_TARGET;
const D3D12_STATE_UNORDERED_ACCESS: u32 = d3d12_ty::D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
const D3D12_STATE_PIXEL_COMPUTE_READ: u32 = d3d12_ty::D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
    | d3d12_ty::D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;

#[repr(C)]
#[derive(Clone, Copy)]
struct FfxApiHeader {
    r#type: u64,
    p_next: *mut FfxApiHeader,
}

impl FfxApiHeader {
    fn new(r#type: u64) -> Self {
        Self {
            r#type,
            p_next: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfxApiDimensions2D {
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfxApiFloatCoords2D {
    x: f32,
    y: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfxApiResourceDescription {
    r#type: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    mip_count: u32,
    flags: u32,
    usage: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfxApiResource {
    resource: *mut c_void,
    description: FfxApiResourceDescription,
    state: u32,
}

impl FfxApiResource {
    const EMPTY: FfxApiResource = FfxApiResource {
        resource: std::ptr::null_mut(),
        description: FfxApiResourceDescription {
            r#type: 0,
            format: 0,
            width: 0,
            height: 0,
            depth: 0,
            mip_count: 0,
            flags: 0,
            usage: 0,
        },
        state: 0,
    };

    fn texture2d(
        resource: &d3d12::Resource,
        format: u32,
        width: u32,
        height: u32,
        usage: u32,
        state: u32,
    ) -> Self {
        Self {
            resource: resource.as_mut_ptr() as *mut c_void,
            description: FfxApiResourceDescription {
                r#type: FFX_API_RESOURCE_TYPE_TEXTURE2D,
                format,
                width,
                height,
                depth: 1,
                mip_count: 1,
                flags: 0,
                usage,
            },
            state,
        }
    }
}

#[repr(C)]
struct FfxCreateContextDescUpscale {
    header: FfxApiHeader,
    flags: u32,
    max_render_size: FfxApiDimensions2D,
    max_upscale_size: FfxApiDimensions2D,
    fp_message: *mut c_void,
}

#[repr(C)]
struct FfxCreateBackendDX12Desc {
    header: FfxApiHeader,
    device: *mut c_void,
}

#[repr(C)]
struct FfxOverrideVersion {
    header: FfxApiHeader,
    version_id: u64,
}

#[repr(C)]
struct FfxQueryDescGetVersions {
    header: FfxApiHeader,
    create_desc_type: u64,
    device: *mut c_void,
    output_count: *mut u64,
    version_ids: *mut u64,
    version_names: *mut *const std::os::raw::c_char,
}

#[repr(C)]
struct FfxQueryDescUpscaleGetJitterPhaseCount {
    header: FfxApiHeader,
    render_width: u32,
    display_width: u32,
    out_phase_count: *mut i32,
}

#[repr(C)]
struct FfxQueryDescUpscaleGetJitterOffset {
    header: FfxApiHeader,
    index: i32,
    phase_count: i32,
    out_x: *mut f32,
    out_y: *mut f32,
}

#[repr(C)]
struct FfxDispatchDescUpscale {
    header: FfxApiHeader,
    command_list: *mut c_void,
    color: FfxApiResource,
    depth: FfxApiResource,
    motion_vectors: FfxApiResource,
    exposure: FfxApiResource,
    reactive: FfxApiResource,
    transparency_and_composition: FfxApiResource,
    output: FfxApiResource,
    jitter_offset: FfxApiFloatCoords2D,
    motion_vector_scale: FfxApiFloatCoords2D,
    render_size: FfxApiDimensions2D,
    upscale_size: FfxApiDimensions2D,
    enable_sharpening: bool,
    sharpness: f32,
    frame_time_delta: f32,
    pre_exposure: f32,
    reset: bool,
    camera_near: f32,
    camera_far: f32,
    camera_fov_angle_vertical: f32,
    view_space_to_meters_factor: f32,
    flags: u32,
}

type FfxReturnCode = u32;
const FFX_API_RETURN_OK: FfxReturnCode = 0;

type PfnFfxCreateContext =
    unsafe extern "C" fn(*mut *mut c_void, *mut FfxApiHeader, *const c_void) -> FfxReturnCode;
type PfnFfxDestroyContext = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> FfxReturnCode;
type PfnFfxQuery = unsafe extern "C" fn(*mut *mut c_void, *mut FfxApiHeader) -> FfxReturnCode;
type PfnFfxDispatch = unsafe extern "C" fn(*mut *mut c_void, *const FfxApiHeader) -> FfxReturnCode;

extern "C" fn fsr_message_callback(message_type: u32, message: *const u16) {
    if message.is_null() {
        return;
    }
    let len = unsafe {
        let mut len = 0usize;
        while *message.add(len) != 0 && len < 4096 {
            len += 1;
        }
        len
    };
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(message, len) });
    eprintln!(
        "[FSR {}] {text}",
        if message_type == 0 {
            "error"
        } else {
            "warning"
        }
    );
}

struct FfxLoader {
    handle: winapi::shared::minwindef::HMODULE,
    create_context: PfnFfxCreateContext,
    destroy_context: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
}

impl Drop for FfxLoader {
    fn drop(&mut self) {
        unsafe {
            winapi::um::libloaderapi::FreeLibrary(self.handle);
        }
    }
}

impl FfxLoader {
    fn load(sdk_dir: &str) -> Result<FfxLoader, String> {
        use std::os::windows::ffi::OsStrExt;
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        let trimmed = sdk_dir.trim();
        if !trimmed.is_empty() {
            candidates.push(std::path::Path::new(trimmed).join("amd_fidelityfx_loader_dx12.dll"));
        }
        if let Some(dir) = std::env::var_os("PW_UPSCALER_SDK_DIR") {
            candidates.push(std::path::Path::new(&dir).join("amd_fidelityfx_loader_dx12.dll"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("sdk").join("amd_fidelityfx_loader_dx12.dll"));
            }
        }
        candidates.push(std::path::PathBuf::from("sdk").join("amd_fidelityfx_loader_dx12.dll"));
        let mut errors: Vec<String> = Vec::new();
        for candidate in candidates {
            if !candidate.is_file() {
                errors.push(format!("{}: not found", candidate.display()));
                continue;
            }
            let wide: Vec<u16> = candidate
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { winapi::um::libloaderapi::LoadLibraryW(wide.as_ptr()) };
            if handle.is_null() {
                errors.push(format!("{}: LoadLibraryW failed", candidate.display()));
                continue;
            }
            let get = |name: &[u8]| -> Result<*const c_void, String> {
                let proc = unsafe {
                    winapi::um::libloaderapi::GetProcAddress(handle, name.as_ptr() as *const i8)
                };
                if proc.is_null() {
                    Err(format!(
                        "GetProcAddress({}) failed",
                        String::from_utf8_lossy(name)
                    ))
                } else {
                    Ok(proc as *const c_void)
                }
            };
            let loaded = (|| {
                Ok(FfxLoader {
                    handle,
                    create_context: unsafe { std::mem::transmute(get(b"ffxCreateContext\0")?) },
                    destroy_context: unsafe { std::mem::transmute(get(b"ffxDestroyContext\0")?) },
                    query: unsafe { std::mem::transmute(get(b"ffxQuery\0")?) },
                    dispatch: unsafe { std::mem::transmute(get(b"ffxDispatch\0")?) },
                })
            })();
            match loaded {
                Ok(loader) => return Ok(loader),
                Err(error) => {
                    unsafe { winapi::um::libloaderapi::FreeLibrary(handle) };
                    errors.push(format!("{}: {error}", candidate.display()));
                }
            }
        }
        Err(format!(
            "FFX loader could not be loaded ({})",
            errors.join("; ")
        ))
    }
}

fn is_bgra_format(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
    )
}

fn ffx_color_format(format: wgpu::TextureFormat) -> u32 {
    if is_bgra_format(format) {
        FFX_API_SURFACE_FORMAT_B8G8R8A8_SRGB
    } else {
        FFX_API_SURFACE_FORMAT_R8G8B8A8_SRGB
    }
}

fn dxgi_color_format(format: wgpu::TextureFormat) -> DXGI_FORMAT {
    if is_bgra_format(format) {
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
    } else {
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
    }
}

fn hr_ok(hr: winapi::shared::winerror::HRESULT) -> bool {
    hr >= 0
}

/// Committed texture + the wgpu import that mirrors it, so both the native
/// FFX command list and wgpu can address the same allocation.
struct NativeTexture2D {
    resource: d3d12::Resource,
    texture: wgpu::Texture,
}

impl NativeTexture2D {
    /// # Safety
    /// The caller guarantees the wgpu device was created by `raw_device`.
    unsafe fn create(
        device: &wgpu::Device,
        raw_device: &d3d12::Device,
        label: &str,
        format: wgpu::TextureFormat,
        dxgi_format: DXGI_FORMAT,
        size: (u32, u32),
        resource_flags: d3d12_ty::D3D12_RESOURCE_FLAGS,
        wgpu_usage: wgpu::TextureUsages,
    ) -> Result<NativeTexture2D, String> {
        use winapi::shared::dxgitype::DXGI_SAMPLE_DESC;
        use winapi::Interface;
        let desc = d3d12_ty::D3D12_RESOURCE_DESC {
            Dimension: d3d12_ty::D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0, // D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT
            Width: u64::from(size.0),
            Height: size.1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: dxgi_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: d3d12_ty::D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: resource_flags,
        };
        let heap = d3d12_ty::D3D12_HEAP_PROPERTIES {
            Type: d3d12_ty::D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: d3d12_ty::D3D12_CPU_PAGE_PROPERTY_NOT_AVAILABLE,
            MemoryPoolPreference: d3d12_ty::D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };
        let mut resource_ptr: *mut d3d12_ty::ID3D12Resource = std::ptr::null_mut();
        let hr = raw_device.CreateCommittedResource(
            &heap,
            d3d12_ty::D3D12_HEAP_FLAG_NONE,
            &desc,
            d3d12_ty::D3D12_RESOURCE_STATE_COMMON,
            std::ptr::null(),
            &d3d12_ty::ID3D12Resource::uuidof(),
            &mut resource_ptr as *mut *mut d3d12_ty::ID3D12Resource as *mut *mut c_void,
        );
        if !hr_ok(hr) || resource_ptr.is_null() {
            return Err(format!(
                "CreateCommittedResource failed for {label} (HRESULT 0x{hr:08x})"
            ));
        }
        // ComPtr is transparent over the raw pointer; adopt the reference
        // CreateCommittedResource handed us instead of AddRef-ing a second.
        let resource: d3d12::Resource = std::mem::transmute(resource_ptr);
        let hal_texture = device
            .as_hal::<wgpu_hal::dx12::Api, _, _>(|maybe| {
                maybe.map(|_hal_device| {
                    let size3 = wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    };
                    // Associated function (not a method): creates the hal
                    // Texture wrapper around the committed resource.
                    unsafe {
                        wgpu_hal::dx12::Device::texture_from_raw(
                            resource.clone(),
                            format,
                            wgpu::TextureDimension::D2,
                            size3,
                            1,
                            1,
                        )
                    }
                })
            })
            .ok_or_else(|| format!("the wgpu device is not a DX12 device ({label})"))?
            .ok_or_else(|| format!("the wgpu device is not a DX12 device ({label})"))?;
        let import_desc = wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu_usage,
            view_formats: &[],
        };
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu_hal::dx12::Api>(hal_texture, &import_desc)
        };
        Ok(NativeTexture2D { resource, texture })
    }
}

const MV_SHADER: &str = r#"
struct MvParams {
    inv_vp: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    render_size: vec2<f32>,
    prev_valid: u32,
    _pad: u32,
};
@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<uniform> params: MvParams;
struct VOut { @builtin(position) pos: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) id: u32) -> VOut {
    var o: VOut;
    let x = f32((id << 1u) & 2u);
    let y = f32(id & 2u);
    o.pos = vec4<f32>(x * 2. - 1., 1. - y * 2., 0., 1.);
    return o;
}
struct FOut {
    // Motion vectors in render-resolution pixel space (rg16f).
    @location(0) mv: vec2<f32>,
    // Raw reversed-Z depth passthrough (r32f): the upscaler samples depth
    // straight from this copy so the input is a plain R32_FLOAT surface
    // instead of a depth-stencil-formatted one it cannot bind as an SRV.
    @location(1) raw_depth: f32,
};
@fragment fn fs(i: VOut) -> FOut {
    let size = params.render_size;
    let clamped = clamp(vec2<i32>(floor(i.pos.xy)), vec2<i32>(0, 0), vec2<i32>(size) - vec2<i32>(1, 1));
    let d = textureLoad(depth, clamped, 0);
    var out: FOut;
    out.raw_depth = d;
    out.mv = vec2<f32>(0.0, 0.0);
    if (params.prev_valid != 0u) {
        let px = floor(i.pos.xy) + vec2<f32>(0.5, 0.5);
        let uv = px / size;
        let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
        var world = params.inv_vp * ndc;
        if (abs(world.w) >= 1e-8) {
            world = world / world.w;
            let clip = params.prev_vp * world;
            if (abs(clip.w) >= 1e-8) {
                let prev_ndc = clip.xy / clip.w;
                let prev_uv = vec2<f32>((prev_ndc.x + 1.0) * 0.5, (1.0 - prev_ndc.y) * 0.5);
                out.mv = (uv - prev_uv) * size;
            }
        }
    }
    return out;
}
"#;

const COMPOSITE_SHADER: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) id: u32) -> VOut {
    var o: VOut;
    let x = f32((id << 1u) & 2u);
    let y = f32(id & 2u);
    o.pos = vec4<f32>(x * 2. - 1., 1. - y * 2., 0., 1.);
    o.uv = vec2<f32>(x, y);
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(frame, samp, i.uv).rgb, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FsrMvParams {
    inv_vp: [[f32; 4]; 4],
    prev_vp: [[f32; 4]; 4],
    render_size: [f32; 2],
    prev_valid: u32,
    pad: u32,
}

struct FsrMotionVectorPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
}

impl FsrMotionVectorPass {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fsr motion vector layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
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
            label: Some("fsr motion vector WGSL"),
            source: wgpu::ShaderSource::Wgsl(MV_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fsr motion vector pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fsr motion vector pass"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fsr motion vector params"),
            size: std::mem::size_of::<FsrMvParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            params,
        }
    }
}

struct FsrComposite {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl FsrComposite {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fsr composite layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fsr composite WGSL"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fsr composite pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fsr composite"),
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
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fsr composite sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Texture,
        target: &wgpu::TextureView,
    ) {
        let view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fsr composite source"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fsr composite"),
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

/// Pick the provider version id to force. Prefers the exact version the app
/// was built against; otherwise the newest enumerated provider wins.
fn choose_version(ids: &[u64]) -> Option<u64> {
    if ids.is_empty() {
        return None;
    }
    ids.iter()
        .copied()
        .find(|id| (*id & 0x7fff_ffff) == u64::from(FFX_UPSCALER_VERSION))
        .or_else(|| ids.iter().copied().max())
}

struct FsrNatives {
    color: NativeTexture2D,
    depth: NativeTexture2D,
    mv: NativeTexture2D,
    output: NativeTexture2D,
}

impl FsrNatives {
    /// # Safety
    /// `raw_device` must be the device `device` was created from.
    unsafe fn create(
        device: &wgpu::Device,
        raw_device: &d3d12::Device,
        render_extent: (u32, u32),
        display_extent: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Result<FsrNatives, String> {
        let color = NativeTexture2D::create(
            device,
            raw_device,
            "fsr color",
            format,
            dxgi_color_format(format),
            render_extent,
            d3d12_ty::D3D12_RESOURCE_FLAG_NONE,
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        )?;
        let depth = NativeTexture2D::create(
            device,
            raw_device,
            "fsr depth",
            wgpu::TextureFormat::R32Float,
            DXGI_FORMAT_R32_FLOAT,
            render_extent,
            d3d12_ty::D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )?;
        let mv = NativeTexture2D::create(
            device,
            raw_device,
            "fsr motion vectors",
            wgpu::TextureFormat::Rg16Float,
            DXGI_FORMAT_R16G16_FLOAT,
            render_extent,
            d3d12_ty::D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )?;
        let output = NativeTexture2D::create(
            device,
            raw_device,
            "fsr output",
            format,
            dxgi_color_format(format),
            display_extent,
            d3d12_ty::D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        )?;
        Ok(FsrNatives {
            color,
            depth,
            mv,
            output,
        })
    }
}

pub struct FsrUpscaler {
    loader: FfxLoader,
    queue: d3d12::CommandQueue,
    allocator: d3d12::CommandAllocator,
    list: d3d12::GraphicsCommandList,
    context: *mut c_void,
    format: wgpu::TextureFormat,
    render_extent: (u32, u32),
    display_extent: (u32, u32),
    natives: FsrNatives,
    mv_pass: FsrMotionVectorPass,
    composite: FsrComposite,
    frame_index: u64,
    phase_count: i32,
    current_jitter: (f32, f32),
    prev_view_proj: Option<Mat4>,
    reset_history: bool,
    last_instant: std::time::Instant,
}

impl Drop for FsrUpscaler {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe {
                (self.loader.destroy_context)(&mut self.context, std::ptr::null());
            }
        }
    }
}

impl FsrUpscaler {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sdk_dir: &str,
        render_extent: (u32, u32),
        display_extent: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Result<FsrUpscaler, String> {
        let _ = queue;
        let loader = FfxLoader::load(sdk_dir)?;
        // Owned clones of the hal handles; the originals belong to wgpu.
        let raw_device = unsafe {
            device
                .as_hal::<wgpu_hal::dx12::Api, _, _>(|maybe| {
                    maybe.map(|hal_device| hal_device.raw_device().clone())
                })
                .ok_or_else(|| "the wgpu device is not a DX12 device".to_string())?
                .ok_or_else(|| "the wgpu device is not a DX12 device".to_string())?
        };
        let raw_queue = unsafe {
            device
                .as_hal::<wgpu_hal::dx12::Api, _, _>(|maybe| {
                    maybe.map(|hal_device| hal_device.raw_queue().clone())
                })
                .ok_or_else(|| "the wgpu device is not a DX12 device".to_string())?
                .ok_or_else(|| "the wgpu device is not a DX12 device".to_string())?
        };
        let device_ptr = raw_device.as_mut_ptr() as *mut c_void;

        // Enumerate the providers the loader can reach and force the newest
        // FSR 4.1-compatible one available; GetVersions needs only the device
        // pointer carried by the query desc itself.
        let mut count: u64 = 0;
        let mut probe = FfxQueryDescGetVersions {
            header: FfxApiHeader::new(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS),
            create_desc_type: FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE,
            device: device_ptr,
            output_count: &mut count,
            version_ids: std::ptr::null_mut(),
            version_names: std::ptr::null_mut(),
        };
        let mut provider_note = String::new();
        let rc = unsafe { (loader.query)(std::ptr::null_mut(), &mut probe.header) };
        let mut chosen_version: Option<u64> = None;
        if rc == FFX_API_RETURN_OK && count > 0 {
            let mut ids = vec![0u64; (count as usize).min(1024)];
            count = count.min(1024);
            count = 0;
            let mut probe = FfxQueryDescGetVersions {
                header: FfxApiHeader::new(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS),
                create_desc_type: FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE,
                device: device_ptr,
                output_count: &mut count,
                version_ids: ids.as_mut_ptr(),
                version_names: std::ptr::null_mut(),
            };
            let rc = unsafe { (loader.query)(std::ptr::null_mut(), &mut probe.header) };
            if rc == FFX_API_RETURN_OK {
                ids.truncate(count as usize);
                chosen_version = choose_version(&ids);
                provider_note = format!(
                    "providers {:?}, selected 0x{:x}",
                    ids.iter().map(|i| format!("0x{i:x}")).collect::<Vec<_>>(),
                    chosen_version.unwrap_or(0)
                );
            }
        } else {
            eprintln!(
                "[FSR] provider enumeration returned 0x{rc:x}; the loader will pick a provider"
            );
        }

        let (allocator, hr) = raw_device.create_command_allocator(d3d12::CmdListType::Direct);
        if hr < 0 {
            return Err(format!(
                "create_command_allocator failed (HRESULT 0x{hr:08x})"
            ));
        }
        let (list, hr) = raw_device.create_graphics_command_list(
            d3d12::CmdListType::Direct,
            &allocator,
            d3d12::PipelineState::null(),
            0,
        );
        if hr < 0 {
            return Err(format!(
                "create_graphics_command_list failed (HRESULT 0x{hr:08x})"
            ));
        }
        // Close the still-open list so every frame can follow the
        // allocator.reset() + list.reset() pattern used for reused lists.
        let hr = list.close();
        if hr < 0 {
            return Err(format!(
                "ID3D12GraphicsCommandList::Close failed (HRESULT 0x{hr:08x})"
            ));
        }

        let mut backend = FfxCreateBackendDX12Desc {
            header: FfxApiHeader::new(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12),
            device: device_ptr,
        };
        let mut override_version = FfxOverrideVersion {
            header: FfxApiHeader::new(FFX_API_DESC_TYPE_OVERRIDE_VERSION),
            version_id: chosen_version.unwrap_or(0),
        };
        let mut upscale = FfxCreateContextDescUpscale {
            header: FfxApiHeader::new(FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE),
            flags: FFX_UPSCALE_ENABLE_DEPTH_INVERTED | FFX_UPSCALE_ENABLE_NON_LINEAR_COLORSPACE,
            max_render_size: FfxApiDimensions2D {
                width: render_extent.0,
                height: render_extent.1,
            },
            max_upscale_size: FfxApiDimensions2D {
                width: display_extent.0,
                height: display_extent.1,
            },
            fp_message: fsr_message_callback as usize as *mut c_void,
        };
        backend.header.p_next = std::ptr::null_mut();
        if chosen_version.is_some() {
            override_version.header.p_next = &mut backend.header as *mut FfxApiHeader;
            upscale.header.p_next = &mut override_version.header as *mut FfxApiHeader;
        } else {
            upscale.header.p_next = &mut backend.header as *mut FfxApiHeader;
        }
        let mut context: *mut c_void = std::ptr::null_mut();
        let rc =
            unsafe { (loader.create_context)(&mut context, &mut upscale.header, std::ptr::null()) };
        if rc != FFX_API_RETURN_OK || context.is_null() {
            return Err(format!("ffxCreateContext failed (0x{rc:x}); is amd_fidelityfx_upscaler_dx12.dll next to the loader?"));
        }
        eprintln!("[FSR] upscaler context 0x{context:x} created ({provider_note})");

        let mut phase_count: i32 = 0;
        let mut phase_query = FfxQueryDescUpscaleGetJitterPhaseCount {
            header: FfxApiHeader::new(FFX_API_QUERY_DESC_TYPE_UPSCALE_GETJITTERPHASECOUNT),
            render_width: render_extent.0,
            display_width: display_extent.0,
            out_phase_count: &mut phase_count,
        };
        let rc = unsafe { (loader.query)(&mut context, &mut phase_query.header) };
        if rc != FFX_API_RETURN_OK || phase_count <= 0 {
            unsafe { (loader.destroy_context)(&mut context, std::ptr::null()) };
            return Err(format!(
                "ffxQueryDescUpscaleGetJitterPhaseCount failed (0x{rc:x})"
            ));
        }

        let natives = match unsafe {
            FsrNatives::create(device, &raw_device, render_extent, display_extent, format)
        } {
            Ok(natives) => natives,
            Err(error) => {
                unsafe { (loader.destroy_context)(&mut context, std::ptr::null()) };
                return Err(error);
            }
        };
        let mv_pass = FsrMotionVectorPass::new(device);
        let composite = FsrComposite::new(device, format);

        Ok(FsrUpscaler {
            loader,
            queue: raw_queue,
            allocator,
            list,
            context,
            format,
            render_extent,
            display_extent,
            natives,
            mv_pass,
            composite,
            frame_index: 0,
            phase_count,
            current_jitter: (0.0, 0.0),
            prev_view_proj: None,
            reset_history: true,
            last_instant: std::time::Instant::now(),
        })
    }

    pub fn render_extent(&self) -> (u32, u32) {
        self.render_extent
    }

    pub fn display_extent(&self) -> (u32, u32) {
        self.display_extent
    }

    pub fn next_jitter(&mut self) -> (f32, f32) {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut query = FfxQueryDescUpscaleGetJitterOffset {
            header: FfxApiHeader::new(FFX_API_QUERY_DESC_TYPE_UPSCALE_GETJITTEROFFSET),
            index: (self.frame_index % self.phase_count.max(1) as u64) as i32,
            phase_count: self.phase_count,
            out_x: &mut x,
            out_y: &mut y,
        };
        let rc = unsafe { (self.loader.query)(&mut self.context, &mut query.header) };
        if rc != FFX_API_RETURN_OK {
            eprintln!("[FSR] GetJitterOffset failed (0x{rc:x}); using zero jitter");
            x = 0.0;
            y = 0.0;
        }
        self.current_jitter = (x, y);
        (x, y)
    }

    /// Clip-space translation matching `current_jitter`, in NDC units.
    pub fn jittered_matrix(&self, matrix: Mat4) -> Mat4 {
        let (jx, jy) = self.current_jitter;
        let (w, h) = self.render_extent;
        let mut m = matrix;
        m.col_mut(3).x += 2.0 * jx / w.max(1) as f32;
        m.col_mut(3).y += 2.0 * jy / h.max(1) as f32;
        m
    }
}

impl FsrUpscaler {
    /// On camera cuts, teleports and FSR re-enables the upscaler history is
    /// meaningless; the next dispatch runs with `reset = true`.
    pub fn reset_history(&mut self) {
        self.reset_history = true;
        self.prev_view_proj = None;
    }

    /// Copies the world color/depth into the native targets, runs the MV pass,
    /// then dispatches FSR on its own native command list.
    pub fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color: &wgpu::Texture,
        depth: &wgpu::Texture,
        unjittered_vp: &Mat4,
    ) -> Result<(), String> {
        let now = std::time::Instant::now();
        let frame_time_ms = (now - self.last_instant).as_secs_f32() * 1000.0;
        self.last_instant = now;

        let (rw, rh) = self.render_extent;
        let size = wgpu::Extent3d {
            width: rw.max(1),
            height: rh.max(1),
            depth_or_array_layers: 1,
        };

        // --- wgpu side: copy world color into the committed texture the
        // native list will read (bit-exact; the target is COPY_DST) and
        // gather motion vectors + a raw R32_FLOAT copy of the reversed-Z
        // depth in one fullscreen pass.
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: &self.natives.color.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            size,
        );
        // --- MV pass: fullscreen depth reprojection.
        let inv_vp = unjittered_vp.inverse();
        let prev_vp = self.prev_view_proj.unwrap_or(*unjittered_vp);
        let params = FsrMvParams {
            inv_vp: inv_vp.to_cols_array_2d(),
            prev_vp: prev_vp.to_cols_array_2d(),
            render_size: [rw as f32, rh as f32],
            prev_valid: u32::from(self.prev_view_proj.is_some()),
            pad: 0,
        };
        queue.write_buffer(&self.mv_pass.params, 0, bytemuck::bytes_of(&params));
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor {
            aspect: wgpu::TextureAspect::Depth,
            ..Default::default()
        });
        let mv_view = self
            .natives
            .mv
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_out_view = self
            .natives
            .depth
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fsr motion vector bind group"),
            layout: &self.mv_pass.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        self.mv_pass.params.as_entire_binding(),
                    ),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fsr motion vector pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &mv_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &depth_out_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.mv_pass.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));

        // --- Native FFX step. The command list runs on the same direct queue
        // wgpu uses, after the submit above, and restores every state it
        // moves so wgpu's tracker (and the next frame's copies) see the
        // states they expect.
        self.allocator.reset();
        let hr = self
            .list
            .reset(&self.allocator, d3d12::PipelineState::null());
        if hr < 0 {
            return Err(format!(
                "ID3D12GraphicsCommandList::Reset failed (HRESULT 0x{hr:08x})"
            ));
        }
        let subresource = d3d12_ty::D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        let none = d3d12_ty::D3D12_RESOURCE_BARRIER_FLAG_NONE;
        let barrier = |resource: &d3d12::Resource, before: u32, after: u32| {
            d3d12::ResourceBarrier::transition(resource.clone(), subresource, before, after, none)
        };
        let barriers = [
            barrier(
                &self.natives.color.resource,
                D3D12_STATE_COPY_DEST,
                D3D12_STATE_PIXEL_COMPUTE_READ,
            ),
            barrier(
                &self.natives.depth.resource,
                D3D12_STATE_COPY_DEST,
                D3D12_STATE_PIXEL_COMPUTE_READ,
            ),
            barrier(
                &self.natives.mv.resource,
                D3D12_STATE_RENDER_TARGET,
                D3D12_STATE_PIXEL_COMPUTE_READ,
            ),
            barrier(
                &self.natives.output.resource,
                D3D12_STATE_COMMON,
                D3D12_STATE_UNORDERED_ACCESS,
            ),
        ];
        self.list.resource_barrier(&barriers);

        let mut dispatch = FfxDispatchDescUpscale {
            header: FfxApiHeader::new(FFX_API_DISPATCH_DESC_TYPE_UPSCALE),
            command_list: self.list.as_mut_ptr() as *mut c_void,
            color: FfxApiResource::texture2d(
                &self.natives.color.resource,
                ffx_color_format(self.format),
                rw,
                rh,
                FFX_API_RESOURCE_USAGE_READ_ONLY,
                FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ,
            ),
            depth: FfxApiResource::texture2d(
                &self.natives.depth.resource,
                FFX_API_SURFACE_FORMAT_R32_FLOAT,
                rw,
                rh,
                FFX_API_RESOURCE_USAGE_DEPTHTARGET,
                FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ,
            ),
            motion_vectors: FfxApiResource::texture2d(
                &self.natives.mv.resource,
                FFX_API_SURFACE_FORMAT_R16G16_FLOAT,
                rw,
                rh,
                FFX_API_RESOURCE_USAGE_READ_ONLY,
                FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ,
            ),
            exposure: FfxApiResource::EMPTY,
            reactive: FfxApiResource::EMPTY,
            transparency_and_composition: FfxApiResource::EMPTY,
            output: FfxApiResource::texture2d(
                &self.natives.output.resource,
                ffx_color_format(self.format),
                self.display_extent.0,
                self.display_extent.1,
                FFX_API_RESOURCE_USAGE_UAV,
                FFX_API_RESOURCE_STATE_UNORDERED_ACCESS,
            ),
            jitter_offset: FfxApiFloatCoords2D {
                x: self.current_jitter.0,
                y: self.current_jitter.1,
            },
            motion_vector_scale: FfxApiFloatCoords2D { x: 1.0, y: 1.0 },
            render_size: FfxApiDimensions2D {
                width: rw,
                height: rh,
            },
            upscale_size: FfxApiDimensions2D {
                width: self.display_extent.0,
                height: self.display_extent.1,
            },
            enable_sharpening: false,
            sharpness: 0.0,
            frame_time_delta: frame_time_ms.clamp(0.1, 1000.0),
            pre_exposure: 1.0,
            reset: self.reset_history,
            camera_near: 1.0,
            camera_far: 16000.0,
            camera_fov_angle_vertical: 70f32.to_radians(),
            view_space_to_meters_factor: 1.0,
            flags: FFX_UPSCALE_FLAG_NON_LINEAR_COLOR_SRGB,
        };
        let rc = unsafe { (self.loader.dispatch)(&mut self.context, &dispatch.header) };
        let restore = [
            barrier(
                &self.natives.color.resource,
                D3D12_STATE_PIXEL_COMPUTE_READ,
                D3D12_STATE_COPY_DEST,
            ),
            barrier(
                &self.natives.depth.resource,
                D3D12_STATE_PIXEL_COMPUTE_READ,
                D3D12_STATE_COPY_DEST,
            ),
            barrier(
                &self.natives.mv.resource,
                D3D12_STATE_PIXEL_COMPUTE_READ,
                D3D12_STATE_RENDER_TARGET,
            ),
            barrier(
                &self.natives.output.resource,
                D3D12_STATE_UNORDERED_ACCESS,
                D3D12_STATE_COMMON,
            ),
        ];
        self.list.resource_barrier(&restore);
        let hr = self.list.close();
        if hr < 0 {
            // The list never ran; move everything back anyway so the next
            // frame starts from the tracked states. Dropping this list's
            // barriers is fine because it never executed.
            return Err(format!(
                "ID3D12GraphicsCommandList::Close failed (HRESULT 0x{hr:08x})"
            ));
        }
        self.queue.execute_command_lists(&[self.list.as_list()]);

        if rc != FFX_API_RETURN_OK {
            return Err(format!("ffxDispatch upscale failed (0x{rc:x})"));
        }

        self.reset_history = false;
        self.prev_view_proj = Some(*unjittered_vp);
        self.frame_index += 1;
        Ok(())
    }

    /// Composites the upscaled frame into the composition view.
    pub fn composite(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
    ) {
        self.composite
            .draw(device, queue, &self.natives.output.texture, target);
    }
}
