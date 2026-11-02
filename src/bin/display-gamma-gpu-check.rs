//! Read back the production whole-frame gamma pass through UNORM and sRGB targets.
//! Run with: cargo run --bin display-gamma-gpu-check
#![allow(dead_code)]
#[path = "../display_gamma.rs"]
mod display_gamma;
#[path = "../gamma_curve.rs"]
mod gamma_curve;

fn main() {
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
    let mut cases = 0;
    let fixture_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("whole-frame encoded gamma input ramp"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex fn vs(@builtin(vertex_index) id:u32)->@builtin(position) vec4<f32> {
 let x=f32((id<<1u)&2u);let y=f32(id&2u);
 return vec4<f32>(x*2.-1.,1.-y*2.,0.,1.);
}
@fragment fn fs(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
 let i=u32(p.x);
 return vec4<f32>(vec3<f32>(f32(i),f32(255u-i),f32((73u*i+17u)%256u))/255.,0.25);
}
"#
            .into(),
        ),
    });
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let mut gamma = display_gamma::DisplayGamma::new(&device, format, 2, 2);
        // Exercise recreation of both raw and render-target views, too.
        gamma.resize(&device, 1, 1);
        gamma.resize(&device, 256, 1);
        let fixture = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("encoded all-channel byte ramp"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &fixture_shader,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fixture_shader,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format: format.remove_srgb_suffix(),
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gamma readback target"),
            size: wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1024,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for setting in (0..=100)
            .map(|v| v as f32)
            .chain([41.9, f32::NAN, f32::INFINITY, -1., 101.])
        {
            let raw = gamma.raw_view();
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("encoded world and overlay fixture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &raw,
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
                pass.set_pipeline(&fixture);
                pass.draw(0..3, 0..1);
            }
            queue.submit(Some(encoder.finish()));
            gamma.present(&device, &queue, &target_view, setting);
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture: &target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &read,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(1024),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: 256,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(Some(encoder.finish()));
            let slice = read.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::Maintain::Wait);
            rx.recv().unwrap().unwrap();
            let bytes = slice.get_mapped_range();
            for x in 0..256usize {
                let rgb = [x as u8, (255 - x) as u8, ((73 * x + 17) % 256) as u8];
                for c in 0..3 {
                    let expected =
                        (gamma_curve::sample(rgb[c] as f32 / 255., setting) * 255.).round() as i32;
                    let memory_channel = if matches!(
                        format,
                        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                    ) {
                        2 - c
                    } else {
                        c
                    };
                    let actual = bytes[x * 4 + memory_channel];
                    assert!(
                        (actual as i32 - expected).abs() <= 1,
                        "{format:?} setting {setting}: {rgb:?} channel {c}: {actual} != {expected}"
                    );
                }
                assert_eq!(bytes[x * 4 + 3], 255);
                cases += 1;
            }
            drop(bytes);
            read.unmap();
        }
    }
    println!("Whole-frame gamma: {cases} RGB readbacks passed (all 101 settings/all 256 channel values, RGBA/BGRA UNORM/sRGB, resized targets)");
}
