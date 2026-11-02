//! GPU regression for the standalone retail-inspired model light helper.
//! This does not imply that animated model rendering is implemented.
#[path = "../model_light.rs"]
mod model_light;
#[path = "../model_material.rs"]
mod model_material;
use model_light::{vertex_light, LightInput};

const CASES: u32 = 47;

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
        size: CASES as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: CASES as u64 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shader = format!(
        "{}\n{}",
        format!("{}\n{}", model_light::WGSL, model_material::WGSL),
        r#"
@group(0) @binding(0) var<storage,read_write> result:array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x;
 var normal=vec3<f32>(0.,1.,0.);var light=vec3<f32>(0.,10.,0.);var enabled=1.;
 var ambient=vec3<f32>(0.1,0.2,0.3);var att=vec3<f32>(0.,0.4,0.);
 if i==1u {enabled=0.;}
 if i==2u {normal=vec3<f32>(0.,-1.,0.);}
 if i==3u {light=vec3<f32>(0.);}
 if i==4u {ambient=vec3<f32>(2.,3.,4.);}
 if i==5u {light=vec3<f32>(10.,10.,0.);normal=vec3<f32>(2.,2.,0.);}
 if i==6u {normal=vec3<f32>(0.);}
 if i==7u {normal=vec3<f32>(0.,0.0000001,0.);}
 if i==8u {normal=vec3<f32>(0.,0.000001,0.);}
 if i>=9u && i<=11u {att=vec3<f32>(1.,0.,0.);}
 if i==9u {light=vec3<f32>(0.,0.0000001,0.);}
 if i==10u {light=vec3<f32>(0.,0.000001,0.);}
 if i==11u {light=vec3<f32>(0.,0.000002,0.);}
 if i==12u {att=vec3<f32>(-1.,0.,0.);}
 if i==13u {att=vec3<f32>(0.);}
 if i==14u {att=vec3<f32>(1.,0.2,0.01);}
 let c=model_vertex_light(vec3<f32>(0.),normal,vec3<f32>(0.,-1.,0.),light,att,ambient,vec3<f32>(0.2,0.1,0.),vec3<f32>(0.4,0.2,0.1),vec3<f32>(0.04,0.08,0.12),enabled);
 result[i]=vec4<f32>(c,1.);
 if i>=15u && i<31u {
  let k=i-15u; let point_on=(k&1u)!=0u;
  var transparencies=array<f32,4>(0.,0.25,1.,1.2);
  let transparency=transparencies[(k/2u)%4u];
  let dir=vec3<f32>(0.,-1.,0.);var norm=vec3<f32>(0.,1.,0.);
  if k>=8u {norm=-norm;}
  let m=bind_model_material(vec4<f32>(0.5,0.25,0.8,0.2),vec4<f32>(0.8,0.6,0.4,0.7),vec4<f32>(0.02,0.03,0.04,0.01),vec4<f32>(0.2,0.3,0.4,0.5),vec4<f32>(0.01,0.02,0.03,0.04),vec4<f32>(0.3,0.4,0.5,0.6),vec4<f32>(0.1,0.2,0.3,0.4),vec4<f32>(0.4,0.3,0.2,0.9),transparency,point_on);
  result[i]=model_vertex_light_rgba(vec3<f32>(0.),norm,dir,vec3<f32>(0.,10.,0.),vec3<f32>(0.,0.4,0.),m.ambient,m.directional_diffuse,m.point_diffuse,m.point_ambient,select(0.,1.,point_on));
 }
 if i>=31u {
  let k=i-31u;let on=(k&1u)!=0u;
  var normal=vec3<f32>(0.,1.,0.);if k>=8u {normal=-normal;}
  var distances=array<f32,4>(0.25,1.,5.,10.);let light=vec3<f32>(0.,distances[(k/2u)%4u],0.);
  let d=model_specular_variant_diffuse(vec3<f32>(0.),normal,vec3<f32>(0.,-1.,0.),light,vec3<f32>(0.,0.4,0.),vec4<f32>(0.1,0.2,0.3,0.1),vec4<f32>(0.2,0.1,0.,0.6),vec4<f32>(0.4,0.2,0.1,0.),vec4<f32>(0.04,0.08,0.12,0.2),select(0.,1.,on));
  let spec=model_directional_specular(normal,vec3<f32>(0.,-1.,0.),vec3<f32>(0.6,-0.8,0.),10.,vec3<f32>(0.2,0.3,0.4));
  result[i]=vec4<f32>(d.rgb+spec,d.a);
 }

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
        pass.dispatch_workgroups(CASES, 1, 1);
    }
    enc.copy_buffer_to_buffer(&output, 0, &read, 0, CASES as u64 * 16);
    queue.submit(Some(enc.finish()));
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range();
    let results: &[f32] = bytemuck::cast_slice(&bytes);
    for i in 0..CASES as usize {
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
            6 => p.normal = [0.; 3],
            7 => p.normal = [0., 1e-7, 0.],
            8 => p.normal = [0., 1e-6, 0.],
            9..=11 => {
                p.attenuation = [1., 0., 0.];
                p.point_position = [0., [1e-7, 1e-6, 2e-6][i - 9], 0.];
            }
            12 => p.attenuation = [-1., 0., 0.],
            13 => p.attenuation = [0.; 3],
            14 => p.attenuation = [1., 0.2, 0.01],
            _ => {}
        }
        let rgb = vertex_light(p);
        let mut expected = [rgb[0], rgb[1], rgb[2], 1.];
        if i >= 15 && i < 31 {
            let k = i - 15;
            let c = model_material::bind(
                model_material::Material {
                    ambient: [0.5, 0.25, 0.8, 0.2],
                    diffuse: [0.8, 0.6, 0.4, 0.7],
                    emissive: [0.02, 0.03, 0.04, 0.01],
                },
                model_material::Lighting {
                    environment_ambient: [0.2, 0.3, 0.4, 0.5],
                    additional_ambient: [0.01, 0.02, 0.03, 0.04],
                    directional_diffuse: [0.3, 0.4, 0.5, 0.6],
                    point_ambient: [0.1, 0.2, 0.3, 0.4],
                    point_diffuse: [0.4, 0.3, 0.2, 0.9],
                    transparency: [0., 0.25, 1., 1.2][(k / 2) % 4],
                    point_enabled: k & 1 != 0,
                },
            );
            p.normal = if k >= 8 { [0., -1., 0.] } else { [0., 1., 0.] };
            p.point_enabled = k & 1 != 0;
            p.other_ambient = c.ambient[..3].try_into().unwrap();
            p.directional_diffuse = c.directional_diffuse[..3].try_into().unwrap();
            p.point_diffuse = c.point_diffuse[..3].try_into().unwrap();
            p.point_ambient = c.point_ambient[..3].try_into().unwrap();
            expected = model_light::vertex_light_rgba(
                p,
                [c.ambient[3], c.directional_diffuse[3], c.point_ambient[3]],
            );
        }
        if i >= 31 {
            let k = i - 31;
            p.point_enabled = k & 1 != 0;
            p.normal = if k >= 8 { [0., -1., 0.] } else { [0., 1., 0.] };
            p.point_position = [0., [0.25, 1., 5., 10.][(k / 2) % 4], 0.];
            expected = model_light::specular_variant_diffuse(p, [0.1, 0.6, 0.2]);
            let spec = model_light::directional_specular(p, [0.6, -0.8, 0.], 10., [0.2, 0.3, 0.4]);
            for j in 0..3 {
                expected[j] += spec[j];
            }
        }
        let actual = &results[i * 4..i * 4 + 4];
        for j in 0..4 {
            assert!(
                (expected[j] - actual[j]).abs() < 1e-5,
                "case {i}: CPU {expected:?} GPU {actual:?}"
            );
        }
        println!("model light case {i}: {actual:?}");
    }
}
