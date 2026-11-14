//! Synthetic end-to-end check of the production FSR 1 EASU + RCAS path.
//! Requires a working wgpu adapter (e.g. lavapipe in Linux CI).
#![allow(dead_code)]
#[path = "../fsr1_gpu.rs"]
mod fsr1_gpu;
#[path = "../render_scale.rs"]
mod render_scale;
#[path = "../upscaling.rs"]
mod upscaling;

fn target(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("FSR 1 diagnostic present target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn clear(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, color: f64) {
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FSR 1 diagnostic input"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: color,
                        g: color,
                        b: color,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    queue.submit(Some(encoder.finish()));
}

fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let row = (w * 4 + 255) / 256 * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("FSR 1 diagnostic readback"),
        size: u64::from(row) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let mapped = slice.get_mapped_range();
    let mut result = Vec::with_capacity((w * h * 4) as usize);
    for line in mapped.chunks_exact(row as usize) {
        result.extend_from_slice(&line[..(w * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    result
}

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("FSR 1 check needs a GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut scale = render_scale::RenderScale::new(&device, format, 64, 64, 128, 128);
    let output = target(&device, 128, 128);
    let output_view = output.create_view(&Default::default());

    // EASU and RCAS must both keep a flat mid-grey neutral through sRGB
    // sampling/writes, even at the clamped corners; black must not make RCAS
    // produce NaNs, and UI/composition colors are not changed by the scaler.
    clear(&device, &queue, &scale.view(), 0.21404114);
    scale.blit(
        &device,
        &queue,
        &output_view,
        upscaling::UpscaleMethod::Fsr1,
        0.2,
    );
    let pixels = read(&device, &queue, &output, 128, 128);
    for (x, y) in [(0, 0), (64, 64), (127, 127)] {
        let i = ((y * 128 + x) * 4) as usize;
        assert!(
            pixels[i..i + 3]
                .iter()
                .all(|v| (*v as i32 - 128).abs() <= 2),
            "flat FSR 1 output at ({x},{y}): {:?}",
            &pixels[i..i + 4]
        );
    }
    clear(&device, &queue, &scale.view(), 0.0);
    scale.blit(
        &device,
        &queue,
        &output_view,
        upscaling::UpscaleMethod::Fsr1,
        0.2,
    );
    assert!(read(&device, &queue, &output, 128, 128)
        .chunks_exact(4)
        .all(|pixel| pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0));

    // A checkerboard exercises the 12-tap edge filter and RCAS, not just the
    // color-neutral flat case. It must produce pixels different from bilinear.
    let checker = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("checkerboard for FSR 1 diagnostic"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex fn vs(@builtin(vertex_index) id:u32)->@builtin(position) vec4<f32> {
  let x=f32((id<<1u)&2u); let y=f32(id&2u);
  return vec4<f32>(x*2.0-1.0,1.0-y*2.0,0.0,1.0);
}
@fragment fn fs(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
  let tile = (u32(p.x)/8u + u32(p.y)/8u) & 1u;
  let c = select(vec3<f32>(0.06,0.15,0.25),vec3<f32>(0.7,0.4,0.2),tile==1u);
  return vec4<f32>(c,1.0);
}
"#
                .into(),
        ),
    });
    let checker_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("checkerboard fixture"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &checker,
            entry_point: "vs",
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &checker,
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
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = scale.view();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FSR 1 checkerboard input"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        pass.set_pipeline(&checker_pipeline);
        pass.draw(0..3, 0..1);
    }
    queue.submit(Some(encoder.finish()));
    scale.blit(
        &device,
        &queue,
        &output_view,
        upscaling::UpscaleMethod::Fsr1,
        0.2,
    );
    let filtered = read(&device, &queue, &output, 128, 128);
    scale.blit(
        &device,
        &queue,
        &output_view,
        upscaling::UpscaleMethod::Off,
        0.2,
    );
    let bilinear = read(&device, &queue, &output, 128, 128);
    let different = filtered
        .chunks_exact(4)
        .zip(bilinear.chunks_exact(4))
        .filter(|(a, b)| a[..3] != b[..3])
        .count();
    assert!(
        different > 64,
        "FSR 1 must not silently use bilinear on a detailed input: {different} differing pixels"
    );

    // Switching back must leave the existing bilinear code path intact.
    clear(&device, &queue, &scale.view(), 1.0);
    scale.blit(
        &device,
        &queue,
        &output_view,
        upscaling::UpscaleMethod::Off,
        0.2,
    );
    let original = read(&device, &queue, &output, 128, 128);
    assert!(original
        .chunks_exact(4)
        .all(|p| p.iter().all(|v| *v == 255)));

    // A native-size FSR request must use the same bilinear fallback, not EASU
    // as a downsampler. A resize must also recreate the EASU output target.
    scale.resize(&device, 64, 64, 64, 64);
    let native = target(&device, 64, 64);
    let native_view = native.create_view(&Default::default());
    clear(&device, &queue, &scale.view(), 0.21404114);
    scale.blit(
        &device,
        &queue,
        &native_view,
        upscaling::UpscaleMethod::Fsr1,
        0.2,
    );
    let fallback = read(&device, &queue, &native, 64, 64);
    scale.blit(
        &device,
        &queue,
        &native_view,
        upscaling::UpscaleMethod::Off,
        0.2,
    );
    assert_eq!(fallback, read(&device, &queue, &native, 64, 64));

    scale.resize(&device, 32, 24, 64, 48);
    let resized = target(&device, 64, 48);
    clear(&device, &queue, &scale.view(), 0.21404114);
    scale.blit(
        &device,
        &queue,
        &resized.create_view(&Default::default()),
        upscaling::UpscaleMethod::Fsr1,
        1.0,
    );
    let data = read(&device, &queue, &resized, 64, 48);
    assert!(
        data.chunks_exact(4).all(|p| (p[0] as i32 - 128).abs() <= 2),
        "FSR 1 result after resize should preserve flat color"
    );
    println!("FSR 1: EASU+RCAS, sRGB neutrality, black, bilinear fallback and resize OK");
}
