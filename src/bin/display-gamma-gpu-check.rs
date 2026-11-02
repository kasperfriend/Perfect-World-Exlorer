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
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut gamma = display_gamma::DisplayGamma::new(&device, format, 2, 2);
        // Exercise recreation of both raw and render-target views, too.
        gamma.resize(&device, 1, 1);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gamma readback target"),
            size: wgpu::Extent3d {
                width: 1,
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
            size: 256,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for setting in [0., 41., 41.9, 100., f32::NAN] {
            for rgb in [[0u8, 1, 255], [10, 11, 12], [64, 128, 192], [254, 255, 0]] {
                let raw = gamma.raw_view();
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("encoded world and overlay fixture"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &raw,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: rgb[0] as f64 / 255.,
                                    g: rgb[1] as f64 / 255.,
                                    b: rgb[2] as f64 / 255.,
                                    a: 0.25,
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
                            bytes_per_row: Some(256),
                            rows_per_image: Some(1),
                        },
                    },
                    wgpu::Extent3d {
                        width: 1,
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
                for c in 0..3 {
                    let expected =
                        (gamma_curve::sample(rgb[c] as f32 / 255., setting) * 255.).round() as i32;
                    assert!(
                        (bytes[c] as i32 - expected).abs() <= 1,
                        "{format:?} setting {setting}: {rgb:?} channel {c}: {} != {expected}",
                        bytes[c]
                    );
                }
                assert_eq!(bytes[3], 255);
                drop(bytes);
                read.unmap();
                cases += 1;
            }
        }
    }
    println!("Whole-frame gamma: {cases} RGB readbacks passed (UNORM and sRGB, resized targets)");
}
