//! Per-view terrain submission test (P2.09).
//!
//! The 1.4.2 client draws the same terrain records into five targets, and
//! `A3DTerrain2` owns one visibility/layer snapshot per frame that every view
//! reads:
//!
//! * the main color+depth pass;
//! * the reflection render target (`0x7c1490`: the same batched PS1.4 stage
//!   setup as `0x7c14b0`, with the bind flipped to the reflection set);
//! * the clipped refraction target (`0x7cb642`/`0x7cb8ef`, shader
//!   `vs/terrain_refract_vs`: above-plane vertices are projected onto the clip
//!   plane);
//! * the minimap, drawn from the same terrain object rather than a second mesh;
//!   and
//! * the full-world map view, which submits the far LOD mesh against the same
//!   per-area layer table.
//!
//! This check drives the **production** terrain shader source out of
//! `src/terrain_gpu.rs` together with its real `vs`, `vs_far` and `fs`
//! entry points, and its real area/layer storage layout. Every view is
//! submitted separately against one shared snapshot, and every view must return
//! the same terrain colour for the same world position.
//!
//! The shader is what decides *what* a view draws, and this file only supplies
//! geometry: the client has no per-view material. So the check also asserts the
//! sequencing rule the viewer must keep — one snapshot, five submissions, no
//! per-view rebuild — by re-reading the storage after every pass.
use wgpu::util::DeviceExt;

const GRID: f32 = 8.;
const SPACING: f32 = 4.;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Area {
    start: u32,
    count: u32,
    _pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Layer {
    tex: u32,
    axis: u32,
    mask: u32,
    _pad: u32,
    uv: [f32; 2],
    _pad2: [f32; 2],
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    read: wgpu::Buffer,
    size: u32,
}

fn main() {
    // Panics exit 101 and their message only reaches the job log, which is not
    // always retrievable; mirror it into a GitHub annotation so a CI failure
    // stays diagnosable.
    std::panic::set_hook(Box::new(|info| {
        println!("::error::terrain-view-gpu-check panicked: {info}");
    }));
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();

    // Production WGSL. This is the exact string the viewer hands to wgpu.
    let source = include_str!("../terrain_gpu.rs")
        .split("const SHADER: &str = r#\"")
        .nth(1)
        .unwrap()
        .split("\"#;")
        .next()
        .unwrap();
    let terrain = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production terrain shaders"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    // Bindings 0..=10, matching the production layout one-for-one.
    let binding = |b: u32| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: match b {
            6 | 7 => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            3 | 5 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            _ => wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
        },
        count: None,
    };
    let uniform_binding = |b: u32| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            uniform_binding(0),
            uniform_binding(1),
            binding(2),
            binding(3),
            binding(4),
            binding(5),
            binding(6),
            binding(7),
            binding(8),
            binding(9),
            binding(10),
        ],
    });

    // Camera is 388 f32; Settings is 12 words.
    let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("camera"),
        size: 388 * 4,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let settings_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("settings"),
        size: 12 * 4,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let storage_usage =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
    let areas_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("per-frame area snapshot"),
        contents: bytemuck::cast_slice(&area_records()),
        usage: storage_usage,
    });
    let layers_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("per-frame layer snapshot"),
        contents: bytemuck::cast_slice(&layer_records()),
        usage: storage_usage,
    });

    // One color layer and one masked layer from the same array.
    let color_array = array(&device, &queue, 64, 1);
    let color_view = color_array.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    eprintln!("terrain-view: uploading mask page");
    // A 512px mask page holding the two 256px slots this check uses. The shader
    // divides by the MASK_PAGE constant (4096) in `fs`, so only slot 0 is
    // addressed here; the point of the check is that all views read the same
    // slot, not the atlas geometry.
    let mask_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain mask page"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // One 256px slot at weight 220, so the composite is deterministic.
    let mut mask_px = vec![0u8; 512 * 512 * 4];
    for px in mask_px.chunks_mut(4) {
        px.copy_from_slice(&[220, 220, 220, 255]);
    }
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &mask_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &mask_px,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(512 * 4),
            rows_per_image: Some(512),
        },
        wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
    );
    let mask_view = mask_tex.create_view(&Default::default());

    // Native sampler policy: MAG/MIN/MIP = 2 = D3DTEXF_LINEAR and ADDRESSU/V = 2
    // = D3DTADDRESS_MIRROR (0x7c14b0), with no D3DTSS_MAXANISOTROPY write.
    let layer_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::MirrorRepeat,
        address_mode_v: wgpu::AddressMode::MirrorRepeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        anisotropy_clamp: 1,
        ..Default::default()
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: settings_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&layer_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&layer_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: areas_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: layers_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
        ],
    });

    // The production terrain vertex layouts, identical to the viewer's.
    const NEAR: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4,4=>Float32];
    const FAR: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x4];
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = |entry: &'static str, far: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &terrain,
                entry_point: entry,
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: if far { 16 } else { 60 },
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: if far { &FAR } else { &NEAR },
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &terrain,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        })
    };
    let near_pipe = pipeline("vs", false);
    let far_pipe = pipeline("vs_far", true);
    let near = near_vertex(&device);
    let far = far_vertex(&device);

    // Five production views. `clip_y < 0` selects the refraction projection,
    // `reflected` the mirrored light set, `far` the distant LOD mesh.
    let views = [
        ("main", 1.0f32, false, false),
        ("reflection", 1.0, true, false),
        ("clipped-refraction", -1.0, false, false),
        ("minimap", 1.0, false, true),
        ("full-world-map", 1.0, false, true),
    ];

    // The snapshot both buffers hold before anything is submitted.
    let area_snapshot = bytemuck::cast_slice::<Area, u8>(&area_records()).to_vec();
    let layer_snapshot = bytemuck::cast_slice::<Layer, u8>(&layer_records()).to_vec();

    let mut sampled = Vec::new();
    for (name, clip_y, reflected, far_mesh) in views {
        // One per-frame snapshot is reused by every view; nothing is rebuilt.
        assert_eq!(
            read_back(&device, &queue, &areas_buf),
            area_snapshot,
            "{name}"
        );
        assert_eq!(
            read_back(&device, &queue, &layers_buf),
            layer_snapshot,
            "{name}"
        );
        eprintln!("terrain-view: submitting {name}");
        write_camera(&queue, &camera_buf, &settings_buf, clip_y, reflected);
        let target = new_target(&device, name);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(name),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
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
            p.set_bind_group(0, &bind, &[]);
            if far_mesh {
                p.set_pipeline(&far_pipe);
                p.set_vertex_buffer(0, far.slice(..));
                p.draw(0..3, 0..1);
            } else {
                p.set_pipeline(&near_pipe);
                p.set_vertex_buffer(0, near.slice(..));
                p.draw(0..3, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &target.read,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(target.size * 4),
                    rows_per_image: Some(target.size),
                },
            },
            wgpu::Extent3d {
                width: target.size,
                height: target.size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let px = read_target(&device, &target);
        let center =
            ((target.size as usize / 2) * target.size as usize + target.size as usize / 2) * 4;
        let rgb = [px[center], px[center + 1], px[center + 2]];
        println!("{name}: terrain RGB={rgb:?}");
        sampled.push((name, rgb));
    }

    // Every view stays on the same snapshot after all five submissions.
    assert_eq!(read_back(&device, &queue, &areas_buf), area_snapshot);
    assert_eq!(read_back(&device, &queue, &layers_buf), layer_snapshot);

    let main = sampled[0].1;
    if main == [0, 0, 0] {
        println!("::error::terrain-view-gpu-check: main view rendered black; sampled={sampled:?}");
        std::process::exit(1);
    }
    for (name, rgb) in &sampled {
        let delta = (0..3)
            .map(|i| (rgb[i] as i32 - main[i] as i32).abs())
            .max()
            .unwrap_or(0);
        // The unlit branch is a pure function of the shared layer snapshot, so
        // the only permitted difference is sampling/interpolation rounding.
        if *rgb == [0, 0, 0] || delta > 3 {
            println!(
                "::error::terrain-view-gpu-check: {name} rgb={rgb:?} vs main={main:?} \
                 delta={delta}; all={sampled:?}"
            );
            std::process::exit(1);
        }
    }
    println!(
        "five views shaded one per-frame snapshot: {} area / {} layers",
        area_records().len(),
        layer_records().len()
    );
}

fn array(device: &wgpu::Device, queue: &wgpu::Queue, side: u32, layers: u32) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain layer array"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers,
        },
        mip_level_count: side.ilog2() + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut level = vec![0u8; (side * side * 4) as usize];
    for (i, px) in level.chunks_mut(4).enumerate() {
        // A gradient, so a shifted sample cannot alias to the same value.
        px.copy_from_slice(&[
            (i % side as usize) as u8 * 4,
            (i / side as usize) as u8 * 4,
            96,
            255,
        ]);
    }
    let mut s = side;
    for mip in 0..=side.ilog2() {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(s * 4),
                rows_per_image: Some(s),
            },
            wgpu::Extent3d {
                width: s,
                height: s,
                depth_or_array_layers: 1,
            },
        );
        if s == 1 {
            break;
        }
        let next = s / 2;
        let mut reduced = vec![0u8; (next * next * 4) as usize];
        for y in 0..next as usize {
            for x in 0..next as usize {
                for k in 0..4 {
                    let a = level[((2 * y) * s as usize + 2 * x) * 4 + k] as u32;
                    let b = level[((2 * y) * s as usize + 2 * x + 1) * 4 + k] as u32;
                    let c = level[((2 * y + 1) * s as usize + 2 * x) * 4 + k] as u32;
                    let d = level[((2 * y + 1) * s as usize + 2 * x + 1) * 4 + k] as u32;
                    reduced[(y * next as usize + x) * 4 + k] = ((a + b + c + d + 2) / 4) as u8;
                }
            }
        }
        level = reduced;
        s = next;
    }
    texture
}

fn area_records() -> Vec<Area> {
    vec![Area {
        start: 0,
        count: 2,
        _pad: [0; 2],
    }]
}

fn layer_records() -> Vec<Layer> {
    vec![
        Layer {
            tex: 0,
            axis: 1,
            mask: u32::MAX,
            _pad: 0,
            uv: [1., 1.],
            _pad2: [0.; 2],
        },
        Layer {
            tex: 0,
            axis: 1,
            mask: 0,
            _pad: 0,
            uv: [1., 1.],
            _pad2: [0.; 2],
        },
    ]
}

/// World position the check samples: the origin cell of an 8x8 grid, which also
/// contains the authored infinite hole used in the viewer's unit tests, so the
/// `discard` path is covered by the same storage.
const EYE: [f32; 3] = [0., 1., 0.];

/// Write `Camera` (binding 0) and `Settings` (binding 1).
fn write_camera(
    queue: &wgpu::Queue,
    camera: &wgpu::Buffer,
    settings: &wgpu::Buffer,
    clip_y: f32,
    reflected: bool,
) {
    let mut bytes = [0u8; 388 * 4];
    let words: &mut [f32; 388] = bytemuck::from_bytes_mut(&mut bytes);
    // Identity view translated to the eye, so the sampled world position sits
    // at clip 0 without any projection.
    words[..16]
        .copy_from_slice(&glam::Mat4::from_translation(-glam::Vec3::from(EYE)).to_cols_array());
    // camera.clip = (plane, clip, 0, 0). clip.y >= 0 is the ordinary path; a
    // negative value selects the refraction projection of terrain_refract_vs.
    //
    // fs() begins with `if camera.clip.y > 0. && i.world.y < camera.clip.x
    // { discard; }`, so the plane is the height *below which* terrain is hidden
    // (the viewer's underwater clip). It must sit below the sampled geometry
    // (y = EYE[1]), otherwise every fragment is discarded and the view renders
    // black. The refraction view keeps the plane just under the surface so the
    // projection is small and the triangle stays on screen.
    words[20] = if clip_y < 0. {
        EYE[1] - 0.1
    } else {
        EYE[1] - 0.5
    };
    words[21] = clip_y;
    // camera.sun = (dir.xyz, night factor). A negative w selects the unlit
    // layer-composite branch, so only the shared layer snapshot matters.
    words[32] = -1.;
    words[33] = 0.;
    words[34] = 0.;
    words[35] = -1.;
    // camera.light = (ambient, diffuse, specular, 1). The reflected view uses
    // the dimmer mirrored set.
    words[36] = if reflected { 0.25 } else { 0.5 };
    words[37] = if reflected { 0.25 } else { 0.5 };
    words[38] = 1.;
    words[39] = 1.;
    // fog_range.z = 0 disables fog, so no view gains a per-view fog term.
    words[380] = 0.;
    words[381] = 0.;
    words[382] = 0.;
    words[383] = 0.;
    queue.write_buffer(camera, 0, &bytes);

    let mut sbytes = [0u8; 12 * 4];
    let s: &mut [f32; 12] = bytemuck::from_bytes_mut(&mut sbytes);
    s[0] = GRID;
    s[1] = GRID;
    s[2] = SPACING;
    s[3] = GRID;
    let areas = [1u32, 1, 2, 1, 0, 0, 0, 0];
    for (i, v) in areas.iter().enumerate() {
        sbytes[16 + i * 4..20 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    queue.write_buffer(settings, 0, &sbytes);
}

fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, source: &wgpu::Buffer) -> Vec<u8> {
    let size = source.size();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range().to_vec();
    drop(slice);
    staging.unmap();
    bytes
}

fn new_target(device: &wgpu::Device, label: &str) -> Target {
    let size = 32u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (size * size * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Target {
        texture,
        view,
        read,
        size,
    }
}

fn read_target(device: &wgpu::Device, target: &Target) -> Vec<u8> {
    let slice = target.read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range().to_vec();
    drop(slice);
    target.read.unmap();
    bytes
}

/// A screen-covering triangle in world space, inside the origin cell.
fn near_vertex(device: &wgpu::Device) -> wgpu::Buffer {
    let mut data = Vec::new();
    for (x, z) in [(-1.5f32, -1.5f32), (1.5, -1.5), (0., 1.5)] {
        let pos = [EYE[0] + x, EYE[1], EYE[2] + z];
        data.extend_from_slice(bytemuck::cast_slice(&pos));
        data.extend_from_slice(bytemuck::cast_slice(&[0f32, 1., 0.]));
        data.extend_from_slice(bytemuck::cast_slice(&[0f32, 0.]));
        data.extend_from_slice(bytemuck::cast_slice(&[1f32; 4]));
        data.extend_from_slice(bytemuck::cast_slice(&[0f32]));
    }
    assert_eq!(data.len(), 3 * 60);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &data,
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn far_vertex(device: &wgpu::Device) -> wgpu::Buffer {
    let mut data = Vec::new();
    for (x, z) in [(-1.5f32, -1.5f32), (1.5, -1.5), (0., 1.5)] {
        let pos = [EYE[0] + x, EYE[1], EYE[2] + z];
        data.extend_from_slice(bytemuck::cast_slice(&pos));
        data.extend_from_slice(bytemuck::cast_slice(&[0f32, 1., 0., 0.]));
    }
    assert_eq!(data.len(), 3 * 16);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &data,
        usage: wgpu::BufferUsages::VERTEX,
    })
}
