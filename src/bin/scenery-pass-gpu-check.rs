//! Raster regression for the recovered EC-BSD visibility flags and the
//! one-snapshot-per-frame rule. Three synthetic placements (hidden,
//! reflect-only, refract-only) are submitted through the production
//! `scenery_visibility::allows` gate in every pass; hidden placements must not
//! appear in reflection/refraction. Also exercises regional distance culling
//! and eviction, and enumerates the pass x recovered-material matrix.
#![allow(dead_code)]
#[path = "../bmd_baked.rs"]
mod bmd_baked;
#[path = "../scenery_visibility.rs"]
mod scenery_visibility;
use scenery_visibility::{
    allows, allows_class_with_context, allows_with_context, Class, ModelClass, Pass,
    RegionResidency, RenderContext, Visibility, NATIVE_DEFAULT_WATER_EFFECT,
};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
}

const SHADER: &str = r#"
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>)->VOut {
    var o:VOut;
    o.pos = vec4<f32>(pos,0.5,1.);
    o.color = color;
    return o;
}
@fragment fn fs(i:VOut)->@location(0) vec4<f32> { return i.color; }
"#;

#[derive(Clone, Copy)]
struct Range {
    start: u32,
    end: u32,
    visibility: Visibility,
}

fn placements() -> [Visibility; 3] {
    [
        Visibility {
            reflect: false,
            refract: false,
        }, // hidden
        Visibility {
            reflect: true,
            refract: false,
        }, // reflection only
        Visibility {
            reflect: false,
            refract: true,
        }, // refraction only
    ]
}

fn expected(pass: Pass) -> [[u8; 4]; 3] {
    let hidden = [0, 0, 0, 255];
    match pass {
        Pass::Reflection => [hidden, [0, 255, 0, 255], hidden],
        Pass::Refraction => [hidden, hidden, [0, 0, 255, 255]],
        Pass::Main | Pass::Fog | Pass::Minimap | Pass::FullMap => {
            [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        }
    }
}

fn cpu_regressions(production: &str) {
    // One snapshot, zero secondary playback/light deltas.
    let snapshot = scenery_visibility::FrameSnapshot::new(12.5, 16);
    for pass in Pass::ALL {
        let view = snapshot.secondary(pass);
        assert_eq!(view.elapsed, 12.5);
        assert_eq!(view.logic_delta_ms, 0);
        assert!(!view.advances_time);
        assert!(!view.consumes_light_publications);
    }
    assert_eq!(snapshot.time_advances, 1);
    assert_eq!(snapshot.light_publications, 1);
    assert!(production.contains("let frame = self.begin_frame(camera, w, h)"));
    assert!(production.contains("let frame = self.frame;"));
    assert!(production.contains("page.draw_selected(&mut p, reflected, pass_kind)"));

    // Hidden placement, distance-culled placement, and a real eviction.
    let ps = [
        scenery_visibility::Placement {
            index: 0,
            center: [0., 0.],
            visibility: Visibility {
                reflect: false,
                refract: false,
            },
            class: Class::Ornaments,
        },
        scenery_visibility::Placement {
            index: 1,
            center: [1000., 0.],
            visibility: Visibility {
                reflect: false,
                refract: false,
            },
            class: Class::Ornaments,
        },
    ];
    let mut residency = RegionResidency::default();
    assert_eq!(residency.select(&ps, [0., 0.], 2, 32.), vec![0]);
    assert_eq!(residency.loads, 1);
    assert_eq!(residency.distance_culls, 1);
    assert!(residency.select(&ps, [200., 0.], 2, 32.).is_empty());
    assert_eq!(residency.evictions, 1);
    assert_eq!(residency.resident_len(), 0);

    // Pass x recovered-material matrix. Every inspected BMD mesh state is
    // admitted to every pass by the shared scenery pipeline; the per-placement
    // flags, not the material state, decide reflection/refraction.
    let mut rows = 0usize;
    for pass in Pass::ALL {
        for (version, textured, two_sided) in [
            (0x1000_0004, false, false),
            (0x1000_0004, true, false),
            (0x1000_0005, false, false),
            (0x1000_0005, false, true),
            (0x1000_0005, true, false),
            (0x1000_0005, true, true),
        ] {
            let state = bmd_baked::classify(version, textured, two_sided)
                .unwrap_or_else(|e| panic!("unsupported recovered material: {e}"));
            assert!(state.kind > 0., "classified kind");
            assert!(
                allows(
                    pass,
                    Visibility {
                        reflect: true,
                        refract: true
                    }
                ),
                "a fully visible placement must be admitted in {pass:?}"
            );
            rows += 1;
            println!(
                "pass {} material v{:x} textured {} two-sided {} kind {}",
                pass.name(),
                version,
                textured,
                two_sided,
                state.kind
            );
        }
    }
    assert_eq!(rows, 36, "six passes x six inspected material states");
    // Recovered class / context gates at the shipped default and level 2.
    let default = RenderContext::default();
    assert_eq!(default.water_effect, NATIVE_DEFAULT_WATER_EFFECT);
    assert!(!allows_class_with_context(
        Pass::Reflection,
        ModelClass::HomeOrnament,
        default
    ));
    assert!(allows_class_with_context(
        Pass::Main,
        ModelClass::HomeOrnament,
        default
    ));
    let level2 = RenderContext::new(2, true);
    assert!(allows_class_with_context(
        Pass::Reflection,
        ModelClass::EcModel,
        level2
    ));
    assert!(allows_class_with_context(
        Pass::Refraction,
        ModelClass::EcModel,
        level2
    ));
    let visible = Visibility {
        reflect: true,
        refract: true,
    };
    assert!(!allows_with_context(Pass::Reflection, visible, level2));
    assert!(!allows_with_context(Pass::Refraction, visible, level2));
    println!("context gates: default WaterEffect=0, level-2/underwater encoded");
}

fn main() {
    let production = include_str!("../gpu.rs");
    cpu_regressions(production);

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
    let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: "vs",
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x4,
                    },
                ],
            }],
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
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
    });

    let colors = [[1.0f32, 0., 0., 1.], [0., 1., 0., 1.], [0., 0., 1., 1.]];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut ranges = Vec::new();
    for (i, color) in colors.iter().enumerate() {
        let x0 = -1. + 0.5 * i as f32;
        let x1 = x0 + 0.25;
        let base = vertices.len() as u32;
        for (x, y) in [(x0, -1.), (x1, -1.), (x0, 1.), (x1, 1.)] {
            vertices.push(Vertex {
                pos: [x, y],
                color: *color,
            });
        }
        let start = indices.len() as u32;
        indices.extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        ranges.push(Range {
            start,
            end: indices.len() as u32,
            visibility: placements()[i],
        });
    }
    let vertex = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
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
    let read = d.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 8192,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    for pass in Pass::ALL {
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
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
            rp.set_pipeline(&pipeline);
            rp.set_vertex_buffer(0, vertex.slice(..));
            rp.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
            for r in &ranges {
                if allows(pass, r.visibility) {
                    rp.draw_indexed(r.start..r.end, 0, 0..1);
                }
            }
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
        let got: [[u8; 4]; 3] = (0..3)
            .map(|i| {
                let x = 8 + i * 32;
                let o = (8 * 128 + x) * 4;
                [bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let want = expected(pass);
        assert_eq!(got, want, "pass {} visibility raster", pass.name());
        println!("pass {}: {:?}", pass.name(), got);
        drop(bytes);
        read.unmap();
    }
    println!(
        "::notice::hidden, reflect-only and refract-only placements are gated in all six passes"
    );
}
