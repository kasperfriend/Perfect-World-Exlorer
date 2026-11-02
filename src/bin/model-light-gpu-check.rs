//! GPU regression for the standalone retail-inspired model light helper.
//! This does not imply that animated model rendering is implemented.
#[path = "../model_light.rs"]
mod model_light;
use model_light::{vertex_light, LightInput};

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
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 6 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 6 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shader = format!(
        "{}\n{}",
        model_light::WGSL,
        r#"
@group(0) @binding(0) var<storage,read_write> result:array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x;
 var normal=vec3<f32>(0.,1.,0.);var light=vec3<f32>(0.,10.,0.);var enabled=1.;
 var ambient=vec3<f32>(0.1,0.2,0.3);
 if i==1u {enabled=0.;}
 if i==2u {normal=vec3<f32>(0.,-1.,0.);}
 if i==3u {light=vec3<f32>(0.);}
 if i==4u {ambient=vec3<f32>(2.,3.,4.);}
 if i==5u {light=vec3<f32>(10.,10.,0.);normal=vec3<f32>(2.,2.,0.);}
 let c=model_vertex_light(vec3<f32>(0.),normal,vec3<f32>(0.,-1.,0.),light,vec3<f32>(0.,0.4,0.),ambient,vec3<f32>(0.2,0.1,0.),vec3<f32>(0.4,0.2,0.1),vec3<f32>(0.04,0.08,0.12),enabled);
 result[i]=vec4<f32>(c,1.);
}
"#
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: "main",
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(6, 1, 1);
    }
    enc.copy_buffer_to_buffer(&output, 0, &read, 0, 96);
    queue.submit(Some(enc.finish()));
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range();
    let results: &[f32] = bytemuck::cast_slice(&bytes);
    for i in 0..6 {
        let mut p = LightInput {
            world_position: [0.; 3],
            normal: [0., 1., 0.],
            direction: [0., -1., 0.],
            point_position: [0., 10., 0.],
            attenuation: [0., 0.4, 0.],
            other_ambient: [0.1, 0.2, 0.3],
            directional_diffuse: [0.2, 0.1, 0.],
            point_diffuse: [0.4, 0.2, 0.1],
            point_ambient: [0.04, 0.08, 0.12],
            point_enabled: true,
        };
        match i {
            1 => p.point_enabled = false,
            2 => p.normal = [0., -1., 0.],
            3 => p.point_position = [0.; 3],
            4 => p.other_ambient = [2., 3., 4.],
            5 => {
                p.point_position = [10., 10., 0.];
                p.normal = [2., 2., 0.];
            }
            _ => {}
        }
        let expected = vertex_light(p);
        let actual = &results[i * 4..i * 4 + 3];
        for j in 0..3 {
            assert!(
                (expected[j] - actual[j]).abs() < 1e-5,
                "case {i}: CPU {expected:?} GPU {actual:?}"
            );
        }
        println!("model light case {i}: {actual:?}");
    }
}
