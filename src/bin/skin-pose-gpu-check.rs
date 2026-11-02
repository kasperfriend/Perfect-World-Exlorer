//! Synthetic compute regression: skin transform feeding point+directional lighting.
//! Not connected to the world renderer and not a real-client model test.
#![allow(dead_code)]
#[path = "../model_light.rs"]
mod model_light;
#[path = "../skin_pose.rs"]
mod skin_pose;
use glam::{Mat4, Vec3};
fn main() {
    let inst = wgpu::Instance::default();
    let adapter = pollster::block_on(inst.request_adapter(&Default::default())).unwrap();
    let (dev, q) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    let out = dev.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4 * 3 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = dev.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4 * 3 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let source = format!(
        "{}\n{}\n{}",
        skin_pose::WGSL,
        model_light::WGSL,
        r#"
@group(0) @binding(0) var<storage,read_write> output:array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x;
 let a=mat4x4<f32>(vec4<f32>(1.,0.,0.,0.),vec4<f32>(0.,1.,0.,0.),vec4<f32>(0.,0.,1.,0.),vec4<f32>(4.,0.,0.,1.));
 let b=mat4x4<f32>(vec4<f32>(0.,1.,0.,0.),vec4<f32>(-1.,0.,0.,0.),vec4<f32>(0.,0.,1.,0.),vec4<f32>(0.,4.,0.,1.));
 let c=mat4x4<f32>(vec4<f32>(1.,0.,0.,0.),vec4<f32>(0.,1.,0.,0.),vec4<f32>(0.,0.,1.,0.),vec4<f32>(0.,0.,0.,1.));
 let d=mat4x4<f32>(vec4<f32>(1.,0.,0.,0.),vec4<f32>(0.,1.,0.,0.),vec4<f32>(0.,0.,1.,0.),vec4<f32>(0.,0.,4.,1.));
 var w=vec3<f32>(0.25,0.5,0.); if i==1u {w=vec3<f32>(1.,0.,0.);} if i==2u {w=vec3<f32>(0.,1.,0.);} if i==3u {w=vec3<f32>(0.,0.,0.);}
 let v=deform_skin(vec3<f32>(1.,2.,3.),vec3<f32>(0.,1.,0.),w,a,b,c,d);
 let light=model_vertex_light(v.position,v.normal,vec3<f32>(0.,-1.,0.),vec3<f32>(0.,10.,0.),vec3<f32>(0.,0.4,0.),vec3<f32>(0.1,0.2,0.3),vec3<f32>(0.2,0.1,0.),vec3<f32>(0.4,0.2,0.1),vec3<f32>(0.04,0.08,0.12),1.);
 output[3u*i]=vec4<f32>(v.position,1.);output[3u*i+1u]=vec4<f32>(v.normal,0.);output[3u*i+2u]=vec4<f32>(light,1.);
}
"#
    );
    let sh = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipe = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &sh,
        entry_point: "main",
    });
    let bind = dev.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipe.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: out.as_entire_binding(),
        }],
    });
    let mut e = dev.create_command_encoder(&Default::default());
    {
        let mut p = e.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        p.set_pipeline(&pipe);
        p.set_bind_group(0, &bind, &[]);
        p.dispatch_workgroups(4, 1, 1);
    }
    e.copy_buffer_to_buffer(&out, 0, &read, 0, 192);
    q.submit(Some(e.finish()));
    let s = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    s.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    dev.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = s.get_mapped_range();
    let vals: &[f32] = bytemuck::cast_slice(&bytes);
    let matrices = [
        Mat4::from_translation(Vec3::new(4., 0., 0.)),
        Mat4::from_rotation_translation(
            glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Vec3::new(0., 4., 0.),
        ),
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(0., 0., 4.)),
    ];
    for (i, w) in [[0.25, 0.5, 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 0.]]
        .into_iter()
        .enumerate()
    {
        let v = skin_pose::deform([1., 2., 3.], [0., 1., 0.], w, [0, 1, 2, 3], &matrices).unwrap();
        let light = model_light::vertex_light(model_light::LightInput {
            world_position: v.position,
            normal: v.normal,
            direction: [0., -1., 0.],
            point_position: [0., 10., 0.],
            attenuation: [0., 0.4, 0.],
            other_ambient: [0.1, 0.2, 0.3],
            directional_diffuse: [0.2, 0.1, 0.],
            point_diffuse: [0.4, 0.2, 0.1],
            point_ambient: [0.04, 0.08, 0.12],
            point_enabled: true,
        });
        for (j, expected) in [v.position, v.normal, light].iter().enumerate() {
            for k in 0..3 {
                assert!(
                    (vals[i * 12 + j * 4 + k] - expected[k]).abs() < 0.00001,
                    "case {i} value {j} component {k}"
                );
            }
        }
        println!(
            "case {i} position {:?}, normal {:?}, lit {:?}",
            v.position, v.normal, light
        );
    }
}
