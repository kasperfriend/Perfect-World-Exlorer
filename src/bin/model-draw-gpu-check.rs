//! Retained asset -> texture retry/cache -> material -> raster/update regression.
//! Synthetic asset, not a claim of world action scheduling or client screenshots.
#![allow(dead_code)]
#[path = "../animation.rs"]
mod animation;
#[path = "../ecmodel.rs"]
mod ecmodel;
#[path = "../model_asset.rs"]
mod model_asset;
#[path = "../model_dds.rs"]
mod model_dds;
#[path = "../model_draw.rs"]
mod model_draw;
#[path = "../../tools/fixtures/model_draw.rs"]
mod model_draw_fixture;
#[path = "../model_draw_gpu.rs"]
mod model_draw_gpu;
#[path = "../model_light.rs"]
mod model_light;
#[path = "../model_material.rs"]
mod model_material;
#[path = "../model_receiver_gpu.rs"]
mod model_receiver_gpu;
#[path = "../model_texture_gpu.rs"]
mod model_texture_gpu;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../skeleton_pose.rs"]
mod skeleton_pose;
#[path = "../skin.rs"]
mod skin;
#[path = "../skin_pose.rs"]
mod skin_pose;
fn white_dds() -> Vec<u8> {
    let mut b = vec![0; 128];
    b[..4].copy_from_slice(b"DDS ");
    for (o, v) in [
        (4, 124u32),
        (8, 0xa1007),
        (12, 4),
        (16, 4),
        (28, 1),
        (76, 32),
        (80, 4),
        (108, 0x1000),
    ] {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    b[84..88].copy_from_slice(b"DXT1");
    b.extend([255, 255, 0, 0, 0, 0, 0, 0]);
    b
}
fn poses(asset: &model_asset::ModelAsset) -> Vec<model_asset::PosedMesh> {
    asset.skins[0]
        .data
        .meshes
        .iter()
        .enumerate()
        .map(|(mesh, m)| model_asset::PosedMesh {
            skin: 0,
            mesh,
            vertices: m
                .vertices
                .iter()
                .map(|v| skin_pose::PosedVertex {
                    position: v.position,
                    normal: v.normal,
                })
                .collect(),
        })
        .collect()
}
fn check(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    draw: &model_draw_gpu::Draw,
    expected: [[u8; 4]; 2],
) {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = device.create_command_encoder(&Default::default());
    {
        let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        draw.draw(&mut p);
    }
    e.copy_texture_to_buffer(
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
    queue.submit(Some(e.finish()));
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range();
    for i in 0..2 {
        for c in 0..4 {
            assert!(
                (bytes[i * 4 + c] as i32 - expected[i][c] as i32).abs() <= 1,
                "pixel {i} channel {c}: {} != {}",
                bytes[i * 4 + c],
                expected[i][c]
            );
        }
    }
}
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
    let (mut asset, _) = model_draw_fixture::fixture();
    let mut material = asset.skins[0].data.materials[0].clone();
    material.ambient = [0.25, 0.5, 0.75, 0.];
    material.diffuse = [0., 0., 0., 1.];
    asset.skins[0].data.materials[0] = material.clone();
    material.ambient = [0.75, 0.25, 0.5, 0.];
    asset.skins[0].data.materials.push(material);
    let source = asset.skins[0].data.meshes[0].clone();
    asset.skins[0].data.meshes.clear();
    for i in 0..2 {
        let mut m = source.clone();
        m.material = i;
        m.indices = vec![0, 1, 2, 2, 1, 3];
        let left = i as f32 - 1.;
        let right = i as f32;
        m.vertices = [
            [left, -1., 0.5],
            [right, -1., 0.5],
            [left, 1., 0.5],
            [right, 1., 0.5],
        ]
        .into_iter()
        .map(|position| skin::Vertex {
            position,
            normal: [0., 0., 1.],
            uv: [0.5; 2],
            weights: [1., 0., 0., 0.],
            joints: [0; 4],
        })
        .collect();
        asset.skins[0].data.meshes.push(m);
    }
    let frame = model_draw_gpu::Frame {
        matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
        lighting: model_material::Lighting {
            environment_ambient: [1.; 4],
            additional_ambient: [0.; 4],
            directional_diffuse: [0., 0., 0., 1.],
            point_ambient: [0.; 4],
            point_diffuse: [0.; 4],
            transparency: 0.,
            point_enabled: false,
        },
        direction: [0., 0., -1.],
        point_position: [0., 0., 10.],
        attenuation: [0., 0.4, 0.],
        eye_direction: [0., 0., -1.],
        directional_specular: [1.; 4],
        specular_variant: true,
        vertex_fog: 1.,
        fog_color: [0.; 4],
        fog_range: [0.; 4],
        clip_plane: [0.; 4],
        reverse_front: false,
        alpha_compare: 0,
        alpha_reference: 84. / 255.,
    };
    let sampler = device.create_sampler(&Default::default());
    let mut reads = 0;
    let draw = model_draw_gpu::Draw::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        None,
        false,
        &sampler,
        &asset,
        &poses(&asset),
        frame,
        |path| {
            reads += 1;
            Ok(if path.contains("tex_a/") {
                vec![0; 128]
            } else {
                white_dds()
            })
        },
    )
    .unwrap();
    assert_eq!(reads, 2);
    assert_eq!(draw.mesh_count(), 2);
    assert_eq!(draw.texture_count(), 1);
    check(
        &device,
        &queue,
        &draw,
        [[64, 128, 191, 255], [191, 64, 128, 255]],
    );
    let mut dim = frame;
    dim.lighting.environment_ambient = [0.5; 4];
    draw.update(&queue, &asset, &poses(&asset), dim).unwrap();
    let expected = [[32, 64, 96, 255], [96, 32, 64, 255]];
    check(&device, &queue, &draw, expected);
    let mut bad = poses(&asset);
    bad[1].vertices[0].normal[0] = f32::NAN;
    assert!(draw.update(&queue, &asset, &bad, frame).is_err());
    check(&device, &queue, &draw, expected);
    let mut moved = poses(&asset);
    for v in &mut moved[0].vertices {
        v.position[0] -= 4.;
    }
    draw.update(&queue, &asset, &moved, dim).unwrap();
    check(&device, &queue, &draw, [[0; 4], expected[1]]);
    println!("::notice::retained model draw passed: two material meshes, shared authored DDS with decode-failure retry, frame update, rejected-update atomicity, and posed geometry movement");
}
