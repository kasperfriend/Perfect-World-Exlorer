//! GPU check of the authored alternate-area material and encoded blending.
#![allow(dead_code)]
mod gpu {
    use bytemuck::{Pod, Zeroable};
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct Vertex {
        pub pos: [f32; 3],
        pub normal: [f32; 3],
        pub uv: [f32; 2],
        pub color: [f32; 4],
        pub kind: f32,
        pub diffuse: u32,
        pub night_diffuse: u32,
    }
    pub fn depth(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }
}
#[path = "../simple_water_gpu.rs"]
mod simple_water_gpu;
#[path = "../water_gpu.rs"]
mod water_gpu;
use wgpu::util::DeviceExt;
fn main() {
    let inst = wgpu::Instance::default();
    let adapter = pollster::block_on(inst.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut frames = Vec::new();
        for n in 0..30 {
            frames.push(vec![image::RgbaImage::from_pixel(
                4,
                4,
                image::Rgba([if n == 1 { 160 } else { 64 }, 128, 192, 64]),
            )]);
        }
        let simple = simple_water_gpu::SimpleWaterGpu::new(&device, &queue, format, &frames);
        let v = |x, y| gpu::Vertex {
            pos: [x, y, 0.5],
            normal: [0., 1., 0.],
            uv: [0., 0.],
            color: [1.; 4],
            kind: 1.,
            diffuse: 0,
            night_diffuse: 0,
        };
        let vertices = [v(-1., -1.), v(1., -1.), v(-1., 1.), v(1., 1.)];
        let indices = [0u32, 1, 2, 1, 3, 2];
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let raw = format.remove_srgb_suffix();
        let alternate = [raw];
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: if format.is_srgb() { &alternate } else { &[] },
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(raw),
            ..Default::default()
        });
        let depth = gpu::depth(&device, 64, 64);
        let dv = depth.create_view(&Default::default());
        for (seconds, frame, distance, fog_on) in [
            (0., 0u8, 1., false),
            (0.034, 1, 1., false),
            (0., 0, 1., true),
            (0., 0, 4., true),
            (0., 0, 8., true),
            (0., 0, 8., false),
        ] {
            simple.update(
                &queue,
                glam::Mat4::IDENTITY * distance,
                seconds,
                [2., 6., if fog_on { 1. } else { 0. }, 0.],
                [0.2, 0.4, 0.6, 1.],
            );
            let mut e = device.create_command_encoder(&Default::default());
            {
                let _p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 50. / 255.,
                                g: 100. / 255.,
                                b: 150. / 255.,
                                a: 1.,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &dv,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            simple.draw(&mut e, &view, &dv, &vb, &ib, 0, 6);
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 64 * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            e.copy_texture_to_buffer(
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
                        rows_per_image: Some(64),
                    },
                },
                wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(Some(e.finish()));
            let sl = read.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            sl.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::Maintain::Wait);
            rx.recv().unwrap().unwrap();
            let bytes = sl.get_mapped_range();
            let at = (32 * 64 + 32) * 4;
            let alpha = 224f32 / 255.;
            for k in 0..3 {
                let base = [50., 100., 150.][k];
                let t = [if frame == 1 { 160. } else { 64. }, 128., 192.][k];
                let fact = [37., 79., 85.][k];
                let color = fact * (1. - 64. / 255.) + t * (64. / 255.);
                let f = if fog_on {
                    ((distance - 2.) / 4f32).clamp(0., 1.)
                } else {
                    0.
                };
                let fogged = color * (1. - f) + [51., 102., 153.][k] * f;
                let expect = (fogged * alpha + base * (1. - alpha)).round() as i32;
                assert!(
                    (bytes[at + k] as i32 - expect).abs() <= 1,
                    "{format:?} frame {frame} channel {k}: {} != {expect}",
                    bytes[at + k]
                );
            }
            println!(
                "{format:?} alternate frame {frame} pixel {:?}",
                &bytes[at..at + 4]
            );
            drop(bytes);
            read.unmap();
        }
    }
}
