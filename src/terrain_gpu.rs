//! GPU terrain: original material layers and masks, plus a persistent world LOD mesh.
use crate::{gpu::Vertex, Terrain};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
const MASK_PAGE: usize = 4096;
const TEX: usize = 512;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Area {
    start: u32,
    count: u32,
    _pad: [u32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Layer {
    tex: u32,
    axis: u32,
    mask: u32,
    _pad: u32,
    uv: [f32; 2],
    _pad2: [f32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Settings {
    params: [f32; 4],
    areas: [u32; 4],
    light: [u32; 4],
}
pub struct TerrainGpu {
    pipeline: wgpu::RenderPipeline,
    far_pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    reflect_bind: wgpu::BindGroup,
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    pub grass_light_layout: wgpu::BindGroupLayout,
    pub grass_light_bind: wgpu::BindGroup,
}
const SHADER: &str = r#"
struct Camera { matrix:mat4x4<f32>, direction:vec4<f32>, clip:vec4<f32>, bounds:vec4<f32>, tree_bounds:vec4<f32>, sun:vec4<f32>, light:vec4<f32>, region_ambient:vec4<f32>, region_diffuse:vec4<f32>, region_direction:vec4<f32>, grass_eye:vec4<f32>, grass_wind:vec4<f32>, grass_history:array<vec4<f32>,80>, fog_range:vec4<f32>, fog_color:vec4<f32> };
struct Settings { params:vec4<f32>, areas:vec4<u32>, light:vec4<u32> };
struct Area { start:u32,count:u32,pad:vec2<u32> };
struct Layer { tex:u32,axis:u32,mask:u32,pad:u32,uv:vec2<f32>,pad2:vec2<f32> };
@group(0) @binding(0) var<uniform> camera:Camera;
@group(0) @binding(1) var<uniform> settings:Settings;
@group(0) @binding(2) var terrain:texture_2d_array<f32>;
@group(0) @binding(3) var terrain_sampler:sampler;
@group(0) @binding(8) var terrain_small:texture_2d_array<f32>;
@group(0) @binding(9) var terrain_full:texture_2d_array<f32>;
@group(0) @binding(10) var lightmaps:texture_2d_array<f32>;
@group(0) @binding(4) var masks:texture_2d_array<f32>;
@group(0) @binding(5) var mask_sampler:sampler;
@group(0) @binding(6) var<storage,read> area_data:array<Area>;
@group(0) @binding(7) var<storage,read> layers:array<Layer>;
struct In {@location(0) pos:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) color:vec4<f32>,@location(4) kind:f32};
struct FarIn {@location(0) pos:vec3<f32>, @location(1) normal:vec4<f32>};
struct Out {@builtin(position) pos:vec4<f32>,@location(0) world:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) @interpolate(flat) distant:f32,@location(3) refraction:vec2<f32>};
// terrain_refract_vs: project above-plane vertices down, but use the original
// vertex for UV generation and eye direction. oD0 is independent depth darkness.
fn terrain_vertex(world:vec3<f32>,normal:vec3<f32>,distant:f32)->Out {
 var o:Out;var projected=world;o.refraction=vec2<f32>(1.);
 if camera.clip.y<0. {
  projected.y=min(world.y,camera.clip.x);let depth=camera.clip.x-projected.y;
  let delta=camera.grass_eye.xyz-world;
  var f=select(1.,0.,depth>0.);
  if abs(delta.y)>0.000001 {f=exp2(-0.075*depth*length(delta)/delta.y);}
  o.refraction=vec2<f32>(f,exp2(-0.15*depth));
 }
 o.pos=camera.matrix*vec4<f32>(projected,1.);o.world=world;o.normal=normal;o.distant=distant;return o;
}
@vertex fn vs(i:In)->Out {return terrain_vertex(i.pos,i.normal,i.kind);}
@vertex fn vs_far(i:FarIn)->Out {return terrain_vertex(i.pos,i.normal.xyz,1.);}
// Approximate the original fixed-function/PS 1.1 arithmetic in encoded RGB.
// D3D8 filters authored texture bytes in encoded space. The texture arrays
// use UNORM views; return linear RGB to the viewer's sRGB render target.
fn to_linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((max(c,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));}
fn fogged(linear:vec3<f32>, reciprocal_w:f32)->vec3<f32> {
 if camera.fog_range.z<0.5 {return linear;}
 let view_z=1./max(reciprocal_w,0.0000001);
 let span=camera.fog_range.y-camera.fog_range.x;
 let f=select(clamp((view_z-camera.fog_range.x)/select(span,0.001,abs(span)<0.001),0.,1.),select(0.,1.,view_z>=camera.fog_range.y),abs(span)<0.001);
 let c=max(linear,vec3<f32>(0.));let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(encoded,camera.fog_color.rgb,f);
 return select(v/12.92,pow((max(v,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),v>vec3<f32>(0.04045));
}
fn terrain_pass_fog(linear:vec3<f32>,reciprocal_w:f32,transmittance:f32)->vec3<f32> {
 if camera.clip.y>=0. || camera.fog_range.z<0.5 {return fogged(linear,reciprocal_w);}
 let c=max(linear,vec3<f32>(0.));let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(camera.fog_color.rgb,encoded,clamp(transmittance,0.,1.));return to_linear(v);
}
@fragment fn fs(i:Out)->@location(0) vec4<f32> {
 if camera.clip.y>0. && i.world.y<camera.clip.x {discard;}
 if i.distant>0.5 && i.world.x>=camera.bounds.x && i.world.x<camera.bounds.y && i.world.z>=camera.bounds.z && i.world.z<camera.bounds.w {discard;}
 let gx=clamp(i.world.x/settings.params.z+settings.params.x*0.5,0.,settings.params.x-1.);
 let gy=clamp(settings.params.y*0.5-i.world.z/settings.params.z,0.,settings.params.y-1.);
 let coord=vec2<f32>(gx,gy)/settings.params.w;
 let wx=dpdx(i.world);let wy=dpdy(i.world);
 let ax=u32(floor(coord.x));let ay=u32(floor(coord.y));let a=area_data[ay*settings.areas.x+ax];
 var color=vec3<f32>(0.);
 var baked=vec3<f32>(1.);
 if camera.sun.w>=0. {let id=ay*settings.areas.x+ax;let slot=id%settings.light.z;let page=id/settings.light.z;let side=f32(settings.light.x);let xy=vec2<f32>(f32(slot%settings.light.y),f32(slot/settings.light.y))*side;let loc=clamp(fract(coord)*side,vec2<f32>(0.5),vec2<f32>(side-0.5));let lmuv=(xy+loc)/vec2<f32>(textureDimensions(lightmaps));let day=textureSampleLevel(lightmaps,mask_sampler,lmuv,i32(page),0.).rgb;let night=textureSampleLevel(lightmaps,mask_sampler,lmuv,i32(page+settings.light.w),0.).rgb;baked=mix(night,day,clamp(camera.sun.w,0.,1.))*2.;}
 for(var k=0u;k<a.count;k=k+1u){let l=layers[a.start+k];var v=vec2<f32>(i.world.x,-i.world.z);if l.axis==0u {v=vec2<f32>(i.world.z,-i.world.y);}else if l.axis==2u {v=vec2<f32>(i.world.x,-i.world.y);}
 var dx=vec2<f32>(wx.x,-wx.z);var dy=vec2<f32>(wy.x,-wy.z);
 if l.axis==0u {dx=vec2<f32>(wx.z,-wx.y);dy=vec2<f32>(wy.z,-wy.y);}else if l.axis==2u {dx=vec2<f32>(wx.x,-wx.y);dy=vec2<f32>(wy.x,-wy.y);}
 dx*=l.uv;dy*=l.uv;let kind=l.tex>>30u;let index=i32(l.tex & 0x3fffffffu);
 // Pass the true screen-space gradient, not a scalar max-footprint LOD.
 // The sampler can then preserve detail along the minor axis at grazing angles.
 var tex:vec3<f32>;if kind==2u {tex=textureSampleGrad(terrain_full,terrain_sampler,v*l.uv,index,dx,dy).rgb;}else if kind==1u {tex=textureSampleGrad(terrain,terrain_sampler,v*l.uv,index,dx,dy).rgb;}else {tex=textureSampleGrad(terrain_small,terrain_sampler,v*l.uv,index,dx,dy).rgb;}
 var alpha=1.;if k>0u {let slot=l.mask%settings.areas.w;let page=l.mask/settings.areas.w;let side=f32(settings.areas.y);let xy=vec2<f32>(f32(slot%settings.areas.z),f32(slot/settings.areas.z))*side;let local=clamp(fract(coord)*side,vec2<f32>(0.5),vec2<f32>(side-0.5));alpha=textureSampleLevel(masks,mask_sampler,(xy+local)/4096.,i32(page),0.).r;}
 if camera.sun.w<0. {tex=to_linear(tex);}
 color=mix(color,tex,alpha);}
 if camera.sun.w>=0. {return vec4<f32>(terrain_pass_fog(to_linear(clamp(color*baked*i.refraction.y,vec3<f32>(0.),vec3<f32>(1.))),i.pos.w,i.refraction.x),1.);}
 let light=camera.light.x+camera.light.y*max(dot(normalize(i.normal),normalize(camera.sun.xyz)),0.);
 return vec4<f32>(terrain_pass_fog(color*light*camera.light.zww,i.pos.w,i.refraction.x),1.);
}
"#;
impl TerrainGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        t: &Terrain,
        normal: &wgpu::Buffer,
        reflection: &wgpu::Buffer,
    ) -> Result<Self, String> {
        let m = &t.materials;
        if m.textures.is_empty()
            || m.textures.len() > device.limits().max_texture_array_layers as usize
        {
            return Err("GPU terrain texture layer limit exceeded".into());
        }
        let masks_count = m
            .areas
            .iter()
            .map(|a| a.layers.len().saturating_sub(1))
            .sum::<usize>();
        // Native client masks are 256². A smaller diagnostic setting is useful
        // on software renderers with severely limited memory.
        let mask_side = if std::env::var_os("PW_SMALL_MASK_ATLAS").is_some() {
            128
        } else {
            256
        };
        let per_row = MASK_PAGE / mask_side;
        let per_page = per_row * per_row;
        let pages = ((masks_count + per_page - 1) / per_page).max(1);
        if pages > device.limits().max_texture_array_layers as usize {
            return Err("GPU terrain mask layer limit exceeded".into());
        }
        // Avoid upscaling every 256px source into a 512px array: preserve
        // client resolutions in two arrays and save substantial GPU memory.
        let count_small = m
            .textures
            .iter()
            .filter(|t| t.w <= 256 && t.h <= 256)
            .count();
        let count_large = m
            .textures
            .iter()
            .filter(|t| (t.w > 256 || t.h > 256) && t.w <= 512 && t.h <= 512)
            .count();
        let count_full = m.textures.len() - count_small - count_large;
        let create_array = |side: usize, count: usize| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("original terrain texture array"),
                size: wgpu::Extent3d {
                    width: side as u32,
                    height: side as u32,
                    depth_or_array_layers: count.max(1) as u32,
                },
                mip_level_count: side.ilog2() + 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // D3D8 filters texture bytes in their authored encoded space.
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let small_tex = create_array(256, count_small);
        let tex = create_array(TEX, count_large);
        let full_tex = create_array(1024, count_full);
        let mut tex_map = Vec::with_capacity(m.textures.len());
        let mut small_n = 0u32;
        let mut large_n = 0u32;
        let mut full_n = 0u32;
        for (image_number, img) in m.textures.iter().enumerate() {
            let (image, base_side, index, kind) = if img.w > 512 || img.h > 512 {
                let i = full_n;
                full_n += 1;
                (&full_tex, 1024, i, 2u32)
            } else if img.w > 256 || img.h > 256 {
                let i = large_n;
                large_n += 1;
                (&tex, TEX, i, 1u32)
            } else {
                let i = small_n;
                small_n += 1;
                (&small_tex, 256, i, 0u32)
            };
            tex_map.push(index | (kind << 30));
            let mut level = vec![0u8; base_side * base_side * 4];
            for y in 0..base_side {
                let sy = y * img.h / base_side;
                for x in 0..base_side {
                    let sx = x * img.w / base_side;
                    let si = (sy * img.w + sx) * 3;
                    let di = (y * base_side + x) * 4;
                    level[di..di + 3].copy_from_slice(&img.pixels[si..si + 3]);
                    level[di + 3] = 255;
                }
            }
            let mut side = base_side;
            for mip in 0..=base_side.ilog2() {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: image,
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: index,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &level,
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
                if side > 1 {
                    let next = side / 2;
                    let mut reduced = vec![0u8; next * next * 4];
                    for y in 0..next {
                        for x in 0..next {
                            for k in 0..4 {
                                let a = level[((2 * y) * side + 2 * x) * 4 + k] as u32;
                                let b = level[((2 * y) * side + 2 * x + 1) * 4 + k] as u32;
                                let c = level[((2 * y + 1) * side + 2 * x) * 4 + k] as u32;
                                let d = level[((2 * y + 1) * side + 2 * x + 1) * 4 + k] as u32;
                                reduced[(y * next + x) * 4 + k] = ((a + b + c + d + 2) / 4) as u8;
                            }
                        }
                    }
                    level = reduced;
                    side = next;
                }
            }
            // wgpu keeps write_texture staging alive until submission. Bound
            // peak memory even when maximum-quality terrain uses 1024px images.
            if image_number % 8 == 7 || image_number + 1 == m.textures.len() {
                queue.submit(std::iter::empty());
                device.poll(wgpu::Maintain::Wait);
            }
        }
        let masktex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain layer masks"),
            size: wgpu::Extent3d {
                width: MASK_PAGE as u32,
                height: MASK_PAGE as u32,
                depth_or_array_layers: pages as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut areas = Vec::new();
        let mut layers = Vec::new();
        let mut page = vec![0u8; MASK_PAGE * MASK_PAGE];
        let mut layer_id = 0usize;
        let mut page_id = 0usize;
        let upload_page = |page: &[u8], id: usize| {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &masktex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: id as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                page,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(MASK_PAGE as u32),
                    rows_per_image: Some(MASK_PAGE as u32),
                },
                wgpu::Extent3d {
                    width: MASK_PAGE as u32,
                    height: MASK_PAGE as u32,
                    depth_or_array_layers: 1,
                },
            )
        };
        for a in &m.areas {
            areas.push(Area {
                start: layers.len() as u32,
                count: a.layers.len() as u32,
                _pad: [0; 2],
            });
            for (j, l) in a.layers.iter().enumerate() {
                let id = if j == 0 {
                    u32::MAX
                } else {
                    if layer_id / per_page != page_id {
                        upload_page(&page, page_id);
                        // Prevent queued 16 MiB mask pages from accumulating
                        // alongside decoded terrain images and lightmaps.
                        if page_id % 2 == 1 {
                            queue.submit(std::iter::empty());
                            device.poll(wgpu::Maintain::Wait);
                        }
                        page.fill(0);
                        page_id += 1;
                    }
                    let slot = layer_id % per_page;
                    let ox = slot % per_row * mask_side;
                    let oy = slot / per_row * mask_side;
                    for y in 0..mask_side {
                        for x in 0..mask_side {
                            let sx = x * l.mask_size / mask_side;
                            let sy = y * l.mask_size / mask_side;
                            page[(oy + y) * MASK_PAGE + ox + x] = l.mask[sy * l.mask_size + sx];
                        }
                    }
                    let id = layer_id as u32;
                    layer_id += 1;
                    id
                };
                layers.push(Layer {
                    tex: tex_map[l.texture],
                    axis: l.axis as u32,
                    mask: id,
                    _pad: 0,
                    uv: [l.u, l.v],
                    _pad2: [0.; 2],
                });
            }
        }
        upload_page(&page, page_id);
        queue.submit(std::iter::empty());
        device.poll(wgpu::Maintain::Wait);
        let (lighttex, light_side, light_per_row, light_per_page, light_pages, availability) =
            load_lightmaps(device, queue, t, mask_side)?;
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain world settings"),
            contents: bytemuck::bytes_of(&Settings {
                params: [
                    m.width as f32,
                    m.height as f32,
                    m.spacing,
                    m.mask_grid as f32,
                ],
                areas: [
                    m.areas_x as u32,
                    mask_side as u32,
                    per_row as u32,
                    per_page as u32,
                ],
                light: [
                    light_side as u32,
                    light_per_row as u32,
                    light_per_page as u32,
                    light_pages as u32,
                ],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let area_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain areas"),
            contents: bytemuck::cast_slice(&areas),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let layer_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain layers"),
            contents: bytemuck::cast_slice(&layers),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let tv = tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let small_view = small_tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let full_view = full_tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let light_view = lighttex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let mv = masktex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        let mask_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // Share the exact terrain day/night lightmap texture with grass.  The
        // client's grassrender.txt modulates grass by these two maps, not a
        // separate procedural directional light.
        let grass_light_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("grass terrain lightmap layout"),
                entries: &[
                    buf(
                        0,
                        wgpu::ShaderStages::FRAGMENT,
                        wgpu::BufferBindingType::Uniform,
                    ),
                    texture(1),
                    sam(2),
                    buf(
                        3,
                        wgpu::ShaderStages::FRAGMENT,
                        wgpu::BufferBindingType::Storage { read_only: true },
                    ),
                ],
            });
        let availability = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("authored terrain lightmap availability for grass"),
            contents: bytemuck::cast_slice(&availability),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let grass_light_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grass terrain day/night lightmaps"),
            layout: &grass_light_layout,
            entries: &[
                entry(0, settings.as_entire_binding()),
                entry(1, wgpu::BindingResource::TextureView(&light_view)),
                entry(2, wgpu::BindingResource::Sampler(&mask_sampler)),
                entry(3, availability.as_entire_binding()),
            ],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain material layout"),
            entries: &[
                buf(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BufferBindingType::Uniform,
                ),
                buf(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BufferBindingType::Uniform,
                ),
                texture(2),
                texture(8),
                texture(9),
                texture(10),
                sam(3),
                texture(4),
                sam(5),
                buf(
                    6,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
                buf(
                    7,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
            ],
        });
        let bind = |camera: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain material bindings"),
                layout: &layout,
                entries: &[
                    entry(0, camera.as_entire_binding()),
                    entry(1, settings.as_entire_binding()),
                    entry(2, wgpu::BindingResource::TextureView(&tv)),
                    entry(8, wgpu::BindingResource::TextureView(&small_view)),
                    entry(9, wgpu::BindingResource::TextureView(&full_view)),
                    entry(10, wgpu::BindingResource::TextureView(&light_view)),
                    entry(3, wgpu::BindingResource::Sampler(&sampler)),
                    entry(4, wgpu::BindingResource::TextureView(&mv)),
                    entry(5, wgpu::BindingResource::Sampler(&mask_sampler)),
                    entry(6, area_buffer.as_entire_binding()),
                    entry(7, layer_buffer.as_entire_binding()),
                ],
            })
        };
        let normal_bind = bind(normal);
        let reflect_bind = bind(reflection);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layered terrain GPU shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain pipeline"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        // Variant selection and pass state come from the recovered native
        // switch (`src/terrain_shading.rs`, 0x7c1450) instead of being restated
        // here. `Caps::default()` is the stock 1.4.2 state: a device with a pixel
        // pipeline and neither GDebugKey flag set, i.e. `Variant::Batch3Layer`.
        let variant = crate::terrain_shading::select(crate::terrain_shading::Caps::default())
            .map_err(|e| format!("terrain variant {e:?} is unreachable in the client"))?;
        debug_assert_eq!(variant, crate::terrain_shading::Variant::Batch3Layer);
        let state = crate::terrain_shading::pass_state(variant);
        // 0x7cd789 pushes D3DCULL_NONE (1); the state type keeps the restored
        // value explicit so the mapping cannot drift silently.
        debug_assert!(matches!(state.cull, crate::terrain_shading::Cull::None));
        let primitive = wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        };
        let targets = [Some(wgpu::ColorTargetState {
            format,
            // ALPHABLENDENABLE is never called for terrain (0x77a290 has no
            // caller in 0x7c1400-0x7d3fff), so terrain is opaque.
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let depth = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: state.z_write,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: Default::default(),
            bias: Default::default(),
        };
        const ATTR: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4,4=>Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("layered terrain"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTR,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &targets,
            }),
            primitive,
            depth_stencil: Some(depth),
            multisample: Default::default(),
            multiview: None,
        });
        const FAR_ATTR: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Snorm8x4];
        let far_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("layered terrain"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_far",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FarVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &FAR_ATTR,
                }],
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
        let (vertices, indices) = world_mesh(t, 4);
        let count = indices.len() as u32;
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("full-world terrain LOD vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("full-world terrain LOD indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        eprintln!(
            "Uploaded world terrain: {} vertices, {} triangles, {} textures, {} masks",
            vertices.len(),
            indices.len() / 3,
            m.textures.len(),
            masks_count
        );
        Ok(Self {
            pipeline,
            far_pipeline,
            bind: normal_bind,
            reflect_bind,
            vertex,
            index,
            count,
            grass_light_layout,
            grass_light_bind,
        })
    }
    pub fn edge_mesh(&self) -> (&wgpu::Buffer, &wgpu::Buffer, u32) {
        (&self.vertex, &self.index, self.count)
    }
    pub fn draw<'a>(
        &'a self,
        p: &mut wgpu::RenderPass<'a>,
        reflected: bool,
        near_vertex: &'a wgpu::Buffer,
        near_index: &'a wgpu::Buffer,
        near_end: u32,
    ) {
        p.set_pipeline(&self.far_pipeline);
        p.set_bind_group(
            0,
            if reflected {
                &self.reflect_bind
            } else {
                &self.bind
            },
            &[],
        );
        p.set_vertex_buffer(0, self.vertex.slice(..));
        p.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        p.draw_indexed(0..self.count, 0, 0..1);
        p.set_pipeline(&self.pipeline);
        p.set_vertex_buffer(0, near_vertex.slice(..));
        p.set_index_buffer(near_index.slice(..), wgpu::IndexFormat::Uint32);
        p.draw_indexed(0..near_end, 0, 0..1);
    }
}
fn buf(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BufferBindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn texture(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}
fn sam(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}
// T2LM v2: 16 independent DDS lightmaps per 512-grid subterrain.
// t2lm/t2lm1 are day/night color maps with independently indexed DDS records.
fn load_lightmaps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    t: &Terrain,
    side: usize,
) -> Result<(wgpu::Texture, usize, usize, usize, usize, Vec<u32>), String> {
    let m = &t.materials;
    if std::env::var_os("PW_TEST_NO_LIGHTMAPS").is_some() {
        let per_row = 16usize;
        let per_page = 256usize;
        let pages = ((m.areas.len() + per_page - 1) / per_page).max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("diagnostic neutral lightmaps"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: (pages * 2) as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        return Ok((
            texture,
            256,
            per_row,
            per_page,
            pages,
            vec![0; m.areas.len().max(1)],
        ));
    }
    let bc = device
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
    let side = if bc { 256 } else { side };
    let per_row = 4096 / side;
    let per_page = per_row * per_row;
    let pages = ((m.areas.len() + per_page - 1) / per_page).max(1);
    if 2 * pages > device.limits().max_texture_array_layers as usize {
        return Err("terrain lightmap page limit exceeded".into());
    }
    // Client DDS payloads are DXT1: use native BC1 rather than quadrupling
    // memory by decoding the 2,816 images to RGBA on compatible GPUs.
    let format = if bc {
        wgpu::TextureFormat::Bc1RgbaUnorm
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("client terrain day/night lightmaps"),
        size: wgpu::Extent3d {
            width: 4096,
            height: 4096,
            depth_or_array_layers: (2 * pages) as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // Upload each original tile directly into its atlas slot. Building a full
    // CPU copy of all atlas pages duplicates hundreds of MiB during startup.
    // Queue uploads are flushed in bounded groups below; the GPU atlas layout
    // and shader sampling remain exactly the same.
    let layout =
        crate::terrain_lightmaps::Layout::new(m.areas_x, m.areas.len(), t.subterrain_names.len())?;
    // Per-area binding state. A missing day record leaves an area unbound
    // (client `ApplyLightMap` returns without binding), which is not the same
    // as the day/night fallback the night slot uses.
    let mut availability = vec![crate::terrain_lightmaps::Availability::Unbound; m.areas.len()];
    let mut unbound = Vec::new();
    for (si, name) in t.subterrain_names.iter().enumerate() {
        let base = t.element.join("maps/world");
        let pair = match crate::terrain_lightmaps::Pair::read_optional(
            &base.join(format!("{name}.t2lm")),
            &base.join(format!("{name}.t2lm1")),
        )? {
            Some(pair) => pair,
            None => {
                for j in 0..crate::terrain_lightmaps::TILES {
                    unbound.push(layout.area(si, j).unwrap());
                }
                continue;
            }
        };
        for (which, ext) in ["t2lm", "t2lm1"].iter().enumerate() {
            let path = t.element.join("maps/world").join(format!("{name}.{ext}"));
            let tiles = crate::terrain_lightmaps::Tiles::parse(pair.bytes(which == 1))
                .map_err(|e| format!("lightmap {}: {e}", path.display()))?;
            for j in 0..crate::terrain_lightmaps::TILES {
                let raw = tiles.tile(j).unwrap();
                let id = layout.area(si, j).unwrap();
                availability[id] = pair.availability();
                let slot = id % per_page;
                let page = id / per_page + which * pages;
                let x = (slot % per_row) * side;
                let y = (slot / per_row) * side;
                let write_tile = |data: &[u8], bytes_per_row: u32, rows_per_image: u32| {
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: x as u32,
                                y: y as u32,
                                z: page as u32,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        data,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(bytes_per_row),
                            rows_per_image: Some(rows_per_image),
                        },
                        wgpu::Extent3d {
                            width: side as u32,
                            height: side as u32,
                            depth_or_array_layers: 1,
                        },
                    );
                };
                let dds = crate::model_dds::Dds::parse(raw)
                    .map_err(|e| format!("lightmap {}: {e}", path.display()))?;
                if dds.format != crate::model_dds::Format::Dxt1
                    || dds.mips.len() != 1
                    || dds.mips[0].width != 256
                    || dds.mips[0].height != 256
                {
                    return Err(format!("Unsupported lightmap DDS in {}", path.display()));
                }
                // Both upload backends must accept exactly the same records.
                if bc {
                    write_tile(dds.mips[0].bytes, (side / 4 * 8) as u32, (side / 4) as u32);
                } else {
                    let img = image::load_from_memory_with_format(raw, image::ImageFormat::Dds)
                        .map_err(|e| format!("lightmap {}: {e}", path.display()))?
                        .to_rgba8();
                    if img.width() != 256 || img.height() != 256 {
                        return Err("unexpected lightmap dimensions".into());
                    }
                    let img = if side == 256 {
                        img
                    } else {
                        image::imageops::resize(
                            &img,
                            side as u32,
                            side as u32,
                            image::imageops::FilterType::Triangle,
                        )
                    };
                    write_tile(img.as_raw(), (side * 4) as u32, side as u32);
                }
            }
        }
        // write_texture staging belongs to wgpu until the next submission.
        // Wait periodically to cap temporary upload memory on slower GPUs.
        if si % 4 == 3 || si + 1 == t.subterrain_names.len() {
            queue.submit(std::iter::empty());
            device.poll(wgpu::Maintain::Wait);
        }
    }
    // An unbound area still owns an atlas slot, so fill it with a neutral tile
    // rather than the texture's zero-initialised contents. The shader keeps
    // sampling the slot; 0.5 grey reproduces "no lightmap bound" instead of
    // black terrain. Grass never reads these slots (availability is 0).
    if !unbound.is_empty() {
        let neutral = neutral_tile(side, bc);
        for &id in &unbound {
            for which in 0..2usize {
                let slot = id % per_page;
                let page = id / per_page + which * pages;
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: ((slot % per_row) * side) as u32,
                            y: ((slot / per_row) * side) as u32,
                            z: page as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &neutral,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(if bc {
                            (side / 4 * 8) as u32
                        } else {
                            (side * 4) as u32
                        }),
                        rows_per_image: Some(if bc { (side / 4) as u32 } else { side as u32 }),
                    },
                    wgpu::Extent3d {
                        width: side as u32,
                        height: side as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        queue.submit(std::iter::empty());
        device.poll(wgpu::Maintain::Wait);
    }
    eprintln!(
        "Loaded {} terrain lightmap areas ({} with authored night maps, {} day-only, {} unbound) at {}px ({})",
        m.areas.len(),
        availability.iter().filter(|a| a.bits() == 3).count(),
        availability.iter().filter(|a| a.bits() == 1).count(),
        unbound.len(),
        side,
        if bc { "native BC1" } else { "RGBA" }
    );
    let availability: Vec<u32> = availability.iter().map(|a| a.bits()).collect();
    Ok((texture, side, per_row, per_page, pages, availability))
}

/// Neutral stand-in for an area the client would leave unbound. BC1 blocks
/// select colour0 (`RGB565(0.5,0.5,0.5)`) with every index bit clear.
fn neutral_tile(side: usize, bc: bool) -> Vec<u8> {
    if bc {
        let mut block = [0u8; 8];
        block[..2].copy_from_slice(&0x8410u16.to_le_bytes());
        block.repeat((side / 4) * (side / 4))
    } else {
        [128, 128, 128, 255].repeat(side * side)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FarVertex {
    pos: [f32; 3],
    normal: [i8; 4],
}
fn world_mesh(t: &Terrain, step: usize) -> (Vec<FarVertex>, Vec<u32>) {
    let xs = grid_axis(t.width, step);
    let ys = grid_axis(t.height, step);
    let mut v = Vec::with_capacity(xs.len() * ys.len());
    for &y in &ys {
        for &x in &xs {
            let at = y * t.width + x;
            let n = t.normals[at];
            v.push(FarVertex {
                pos: [
                    (x as f32 - t.width as f32 * 0.5) * t.grid_spacing,
                    t.heights[at],
                    (t.height as f32 * 0.5 - y as f32) * t.grid_spacing,
                ],
                normal: [n[0], n[1], n[2], 0],
            });
        }
    }
    let mut indices = Vec::with_capacity((xs.len() - 1) * (ys.len() - 1) * 6);
    for y in 0..ys.len() - 1 {
        for x in 0..xs.len() - 1 {
            let a = (y * xs.len() + x) as u32;
            let b = a + 1;
            let c = a + xs.len() as u32;
            indices.extend_from_slice(&[a, b, c, b, c + 1, c]);
        }
    }
    (v, indices)
}

fn grid_axis(size: usize, step: usize) -> Vec<usize> {
    assert!(size > 0 && step > 0);
    let mut values: Vec<_> = (0..size).step_by(step).collect();
    if values.last().copied() != Some(size - 1) {
        values.push(size - 1);
    }
    values
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_tiles_match_the_atlas_block_layout() {
        let rgba = super::neutral_tile(256, false);
        assert_eq!(rgba.len(), 256 * 256 * 4);
        assert!(rgba.chunks(4).all(|p| p == [128, 128, 128, 255]));
        let bc1 = super::neutral_tile(256, true);
        assert_eq!(bc1.len(), 256 * 256 / 2);
        // Every 8-byte block selects colour0 = RGB565(0.5,0.5,0.5).
        assert!(bc1.chunks(8).all(|b| b == [0x10, 0x84, 0, 0, 0, 0, 0, 0]));
        assert_eq!(super::neutral_tile(16, true).len(), 16 * 16 / 2);
    }
    #[test]
    fn lod_grid_covers_exact_edges_without_duplicates() {
        assert_eq!(grid_axis(17, 8), vec![0, 8, 16]);
        assert_eq!(grid_axis(18, 8), vec![0, 8, 16, 17]);
        assert_eq!(grid_axis(1, 8), vec![0]);
    }
}
