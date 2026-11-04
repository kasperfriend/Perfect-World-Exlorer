//! Production authored-mip uploads: compare native BC (when available) and the
//! decoded fallback across all levels, including 2x2/1x1 and BC1 transparency.
#![allow(dead_code)]
#[path = "../model_dds.rs"]
mod model_dds;
#[path = "../model_texture_gpu.rs"]
mod model_texture_gpu;
fn fixture(tag: &[u8; 4], transparent: bool) -> Vec<u8> {
    let mut bytes = vec![0; 128];
    bytes[..4].copy_from_slice(b"DDS ");
    for (o, v) in [
        (4, 124u32),
        (8, 0xa1007),
        (12, 8),
        (16, 8),
        (28, 4),
        (76, 32),
        (80, 4),
        (108, 0x401008),
    ] {
        bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes[84..88].copy_from_slice(tag);
    for (m, (color, alpha)) in [
        (0xf800u16, 17u8),
        (0x07e0, 85),
        (0x001f, 170),
        (0xffff, 255),
    ]
    .into_iter()
    .enumerate()
    {
        let blocks = if m == 0 { 4 } else { 1 };
        for _ in 0..blocks {
            if tag == b"DXT3" {
                bytes.extend([alpha; 8]);
            }
            if tag == b"DXT5" {
                bytes.extend([alpha, 0, 0, 0, 0, 0, 0, 0]);
            }
            if transparent {
                bytes.extend([0, 0, 0, 248, 255, 255, 255, 255]);
            } else {
                bytes.extend(color.to_le_bytes());
                bytes.extend([0; 6]);
            }
        }
    }
    bytes
}
fn main() {
    std::panic::set_hook(Box::new(|p| {
        eprintln!("::error::{}", p.to_string().replace('\n', "%0A"))
    }));
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:None,source:wgpu::ShaderSource::Wgsl(r#"
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var<storage,read_write> result:array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {result[id.x]=textureLoad(image,vec2<i32>(0),i32(id.x));}
"#.into())});
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: "main",
    });
    let mut comparisons = 0;
    for (tag, transparent) in [
        (b"DXT1", false),
        (b"DXT3", false),
        (b"DXT5", false),
        (b"DXT1", true),
    ] {
        let bytes = fixture(tag, transparent);
        for mode in [
            model_texture_gpu::UploadMode::Auto,
            model_texture_gpu::UploadMode::Decoded,
        ] {
            let texture = model_texture_gpu::upload(&device, &queue, &bytes, mode).unwrap();
            assert_eq!(texture.mip_level_count(), 4);
            let view = texture.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: output.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(4, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &read, 0, 64);
            queue.submit(Some(encoder.finish()));
            let slice = read.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::Maintain::Wait);
            rx.recv().unwrap().unwrap();
            let mapped = slice.get_mapped_range();
            let values: &[f32] = bytemuck::cast_slice(&mapped);
            for (m, color) in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [1.; 3]]
                .into_iter()
                .enumerate()
            {
                let alpha = if transparent {
                    0.
                } else if tag == b"DXT1" {
                    1.
                } else {
                    [17., 85., 170., 255.][m] / 255.
                };
                let rgb = if transparent { [0.; 3] } else { color };
                for (i, want) in [rgb[0], rgb[1], rgb[2], alpha].into_iter().enumerate() {
                    let got = values[m * 4 + i];
                    assert!(
                        got.is_finite() && (got - want).abs() < 0.00001,
                        "{tag:?}, mip {m}, channel {i}: {got} != {want}"
                    );
                }
                comparisons += 1;
            }
            drop(mapped);
            read.unmap();
        }
    }
    let mut tga = vec![0; 18];
    tga[2] = 2;
    tga[12..14].copy_from_slice(&1u16.to_le_bytes());
    tga[14..16].copy_from_slice(&1u16.to_le_bytes());
    tga[16] = 32;
    tga[17] = 8;
    tga.extend_from_slice(&[3, 2, 1, 4]);
    let (texture, alpha) = model_texture_gpu::upload_with_alpha(
        &device,
        &queue,
        &tga,
        model_texture_gpu::UploadMode::Auto,
    )
    .unwrap();
    assert!(alpha);
    assert_eq!((texture.width(), texture.height()), (1, 1));
    assert_eq!(texture.mip_level_count(), 1);
    assert_eq!(texture.format(), wgpu::TextureFormat::Rgba8Unorm);
    println!(
        "{comparisons} model texture mip RGBA checks plus TGA upload passed; native BC enabled: {}",
        !features.is_empty()
    );
}
