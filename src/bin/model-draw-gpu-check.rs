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
#[path = "../model_playback.rs"]
mod model_playback;
#[path = "../model_queue.rs"]
mod model_queue;
#[path = "../model_receiver_gpu.rs"]
mod model_receiver_gpu;
#[path = "../model_texture_gpu.rs"]
mod model_texture_gpu;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
#[path = "../scene_lights.rs"]
mod scene_lights;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../skeleton_pose.rs"]
mod skeleton_pose;
#[path = "../skin.rs"]
mod skin;
#[path = "../skin_pose.rs"]
mod skin_pose;
#[path = "../world_models.rs"]
mod world_models;
#[path = "../world_models_gpu.rs"]
mod world_models_gpu;
trait TestDraw {
    fn needs_depth(&self) -> bool {
        false
    }
    fn submit<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>);
}
impl TestDraw for model_draw_gpu::Draw {
    fn submit<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        self.draw(pass);
    }
}
impl TestDraw for world_models_gpu::World {
    fn needs_depth(&self) -> bool {
        true
    }
    fn submit<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        self.draw(pass);
    }
}
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
fn check(device: &wgpu::Device, queue: &wgpu::Queue, draw: &dyn TestDraw, expected: [[u8; 4]; 2]) {
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
    let depth = draw.needs_depth().then(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    });
    let depth_view = depth.as_ref().map(|d| d.create_view(&Default::default()));
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
            depth_stencil_attachment: depth_view.as_ref().map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        draw.submit(&mut p);
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
    material.two_sided = false;
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
        blend_enabled: true,
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
    // A BC1 DDS decoded into RGBA8 must retain its source queue category.
    assert_eq!(draw.alpha_texture(0), Some(false));
    assert_eq!(draw.alpha_texture(1), Some(false));
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
    // Descriptor RGB selects the clamped scaled-material method; alpha remains
    // separately controlled. Restore original method before the world fixture.
    asset.descriptor.original_color = Some(0xff804020);
    draw.update(&queue, &asset, &poses(&asset), dim).unwrap();
    check(&device, &queue, &draw, [[16, 16, 12, 255], [48, 8, 8, 255]]);
    asset.descriptor.original_color = None;
    let playback = model_playback::Playback::new(&asset, None).unwrap();
    let scene_assets = world_models::Assets {
        assets: vec![asset],
        instances: vec![world_models::Instance {
            placement: scene::SceneModel {
                export_id: 1,
                position: [0.; 3],
                direction: [0., 0., 1.],
                up: [0., 1., 0.],
                path: "models/a/a.ecm".into(),
                action: "absent".into(),
            },
            asset: 0,
            action: None,
            playback: Ok(playback),
        }],
        failures: vec![],
        unloaded: vec![],
    };
    let mut opaque = frame;
    opaque.blend_enabled = false;
    opaque.lighting.transparency = -1.;
    opaque.alpha_compare = 5;
    opaque.alpha_reference = 84. / 255.;
    let mut opaque_dim = dim;
    opaque_dim.blend_enabled = false;
    opaque_dim.lighting.transparency = -1.;
    opaque_dim.alpha_compare = 5;
    opaque_dim.alpha_reference = 84. / 255.;
    let mut world = world_models_gpu::World::with_reader(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        scene_assets,
        |_| {
            let mut b = white_dds();
            b[84..88].copy_from_slice(b"DXT3");
            b.splice(128..128, [0x88; 8]);
            Ok(b)
        },
    )
    .unwrap();
    assert_eq!(world.status(), (0, 0, 1));
    world.update(
        &device,
        &queue,
        opaque,
        [0.; 3],
        &scene_lights::SceneLights::default(),
        0.,
        0,
    );
    check(
        &device,
        &queue,
        &world,
        [[64, 128, 191, 136], [191, 64, 128, 136]],
    );
    world.update(
        &device,
        &queue,
        opaque_dim,
        [0.; 3],
        &scene_lights::SceneLights::default(),
        0.,
        17,
    );
    check(
        &device,
        &queue,
        &world,
        [[32, 64, 96, 136], [96, 32, 64, 136]],
    );
    // Main projection mirrors X. Correct front interpretation is mandatory for
    // one-sided authored materials, not just the synthetic identity camera.
    let mut mirrored = opaque_dim;
    mirrored.matrix = glam::Mat4::from_scale(glam::Vec3::new(-1., 1., 1.)).to_cols_array_2d();
    mirrored.reverse_front = true;
    world.update(
        &device,
        &queue,
        mirrored,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    check(
        &device,
        &queue,
        &world,
        [[96, 32, 64, 136], [32, 64, 96, 136]],
    );
    mirrored.reverse_front = false;
    world.update(
        &device,
        &queue,
        mirrored,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    check(&device, &queue, &world, [[0; 4]; 2]);
    // Reflection also mirrors Y: the two flips restore the original winding.
    mirrored.matrix = glam::Mat4::from_scale(glam::Vec3::new(-1., -1., 1.)).to_cols_array_2d();
    world.update(
        &device,
        &queue,
        mirrored,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    check(
        &device,
        &queue,
        &world,
        [[96, 32, 64, 136], [32, 64, 96, 136]],
    );
    // Clip in original world space, even when the camera is mirrored.
    mirrored.clip_plane = [1., 0., 0., 0.];
    world.update(
        &device,
        &queue,
        mirrored,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    check(&device, &queue, &world, [[96, 32, 64, 136], [0; 4]]);
    mirrored.clip_plane = [0.; 4];
    mirrored.fog_range = [0., 2., 1., 0.];
    mirrored.fog_color = [0.; 4];
    world.update(
        &device,
        &queue,
        mirrored,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    check(
        &device,
        &queue,
        &world,
        [[48, 16, 32, 136], [16, 32, 48, 136]],
    );
    let lights = scene_lights::SceneLights {
        lights: vec![scene_lights::SceneLight {
            position: [0., 0., 0.5],
            range: 2.,
            stored_attenuation: [0., 0.4, 0.],
            diffuse: [0., 0., 0., 1.],
            ambient: [0.5, 0.5, 0.5, 1.],
            bounds_center: [0., 0., 0.5],
            bounds_half_extent: [2.; 3],
            bounds_min: [-2., -2., -1.5],
            bounds_max: [2., 2., 2.5],
            flags: 1,
        }],
    };
    world.update(&device, &queue, opaque_dim, [13., 0., 0.], &lights, 0., 17);
    check(
        &device,
        &queue,
        &world,
        [[32, 64, 96, 136], [96, 32, 64, 136]],
    );
    world.update(&device, &queue, opaque_dim, [13., 0., 0.], &lights, 1., 17);
    check(
        &device,
        &queue,
        &world,
        [[64, 128, 191, 136], [191, 64, 128, 136]],
    );
    assert_eq!(world.status(), (1, 0, 1));
    let mut unsupported = opaque_dim;
    unsupported.blend_enabled = true;
    world.update(
        &device,
        &queue,
        unsupported,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    assert_eq!(world.status(), (0, 1, 1));
    world.update(
        &device,
        &queue,
        opaque_dim,
        [0.; 3],
        &Default::default(),
        0.,
        0,
    );
    assert_eq!(world.status(), (0, 1, 1)); // rejected instances are not retried as stand-ins
    println!("::notice::world instance pipeline passed: source placement basis, no-action bind pose, authored texture/material, reversed-depth attachment, and updated regional lighting");
    println!("::notice::retained model draw passed: two material meshes, shared authored DDS with decode-failure retry, frame update, rejected-update atomicity, and posed geometry movement");
}
