//! GPU-instanced, runtime-derived far-tree LOD covering all scene placements.
use crate::{gpu::Vertex, trees::Trees};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    position: [f32; 3],
    info: [f32; 2], // transition progress; 0 single, 1 outgoing, 2 incoming
}
struct Batch {
    indices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
}
struct Group {
    positions: Vec<[f32; 3]>,
    height: f32,
    first_batch: usize,
}
pub struct FarTrees {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    reflected_bind: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    batches: Vec<Batch>,
    groups: Vec<Group>,
    staging: Vec<Instance>,
}
const SHADER: &str = r#"
struct Camera {matrix:mat4x4<f32>,direction:vec4<f32>,clip:vec4<f32>,bounds:vec4<f32>,tree_bounds:vec4<f32>,sun:vec4<f32>,light:vec4<f32>,region_ambient:vec4<f32>,region_diffuse:vec4<f32>,region_direction:vec4<f32>,grass_eye:vec4<f32>,grass_wind:vec4<f32>,grass_history:array<vec4<f32>,80>,fog_range:vec4<f32>,fog_color:vec4<f32>};
@group(0) @binding(0) var<uniform> camera:Camera; @group(0) @binding(1) var atlas:texture_2d<f32>; @group(0) @binding(2) var atlas_sampler:sampler;
@group(0) @binding(3) var<storage,read> rocking:array<vec4<f32>>;
fn rock_sample(corner:u32,time:f32)->vec3<f32>{
 let phase=fract(time/16.)*64.;let idx=u32(floor(phase));let next=(idx+1u)%64u;let f=fract(phase);let p=corner*64u;
 var result=mix(rocking[p+idx].xyz,rocking[p+next].xyz,f);
 // Bring the end of the measured interval smoothly back to its start.
 result=mix(result,rocking[p].xyz,clamp((phase-60.)/4.,0.,1.));return result;
}
fn fogged(linear:vec3<f32>, reciprocal_w:f32)->vec3<f32> {
 if camera.fog_range.z<0.5 {return linear;}
 let view_z=1./max(reciprocal_w,0.0000001);
 let span=camera.fog_range.y-camera.fog_range.x;
 let f=select(clamp((view_z-camera.fog_range.x)/select(span,0.001,abs(span)<0.001),0.,1.),select(0.,1.,view_z>=camera.fog_range.y),abs(span)<0.001);
 let c=max(linear,vec3<f32>(0.));let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(encoded,camera.fog_color.rgb,f);
 return select(v/12.92,pow((max(v,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),v>vec3<f32>(0.04045));
}
struct In {@location(0) pos:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) color:vec4<f32>,@location(4) kind:f32,@location(5) instance:vec3<f32>,@location(6) info:vec2<f32>,@location(7) tex_h:u32};
struct Out {@builtin(position) pos:vec4<f32>,@location(0) normal:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) @interpolate(flat) kind:f32,@location(4) world:vec3<f32>,@location(5) @interpolate(flat) tree:vec3<f32>,@location(6) @interpolate(flat) tex_h:f32,@location(7) @interpolate(flat) fade:vec2<f32>};
@vertex fn vs(i:In)->Out {var o:Out;var pos=i.pos+i.instance;var norm=i.normal;
 if i.kind>4.5 {let d=normalize(camera.direction.xyz);let az=atan2(d.z,d.x);let el=asin(clamp(d.y,-1.,1.));let rho=i.color.w;var corner=i.normal;let rock_index=i.tex_h>>8u;if rock_index>0u {corner=mix(corner,rock_sample(rock_index-1u,camera.tree_bounds.w),clamp(camera.tree_bounds.z,0.,1.));}let q=vec3<f32>(cos(el)*corner.x-sin(el)*corner.z,corner.y,sin(el)*corner.x+cos(el)*corner.z);let a=az+rho;var v=vec3<f32>(cos(a)*q.x-sin(a)*q.y,sin(a)*q.x+cos(a)*q.y,q.z);if rho>1000. {v=corner;}pos+=vec3<f32>(v.x,v.z,v.y);norm=vec3<f32>(0.,1.,0.);}
 o.pos=camera.matrix*vec4<f32>(pos,1.);o.world=pos;o.normal=norm;o.uv=i.uv;o.color=i.color;o.kind=i.kind;o.tree=i.instance;o.tex_h=f32(i.tex_h & 127u)*select(128./8192.,128./2048.,(i.tex_h & 128u)!=0u);o.fade=i.info;return o;}
@fragment fn fs(i:Out)->@location(0) vec4<f32>{if camera.clip.y!=0. && (i.world.y-camera.clip.x)*camera.clip.y<0. {discard;}

 let scale=vec2<f32>(i.color.z,i.tex_h);let dx=dpdx(i.uv);let dy=dpdy(i.uv);let dims=vec2<f32>(textureDimensions(atlas));let mip=clamp(log2(max(max(length(dx*dims),length(dy*dims)),1.)),0.,7.);let halfpixel=min(vec2<f32>(0.5*exp2(mip))/dims,scale*0.499);let uv=i.color.xy+clamp(fract((i.uv-i.color.xy)/scale)*scale,halfpixel,scale-halfpixel);let tex=textureSampleGrad(atlas,atlas_sampler,uv,dx,dy);if i.kind>3.5 && tex.a<0.5 {discard;}
 // Complementary ordered coverage: old and new snapshots never both draw
 // at the same pixel, while the tree is fully visible at the endpoints.
 if i.fade.y>0.5 {let xy=vec2<u32>(i.pos.xy)%vec2<u32>(4u);let b=(((xy.x&1u)^(xy.y&1u))*8u)+((xy.y&1u)*4u)+((((xy.x>>1u)&1u)^((xy.y>>1u)&1u))*2u)+((xy.y>>1u)&1u);let d=(f32(b)+0.5)/16.;if (i.fade.y<1.5 && d<i.fade.x)||(i.fade.y>1.5 && d>=i.fade.x){discard;}}
 if camera.region_ambient.w>0.5 {
  let lambert=max(dot(normalize(i.normal),normalize(camera.region_direction.xyz)),0.);
  // These are regional light coefficients, not the original SpeedTree
  // material constants. The branch-only PS 1.4 shader's x2 operation is
  // not justified for every leaf/frond; applying it clipped bright foliage.
  // Keep this fallback bounded and mostly neutral until material constants,
  // normals and the separate client passes are recovered.
  let rgb=clamp(camera.region_ambient.rgb+camera.region_diffuse.rgb*lambert,vec3<f32>(0.),vec3<f32>(1.));
  let luminance=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
  let light=mix(vec3<f32>(luminance),rgb,0.25);
  return vec4<f32>(fogged(tex.rgb*light,i.pos.w),1.);
 }
 let light=camera.light.x+camera.light.y*max(dot(normalize(i.normal),normalize(camera.sun.xyz)),0.);return vec4<f32>(fogged(tex.rgb*light*camera.light.zww,i.pos.w),1.);}
"#;
impl FarTrees {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        trees: &Trees,
        uniform: &wgpu::Buffer,
        reflected: &wgpu::Buffer,
        authored_mips: bool,
    ) -> Self {
        // Real composite/bark textures are mostly 512 and some 1024px.
        // Pack rectangular native-size tiles into an 8192² atlas instead of
        // reducing everything to 256². Keep the low-memory diagnostic mode.
        let small = std::env::var_os("PW_SMALL_TREE_ATLAS").is_some()
            || device.limits().max_texture_dimension_2d < 8192;
        let side = if small { 2048usize } else { 8192usize };
        let grid = side / 128;
        let mut occupied = vec![false; grid * grid];
        let mut atlas = vec![0u8; side * side * 4];
        let mut vertices = Vec::new();
        let mut rocking_samples: Vec<[f32; 4]> = Vec::new();
        let mut indices = Vec::new();
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        let mut groups = Vec::new();
        let mut packed_count = 0usize;
        let mut mip_slots: Vec<(usize, usize, usize, usize, bool, Option<Vec<image::RgbaImage>>)> =
            Vec::new();
        if authored_mips {
            let request = crate::vegetation_material::Request {
                mips: crate::vegetation_material::RequestedMips::Authored,
                anisotropy: crate::vegetation_material::policy(crate::vegetation_material::Material::TreeLeaf)
                    .renderer_sampler
                    .anisotropy,
                cutout_reference: Some(crate::vegetation_material::LEGACY_TREE_ALPHA_REF),
            };
            if let Err(rejection) = crate::vegetation_material::validate(
                crate::vegetation_material::Material::TreeLeaf,
                request,
            ) {
                eprintln!(
                    "P4.07 tree authored-mip experiment: unsupported native sampler pair kept explicit: {rejection:?}; retaining the viewer's approved sampler"
                );
            }
            if let Err(rejection) = crate::vegetation_material::validate_cutout(
                crate::vegetation_material::Material::TreeLeaf,
                Some(crate::vegetation_material::LEGACY_TREE_ALPHA_REF),
            ) {
                eprintln!(
                    "P4.07 tree authored-mip experiment: tree alpha threshold remains unsupported: {rejection:?}; retaining the approved legacy shader cutout"
                );
            }
        }
        for (id, maybe) in trees.types.iter().enumerate() {
            let Some(tree) = maybe else { continue };
            let positions: Vec<_> = trees
                .placements
                .iter()
                .filter(|p| p.tree_type as usize == id)
                .map(|p| p.position)
                .collect();
            if positions.is_empty() {
                continue;
            }
            let height = tree.lods[0]
                .branch
                .vertices
                .iter()
                .chain(tree.lods[0].frond.vertices.iter())
                .map(|v| v.pos[2])
                .chain(tree.lods[0].leaves.iter().map(|l| l.center[2]))
                .fold(5.0f32, f32::max);
            let mut slots = Vec::new();
            for (part_index, img) in [&tree.bark, &tree.composite].into_iter().enumerate() {
                let w = if small {
                    128
                } else {
                    (img.width() as usize).next_power_of_two().clamp(128, side)
                };
                let h = if small {
                    128
                } else {
                    (img.height() as usize).next_power_of_two().clamp(128, side)
                };
                let (gw, gh) = (w / 128, h / 128);
                let mut found = None;
                'scan: for gy in 0..=grid - gh {
                    for gx in 0..=grid - gw {
                        if (gy..gy + gh).all(|y| (gx..gx + gw).all(|x| !occupied[y * grid + x])) {
                            found = Some((gx, gy));
                            break 'scan;
                        }
                    }
                }
                let Some((gx, gy)) = found else {
                    eprintln!("Tree atlas exhausted; type {id} omitted");
                    break;
                };
                for y in gy..gy + gh {
                    for x in gx..gx + gw {
                        occupied[y * grid + x] = true;
                    }
                }
                let (ox, oy) = (gx * 128, gy * 128);
                for y in 0..h {
                    for x in 0..w {
                        let sx = (x as u32 * img.width() / w as u32).min(img.width() - 1);
                        let sy = (y as u32 * img.height() / h as u32).min(img.height() - 1);
                        let dst = ((oy + y) * side + ox + x) * 4;
                        atlas[dst..dst + 4].copy_from_slice(&img.get_pixel(sx, sy).0);
                    }
                }
                slots.push((ox, oy, w, h));
                let authored = if authored_mips {
                    let mips = if part_index == 0 {
                        &tree.bark_mips
                    } else {
                        &tree.composite_mips
                    };
                    if crate::vegetation_textures::chain_fits(
                        mips,
                        w as u32,
                        h as u32,
                        7,
                    ) {
                        if mips.len() > 7 {
                            eprintln!(
                                "P4.07 tree type {id} part {part_index}: {} authored tail mip(s) beyond atlas LOD 7 retained in cache but not separately uploadable",
                                mips.len() - 7
                            );
                        }
                        Some(mips[..7].to_vec())
                    } else {
                        eprintln!(
                            "P4.07 tree type {id} part {part_index}: authored mip chain does not fit {w}x{h} atlas slot; keeping generated mips for this tile"
                        );
                        None
                    }
                } else {
                    None
                };
                mip_slots.push((ox, oy, w, h, part_index == 1, authored));
                packed_count += 1;
            }
            if slots.len() != 2 {
                continue;
            }
            let first_batch = batches.len();
            for lod in &tree.lods {
                let rocking_base = rocking_samples.len() / 64;
                rocking_samples.extend(lod.rocking.iter().map(|v| [v[0], v[1], v[2], 0.]));
                let start_inst = instances.len() as u32;
                instances.extend(positions.iter().map(|&position| Instance {
                    position,
                    info: [0., 0.],
                }));
                let end_inst = instances.len() as u32;
                let start_idx = indices.len() as u32;
                for (part, slot_id, kind) in
                    [(&lod.branch, slots[0], 3.), (&lod.frond, slots[1], 4.)]
                {
                    let ox = slot_id.0 as f32 / side as f32;
                    let oy = slot_id.1 as f32 / side as f32;
                    let scale = slot_id.2 as f32 / side as f32;
                    let sy = slot_id.3 as f32 / side as f32;
                    let base = vertices.len() as u32;
                    for v in &part.vertices {
                        vertices.push(Vertex {
                            pos: [v.pos[0], v.pos[2], v.pos[1]],
                            normal: [v.normal[0], v.normal[2], v.normal[1]],
                            uv: [ox + v.uv[0] * scale, oy + v.uv[1] * sy],
                            color: [ox, oy, scale, 0.],
                            kind,
                            diffuse: (slot_id.3 as u32 / 128) | if small { 128 } else { 0 },
                            night_diffuse: 0xffff_ffff,
                        });
                    }
                    indices.extend(part.indices.iter().map(|i| base + *i));
                }
                let slot_id = slots[1];
                let ox = slot_id.0 as f32 / side as f32;
                let oy = slot_id.1 as f32 / side as f32;
                let scale = slot_id.2 as f32 / side as f32;
                let sy = slot_id.3 as f32 / side as f32;
                for leaf in &lod.leaves {
                    let base = vertices.len() as u32;
                    for j in 0..4 {
                        vertices.push(Vertex {
                            pos: [leaf.center[0], leaf.center[2], leaf.center[1]],
                            normal: leaf.corners[j],
                            uv: [ox + leaf.uv[j][0] * scale, oy + leaf.uv[j][1] * sy],
                            color: [ox, oy, scale, leaf.rho],
                            kind: 5.,
                            diffuse: (slot_id.3 as u32 / 128)
                                | if small { 128 } else { 0 }
                                | if lod.rocking_clusters > 0 {
                                    ((rocking_base + leaf.cluster as usize * 4 + j + 1) as u32) << 8
                                } else {
                                    0
                                },
                            night_diffuse: 0xffff_ffff,
                        });
                    }
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
                batches.push(Batch {
                    indices: start_idx..indices.len() as u32,
                    instances: start_inst..end_inst,
                });
            }
            groups.push(Group {
                positions,
                height,
                first_batch,
            });
        }
        // A valid non-empty allocation is required even for an empty user tree cache.
        if vertices.is_empty() {
            vertices.push(Vertex::zeroed())
        }
        if indices.is_empty() {
            indices.push(0)
        }
        if instances.is_empty() {
            instances.push(Instance::zeroed())
        }
        let image = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("far tree textures"),
            size: wgpu::Extent3d {
                width: side as u32,
                height: side as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 8,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &image,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some((side * 4) as u32),
                rows_per_image: Some(side as u32),
            },
            wgpu::Extent3d {
                width: side as u32,
                height: side as u32,
                depth_or_array_layers: 1,
            },
        );
        // Generate each rectangle separately so lower mip levels cannot blend
        // neighboring trees. Keep unadjusted alpha for the next reduction;
        // preserve approximate cutout coverage only in the uploaded copy.
        for (ox, oy, w, h, cutout, authored) in mip_slots {
            if let Some(chain) = authored {
                for (index, img) in chain.iter().enumerate() {
                    let level = index as u32 + 1;
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &image,
                            mip_level: level,
                            origin: wgpu::Origin3d {
                                x: (ox >> level) as u32,
                                y: (oy >> level) as u32,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        img.as_raw(),
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(img.width() * 4),
                            rows_per_image: Some(img.height()),
                        },
                        wgpu::Extent3d {
                            width: img.width(),
                            height: img.height(),
                            depth_or_array_layers: 1,
                        },
                    );
                }
                continue;
            }
            let mut raw = vec![0u8; w * h * 4];
            for y in 0..h {
                let src = ((oy + y) * side + ox) * 4;
                raw[y * w * 4..(y + 1) * w * 4].copy_from_slice(&atlas[src..src + w * 4]);
            }
            let source_coverage = crate::foliage_mips::coverage(&raw);
            let (mut mw, mut mh) = (w, h);
            for level in 1..8 {
                raw = crate::foliage_mips::downsample(&raw, mw, mh, cutout);
                mw /= 2;
                mh /= 2;
                let corrected = if cutout {
                    crate::foliage_mips::preserve_coverage(&raw, source_coverage)
                } else {
                    raw.clone()
                };
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &image,
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: (ox >> level) as u32,
                            y: (oy >> level) as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &corrected,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some((mw * 4) as u32),
                        rows_per_image: Some(mh as u32),
                    },
                    wgpu::Extent3d {
                        width: mw as u32,
                        height: mh as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        drop(atlas);
        let view = image.create_view(&Default::default());
        // The authored-mip experiment changes mip sources only. Keep the
        // already-approved renderer sampler (including 16x anisotropy) intact.
        let anisotropy_clamp = crate::vegetation_material::LEGACY_ANISOTROPY;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp,
            ..Default::default()
        });
        if rocking_samples.is_empty() {
            rocking_samples.resize(64, [0.; 4]);
        }
        let rocking_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("client sampled SpeedTree leaf rocking"),
            contents: bytemuck::cast_slice(&rocking_samples),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("far trees"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let make_bind = |b: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("far trees bindings"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: rocking_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let bind = make_bind(uniform);
        let reflected_bind = make_bind(reflected);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("far trees shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("far trees"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        const ATTR: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4,4=>Float32,7=>Uint32];
        const INST: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![5=>Float32x3,6=>Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("instanced far trees"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &ATTR,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INST,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("far tree vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("far tree indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_count = instances.len() / 3;
        let staging = instances;
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("all tree placements"),
            contents: bytemuck::cast_slice(&staging),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        eprintln!(
            "Uploaded {} tree placements in {} GPU LOD batches ({} textures, {}px atlas{} with foliage mips)",
            instance_count,
            batches.len(),
            packed_count,
            side,
            if small { " low-memory" } else { " source-size" }
        );
        Self {
            pipeline,
            bind,
            reflected_bind,
            vertices,
            indices,
            instances,
            batches,
            groups,
            staging,
        }
    }
    pub fn update_lods(&mut self, queue: &wgpu::Queue, camera: [f32; 3], scale: f32) {
        // Group the small set of placements on the CPU; all tree geometry and
        // leaf billboarding stay GPU-instanced. Avoid submitting all 3 LODs.
        for group in &self.groups {
            let mut counts = [0u32; 3];
            for &p in &group.positions {
                let dx = p[0] - camera[0];
                let dz = p[2] - camera[2];
                let dist = (dx * dx + dz * dz).sqrt();
                let selection = crate::tree_lod::select(dist, group.height, scale);
                let choices = [
                    (
                        selection.primary,
                        if selection.secondary.is_some() {
                            [selection.fraction, 1.]
                        } else {
                            [0., 0.]
                        },
                    ),
                    (
                        selection.secondary.unwrap_or(selection.primary),
                        [selection.fraction, 2.],
                    ),
                ];
                for (slot, (level, info)) in choices.into_iter().enumerate() {
                    if slot == 1 && selection.secondary.is_none() {
                        break;
                    }
                    let batch = &self.batches[group.first_batch + level];
                    let index = (batch.instances.start + counts[level]) as usize;
                    self.staging[index] = Instance { position: p, info };
                    counts[level] += 1;
                }
            }
            for (i, count) in counts.into_iter().enumerate() {
                let b = &mut self.batches[group.first_batch + i];
                b.instances.end = b.instances.start + count;
            }
        }
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.staging));
    }
    pub fn draw<'a>(&'a self, p: &mut wgpu::RenderPass<'a>, reflected: bool) {
        p.set_pipeline(&self.pipeline);
        p.set_bind_group(
            0,
            if reflected {
                &self.reflected_bind
            } else {
                &self.bind
            },
            &[],
        );
        p.set_vertex_buffer(0, self.vertices.slice(..));
        p.set_vertex_buffer(1, self.instances.slice(..));
        p.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for b in &self.batches {
            p.draw_indexed(b.indices.clone(), 0, b.instances.clone());
        }
    }
}
