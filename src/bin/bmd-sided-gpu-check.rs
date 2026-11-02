//! Raster regression using the production BMD fragment shader and real face
//! winding: single-sided, two-sided and mirrored-placement quads, plus the
//! stage-0 alpha rule — `MODULATE(TEXTURE, DIFFUSE)` under the pass's
//! `SRCALPHA`/`INVSRCALPHA` blend, so half-alpha scenery lands halfway and
//! zero-alpha scenery writes nothing.
use wgpu::util::DeviceExt;

/// One case: `flip` is the projection winding parity (`camera.clip.z`),
/// `disable` the culling-disabled flag (`camera.clip.w`), `alpha` the alpha
/// channel of the bound 1x1 texture. The first four cases sweep culling with an
/// opaque atlas; the last two probe the stage-0 alpha at half and zero.
const CASES: [(f32, f32, u8); 6] = [
    (0., 0., 255),
    (1., 0., 255),
    (0., 1., 255),
    (1., 1., 255),
    (0., 1., 128),
    (0., 1., 0),
];

fn main() {
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
    let source = include_str!("../gpu.rs")
        .split("const SHADER: &str = r#\"")
        .nth(1)
        .unwrap()
        .split("\"#;")
        .next()
        .unwrap();
    // Six quads: 0 single-sided CCW, 1 single-sided CW (index order reversed),
    // 2 two-sided CCW, 3 two-sided CW, 4 single-sided placed by a mirrored
    // basis (negative determinant: positions mirrored in X), 5 two-sided
    // mirrored. The mirrored quads emulate `x = y.cross(z)` for an ornament
    // whose placed basis has a negative determinant.
    let source = format!(
        "{source}\n{}\n",
        r#"
@vertex fn test_vs(@builtin(vertex_index) id:u32,@builtin(instance_index) inst:u32)->VOut {
 var o:VOut;
 var base=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(0.,1.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));
 var idx=id;
 if inst==1u || inst==3u {if id%3u==1u {idx=id+1u;} else if id%3u==2u {idx=id-1u;}}
 var p=base[idx];
 if inst>=4u {p=vec2<f32>(1.-p.x,p.y);}
 o.pos=vec4<f32>(-1.+0.25*(f32(inst)+p.x),-1.+2.*p.y,0.5,1.);
 o.normal=vec3<f32>(0.,1.,0.);o.uv=vec2<f32>(0.5);o.color=vec4<f32>(0.,0.,1.,1.);
 o.world=vec3<f32>(0.);o.diffuse=vec3<f32>(0.5);
 o.kind=select(2.,2.1,inst==2u || inst==3u || inst==5u);return o;
}
"#
    );
    let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: "test_vs",
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: "fs",
            // The production `scenery_pipeline` blend: the client's
            // `ALPHABLENDENABLE(1)` with `SRCALPHA`/`INVSRCALPHA`.
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
    });
    let atlas = d.create_texture(&wgpu::TextureDescriptor {
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
    let atlas_view = atlas.create_view(&Default::default());
    let sampler = d.create_sampler(&Default::default());
    // 16 pixels per case, 128 wide keeps COPY_BYTES_PER_ROW_ALIGNMENT.
    let size = wgpu::Extent3d {
        width: 128,
        height: 16,
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
    for (flip, disable, alpha) in CASES {
        q.write_texture(
            atlas.as_image_copy(),
            &[255, 255, 255, alpha],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let mut uniform = [0f32; 388];
        uniform[22] = flip;
        uniform[23] = disable;
        let buffer = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let read = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 8192,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..6, 0..6);
        }
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &read,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(512),
                    rows_per_image: Some(16),
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
        let reds: Vec<u8> = (0..6)
            .map(|i| bytes[(8 * 128 + i * 16 + 8) * 4])
            .collect();
        let pixels: Vec<bool> = reds.iter().map(|&red| red > 200).collect();
        if alpha == 255 {
            // Two-sided meshes are never dropped, mirrored or not (`CULLMODE(1)`
            // at 0x7a98cf). Single-sided meshes keep only the front facing the
            // camera; the mirrored placement reverses `front_facing` exactly as
            // the negative-determinant basis did in the client.
            let two_sided = [false, false, true, true, false, true];
            let mut expected = [false; 6];
            for i in 0..6 {
                expected[i] = if disable != 0. || two_sided[i] {
                    true
                } else {
                    // CCW quads face the camera when the projection is not
                    // flipped; the mirrored instance is the opposite of its
                    // cell neighbour.
                    let mirrored = i >= 4;
                    let ccw = !matches!(i, 1 | 3);
                    (ccw != mirrored) == (flip == 0.)
                };
            }
            assert_eq!(
                pixels, expected,
                "flip {flip}, culling disabled {disable}, opaque atlas {alpha}"
            );
        } else if alpha == 128 {
            // `ALPHAOP(4)` is MODULATE of TEXTURE and DIFFUSE (`0x7a9785`,
            // `0x7a9792`) over the pass's SRCALPHA/INVSRCALPHA blend, so a
            // half-transparent texel must land halfway to the cleared target.
            // An opaque shader would write 255 here.
            assert!(
                reds.iter().all(|&red| (110..=145).contains(&red)),
                "half-alpha scenery did not blend: {reds:?}"
            );
        } else {
            // Alpha 0 contributes nothing: this is the canopy case that used to
            // render as a solid card under an opaque shader.
            assert!(
                reds.iter().all(|&red| red == 0),
                "zero-alpha scenery still wrote the surface: {reds:?}"
            );
        }
        println!("flip {flip} culled {disable} alpha {alpha}: {reds:?}");
        drop(bytes);
        read.unmap();
    }
}
