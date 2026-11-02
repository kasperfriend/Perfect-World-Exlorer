//! Bit-exact comparison of the production BMD integer color blend with the
//! scalar 0x7b9bc0 reference for every 8-bit night weight and color extremes.
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
    let code = include_str!("../gpu.rs");
    let production = code
        .split("const SHADER: &str = r#\"")
        .nth(1)
        .unwrap()
        .split("\"#;")
        .next()
        .unwrap();
    let shader = format!(
        "{production}\n{}",
        r#"
@group(0) @binding(3) var<storage,read_write> result:array<u32>;
@compute @workgroup_size(64) fn check(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x;let weight=i%256u;let pattern=i/256u;
 var day=0xffffffffu;var night=0xff000000u;
 if pattern==1u {day=0xff000000u;night=0xffffffffu;}
 if pattern==2u {day=0xffe17f02u;night=0xff03f18du;}
 if pattern==3u {day=0xff010101u;night=0xfffefefeu;}
 if pattern==4u {day=0xff808080u;night=day;}
 if pattern==5u {day=0xff00ff01u;night=0xffff00feu;}
 if pattern==6u {day=0x002d4056u;night=0x00323e54u;}
 if pattern==7u {day=0x00ffffffu;night=day;}
 // Interior of each quantized weight interval avoids CPU/GPU tie ambiguity
 // in input factor generation; the byte-mixing result must be EXACT.
 let factor=min((f32(weight)+0.25)/255.,1.);
 let c=bmd_baked_rgb(day,night,factor);
 result[i]=0xff000000u|(c.r<<16u)|(c.g<<8u)|c.b;
}
"#
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production BMD blend verification"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: "check",
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 2048 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 2048 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 3,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &read, 0, 2048 * 4);
    queue.submit(Some(encoder.finish()));
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    let colors: &[(u32, u32)] = &[
        (0xffffffff, 0xff000000),
        (0xff000000, 0xffffffff),
        (0xffe17f02, 0xff03f18d),
        (0xff010101, 0xfffefefe),
        (0xff808080, 0xff808080),
        (0xff00ff01, 0xffff00fe),
        (0x002d4056, 0x00323e54),
        (0x00ffffff, 0x00ffffff),
    ];
    for (pattern, &(day, night)) in colors.iter().enumerate() {
        for w in 0..256u32 {
            let channel = |shift: u32| -> u32 {
                ((((day >> shift) & 255) * (255 - w) + ((night >> shift) & 255) * w) >> 8) << shift
            };
            let expected = 0xff000000 | channel(16) | channel(8) | channel(0);
            let i = (pattern * 256 + w as usize) * 4;
            let actual = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
            assert_eq!(actual, expected, "BMD pattern {pattern}, weight {w}");
        }
    }
    println!("BMD integer day/night blend: 2048 bit-exact production GPU cases passed");
}
