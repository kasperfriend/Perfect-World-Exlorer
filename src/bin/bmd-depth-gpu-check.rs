//! Raster regression for a real coincident BMD depth conflict. The retail
//! world pass leaves `ZFUNC` at `LESSEQUAL` (`0x7a9808..0x7a99bc` writes no
//! `ZFUNC`/`ZBIAS`); under the viewer's reversed projection that is
//! `GREATER_EQUAL` for baked scenery, so the later of two coincident
//! submissions wins. A `GREATER` control keeps the first surface.
#![allow(dead_code)]
#[path = "../bmd_baked.rs"]
mod bmd_baked;
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@group(0) @binding(0) var<uniform> tint: vec4<f32>;
@vertex fn vs(@builtin(vertex_index) id:u32)->VOut {
    var p = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    var o:VOut;
    o.pos = vec4<f32>(p[id],0.5,1.);
    o.color = tint;
    return o;
}
@fragment fn fs(i:VOut)->@location(0) vec4<f32> { return i.color; }
"#;

fn main() {
    let production = include_str!("../gpu.rs");
    assert!(
        production.contains("depth_compare: wgpu::CompareFunction::GreaterEqual"),
        "production scenery pipeline must keep reversed-depth GREATER_EQUAL"
    );
    assert_eq!(
        bmd_baked::overlap_outcome(true, false, false, true),
        bmd_baked::OverlapOutcome::LaterSubmissionWins,
        "same-submission coincident pair must resolve to the later draw"
    );
    assert_eq!(
        bmd_baked::overlap_outcome(true, false, false, false),
        bmd_baked::OverlapOutcome::Unresolved,
        "cross-page coincident pair must not assume an order"
    );

    let inst = wgpu::Instance::default();
    let adapter = pollster::block_on(inst.request_adapter(&Default::default())).expect("adapter");
    let (d, q) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let make = |compare| {
        d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: compare,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        })
    };
    let equal = make(wgpu::CompareFunction::GreaterEqual);
    let strict = make(wgpu::CompareFunction::Greater);
    let first = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[1.0f32, 0., 0., 1.]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let second = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[0.0f32, 1., 0., 1.]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind = |pipeline: &wgpu::RenderPipeline, buffer: &wgpu::Buffer| {
        d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    };
    let size = wgpu::Extent3d {
        width: 64,
        height: 1,
        depth_or_array_layers: 1,
    };
    let target = d.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = d.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let read = d.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for (pipeline, expected, label) in [
        (&equal, [0u8, 255, 0, 255], "GREATER_EQUAL later wins"),
        (&strict, [255u8, 0, 0, 255], "GREATER first wins"),
    ] {
        let b0 = bind(pipeline, &first);
        let b1 = bind(pipeline, &second);
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &b0, &[]);
            pass.draw(0..3, 0..1);
            // Exactly coincident geometry at exactly the same depth.
            pass.set_bind_group(0, &b1, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &read,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            size,
        );
        q.submit(Some(enc.finish()));
        let slice = read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        d.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let bytes = slice.get_mapped_range();
        let pixel = &bytes[(32 * 4)..(32 * 4 + 4)];
        assert_eq!(pixel, expected, "{label}: {pixel:?}");
        println!("{label}: {:?}", pixel);
        drop(bytes);
        read.unmap();
    }
    println!("::notice::coincident BMD depth conflict: later submission wins under GREATER_EQUAL");
}
