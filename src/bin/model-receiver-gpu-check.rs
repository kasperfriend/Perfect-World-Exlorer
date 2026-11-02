//! Raster regression through production posed-mesh/material/light/texture,
//! encoded fog and SRCALPHA blending, not merely a compute lighting helper.
#![allow(dead_code)]
#[path = "../model_light.rs"]
mod model_light;
#[path = "../model_material.rs"]
mod model_material;
#[path = "../model_receiver_gpu.rs"]
mod model_receiver_gpu;
use model_receiver_gpu::{Mesh, Params, Receiver, Vertex};
fn main() {
    std::panic::set_hook(Box::new(|p| {
        eprintln!("::error::{}", p.to_string().replace('\n', "%0A"))
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
    assert!(Receiver::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, None, false).is_err());
    let receiver = Receiver::new(&device, wgpu::TextureFormat::Rgba8Unorm, None, false).unwrap();
    let image = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texel = [128u8, 64, 192, 128];
    queue.write_texture(
        image.as_image_copy(),
        &texel,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        image.size(),
    );
    let view = image.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: image.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let positions = [
        [-1., -1., 0.5],
        [1., -1., 0.5],
        [-1., 1., 0.5],
        [1., 1., 0.5],
    ];
    let background = [32u8, 96, 160, 192];
    let fog_color = [0.12, 0.24, 0.36, 1.];
    let mut cases = 0;
    for specular in [false, true] {
        for point in [false, true] {
            for normal in [[0., 0., 1.], [0., 0., -1.]] {
                for fog in [0., 0.4, 1.] {
                    let constants = model_material::bind(
                        model_material::Material {
                            ambient: [0.5, 0.25, 0.8, 0.2],
                            diffuse: [0.8, 0.6, 0.4, 0.7],
                            emissive: [0.02, 0.03, 0.04, 0.01],
                        },
                        model_material::Lighting {
                            environment_ambient: [0.2, 0.3, 0.4, 0.5],
                            additional_ambient: [0.01, 0.02, 0.03, 0.04],
                            directional_diffuse: [0.3, 0.4, 0.5, 0.8],
                            point_ambient: [0.1, 0.2, 0.3, 0.4],
                            point_diffuse: [0.4, 0.3, 0.2, 0.9],
                            transparency: 0.,
                            point_enabled: point,
                        },
                    );
                    let input = model_light::LightInput {
                        world_position: positions[0],
                        normal,
                        direction: [0., 0., -1.],
                        point_position: [0., 0., 10.],
                        attenuation: [0., 0.4, 0.],
                        other_ambient: constants.ambient[..3].try_into().unwrap(),
                        directional_diffuse: constants.directional_diffuse[..3].try_into().unwrap(),
                        point_diffuse: constants.point_diffuse[..3].try_into().unwrap(),
                        point_ambient: constants.point_ambient[..3].try_into().unwrap(),
                        point_enabled: point,
                    };
                    let alpha = [
                        constants.ambient[3],
                        constants.directional_diffuse[3],
                        constants.point_ambient[3],
                    ];
                    let primary = if specular {
                        model_light::specular_variant_diffuse(input, alpha)
                    } else {
                        model_light::vertex_light_rgba(input, alpha)
                    };
                    let specular_color = model_material::bind_specular(
                        [0.06, 0.08, 0.08, 1.],
                        [0.5, 0.25, 0.125, 1.],
                    );
                    let shine = if specular {
                        model_light::directional_specular(
                            input,
                            [0., 0., -1.],
                            7.,
                            [specular_color[0], specular_color[1], specular_color[2]],
                        )
                    } else {
                        [0.; 3]
                    };
                    let params = Params {
                        matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
                        ambient: constants.ambient,
                        diffuse: constants.directional_diffuse,
                        point_ambient: constants.point_ambient,
                        point_diffuse: constants.point_diffuse,
                        direction: [0., 0., -1., 0.],
                        point_position: [0., 0., 10., 0.],
                        attenuation: [0., 0.4, 0., 0.],
                        eye_direction: [0., 0., -1., 0.],
                        specular: specular_color,
                        lighting: [7., u32::from(point) as f32, u32::from(specular) as f32, 0.],
                        fog_color,
                        fog_range: [0.; 4],
                        clip_plane: [0.; 4],
                        raster: [1., 0., 0., 0.],
                    };
                    let vertices: Vec<_> = positions
                        .iter()
                        .map(|&position| Vertex {
                            position,
                            normal,
                            uv: [0.5; 2],
                            fog,
                        })
                        .collect();
                    let mesh = Mesh::new(&device, &vertices, &[0, 1, 2, 2, 1, 3]).unwrap();
                    assert!(mesh.update(&queue, &vertices[..3]).is_err());
                    let binding = receiver.binding(&device, &view, &sampler, &params).unwrap();
                    for reject in [0, 1, 2, 3, 4, 5] {
                        let mut p = params;
                        if reject == 1 {
                            p.clip_plane = [0., 0., 0., -1.];
                        }
                        if reject == 2 {
                            p.raster[2] = 1.;
                            p.raster[3] = 1.;
                        }
                        if reject == 3 {
                            p.raster[0] = 0.;
                            p.raster[1] = 1.;
                        }
                        if reject == 4 {
                            p.raster[0] = 0.;
                            p.raster[1] = 0.;
                        }
                        if reject == 5 {
                            p.fog_range = [0., 2., 1., 0.];
                        }
                        binding.update(&queue, &p).unwrap();
                        let mut encoder = device.create_command_encoder(&Default::default());
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: None,
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &target_view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color {
                                            r: background[0] as f64 / 255.,
                                            g: background[1] as f64 / 255.,
                                            b: background[2] as f64 / 255.,
                                            a: background[3] as f64 / 255.,
                                        }),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            receiver.draw(&mut pass, &mesh, &binding);
                        }
                        encoder.copy_texture_to_buffer(
                            target.as_image_copy(),
                            wgpu::ImageCopyBuffer {
                                buffer: &read,
                                layout: wgpu::ImageDataLayout {
                                    offset: 0,
                                    bytes_per_row: Some(256),
                                    rows_per_image: Some(1),
                                },
                            },
                            target.size(),
                        );
                        queue.submit(Some(encoder.finish()));
                        let slice = read.slice(..);
                        let (tx, rx) = std::sync::mpsc::channel();
                        slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
                        device.poll(wgpu::Maintain::Wait);
                        rx.recv().unwrap().unwrap();
                        let bytes = slice.get_mapped_range();
                        let opacity = (primary[3] * texel[3] as f32 / 255.).clamp(0., 1.);
                        for c in 0..4 {
                            let src = if c == 3 {
                                opacity
                            } else {
                                let encoded =
                                    (texel[c] as f32 / 255. * primary[c] + shine[c]).clamp(0., 1.);
                                let f = if reject == 5 { 0.5 } else { fog };
                                fog_color[c] * (1. - f) + encoded * f
                            };
                            let expected = if reject != 0 && reject != 4 && reject != 5 {
                                background[c]
                            } else {
                                ((src * opacity + background[c] as f32 / 255. * (1. - opacity))
                                    * 255.)
                                    .round() as u8
                            };
                            assert!(
                                (bytes[c] as i32 - expected as i32).abs() <= 1,
                                "raster case {cases} channel {c}: {} != {expected}",
                                bytes[c]
                            );
                        }
                        drop(bytes);
                        read.unmap();
                        cases += 1;
                    }
                }
            }
        }
    }
    println!("::notice::{cases} production model raster RGBA checks passed (material/light variants, encoded blend, fog, clip and alpha-test)");
}
