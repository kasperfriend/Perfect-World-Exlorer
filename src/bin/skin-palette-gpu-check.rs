//! Production four-matrix skin deformation versus CPU, including the reserved
//! model slot, implicit fourth weight, rotations/scales and zero normals.
#![allow(dead_code)]
#[path = "../skin_pose.rs"]
mod skin_pose;
use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

fn main() {
    std::panic::set_hook(Box::new(|p| {
        eprintln!("::error::{}", p.to_string().replace('\n', "%0A"))
    }));
    let matrices = [
        Mat4::from_rotation_translation(Quat::from_rotation_x(0.4), Vec3::new(2., 3., 4.)),
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.5, 2., 1.5),
            Quat::from_rotation_y(-0.7),
            Vec3::new(-3., 5., 1.),
        ),
        Mat4::from_rotation_translation(Quat::from_rotation_z(1.2), Vec3::new(8., -2., 3.)),
        Mat4::from_translation(Vec3::new(10., 4., -12.)),
    ];
    let mut data = Vec::<f32>::new();
    let mut expected = Vec::new();
    for normal in [[0., 2., 1.], [0.; 3]] {
        for weights in [[1., 0., 0.], [0.25, 0.5, 0.], [0.; 3], [1.25, -0.5, 0.125]] {
            let position = [1., -2., 3.];
            let mut row = [0.; 76];
            for (i, m) in matrices.iter().enumerate() {
                row[i * 16..i * 16 + 16].copy_from_slice(&m.to_cols_array());
            }
            row[64..67].copy_from_slice(&position);
            row[68..71].copy_from_slice(&normal);
            row[72..75].copy_from_slice(&weights);
            data.extend_from_slice(&row);
            expected.push(
                skin_pose::deform(position, normal, weights, [0, 1, 2, 3], &matrices).unwrap(),
            );
        }
    }
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
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("skin cases"),
        contents: bytemuck::cast_slice(&data),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let size = expected.len() as u64 * 32;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let source = format!(
        "{}\n{}",
        skin_pose::WGSL,
        r#"
struct Case {m:array<mat4x4<f32>,4>,position:vec4<f32>,normal:vec4<f32>,weights:vec4<f32>};
@group(0) @binding(0) var<storage,read> cases:array<Case>;
@group(0) @binding(1) var<storage,read_write> result:array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 let c=cases[id.x];let p=deform_skin(c.position.xyz,c.normal.xyz,c.weights.xyz,c.m[0],c.m[1],c.m[2],c.m[3]);
 result[id.x*2]=vec4<f32>(p.position,1.);result[id.x*2+1]=vec4<f32>(p.normal,0.);
}
"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production skin deformation"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: "main",
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
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
        pass.dispatch_workgroups(expected.len() as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &read, 0, size);
    queue.submit(Some(encoder.finish()));
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range();
    let values: &[f32] = bytemuck::cast_slice(&bytes);
    for (i, expected) in expected.iter().enumerate() {
        for (channel, want) in expected
            .position
            .iter()
            .chain(expected.normal.iter())
            .enumerate()
        {
            let index = i * 8 + if channel < 3 { channel } else { channel + 1 };
            let actual = values[index];
            assert!(
                actual.is_finite() && (actual - want).abs() < 0.0001,
                "skin case {i}, channel {channel}: {actual} != {want}"
            );
        }
        assert_eq!(values[i * 8 + 3], 1.);
        assert_eq!(values[i * 8 + 7], 0.);
    }
    println!(
        "{} production skin position/normal comparisons passed",
        expected.len()
    );
    drop(bytes);
    read.unmap();
}
