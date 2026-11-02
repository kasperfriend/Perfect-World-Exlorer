//! Retained ordinary model GPU draw/update ownership. One Draw is a single
//! render-pass instance: reflection/refraction need distinct uniforms so queue
//! writes cannot overwrite constants for a previously encoded pass.
use crate::{
    model_asset::{ModelAsset, PosedMesh},
    model_draw, model_material,
    model_receiver_gpu::{self as raster, Params},
    model_texture_gpu,
};

#[derive(Clone, Copy)]
pub struct Frame {
    pub blend_enabled: bool,
    pub matrix: [[f32; 4]; 4],
    pub lighting: model_material::Lighting,
    pub direction: [f32; 3],
    pub point_position: [f32; 3],
    pub attenuation: [f32; 3],
    pub eye_direction: [f32; 3],
    pub directional_specular: [f32; 4],
    pub specular_variant: bool,
    pub vertex_fog: f32,
    pub fog_color: [f32; 4],
    pub fog_range: [f32; 4],
    pub clip_plane: [f32; 4],
    pub reverse_front: bool,
    /// Zero disables alpha testing; 1..8 are the source D3DCMPFUNC values.
    pub alpha_compare: u32,
    pub alpha_reference: f32,
}
impl Frame {
    fn params(self, material: &crate::skin::Material) -> Result<Params, String> {
        if self.alpha_compare > 8 || !self.vertex_fog.is_finite() {
            return Err("invalid model frame alpha comparison/vertex fog".into());
        }
        let c = material.light_constants(self.lighting);
        let vector = |v: [f32; 3]| [v[0], v[1], v[2], 0.];
        let p = Params {
            matrix: self.matrix,
            ambient: c.ambient,
            diffuse: c.directional_diffuse,
            point_ambient: c.point_ambient,
            point_diffuse: c.point_diffuse,
            direction: vector(self.direction),
            point_position: vector(self.point_position),
            attenuation: vector(self.attenuation),
            eye_direction: vector(self.eye_direction),
            specular: model_material::bind_specular(material.specular, self.directional_specular),
            lighting: [
                material.power,
                u32::from(self.lighting.point_enabled) as f32,
                u32::from(self.specular_variant) as f32,
                0.,
            ],
            fog_color: self.fog_color,
            fog_range: self.fog_range,
            clip_plane: self.clip_plane,
            raster: [
                u32::from(material.two_sided) as f32,
                u32::from(self.reverse_front) as f32,
                self.alpha_compare as f32,
                self.alpha_reference,
            ],
        };
        if bytemuck::cast_slice::<Params, f32>(std::slice::from_ref(&p))
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite model frame constants".into());
        }
        Ok(p)
    }
}
struct Mesh {
    key: (usize, usize),
    paths: [String; 2],
    indices: Vec<u16>,
    vertices: usize,
    alpha_texture: bool,
    mesh: raster::Mesh,
    binding: raster::Binding,
}
pub struct Draw {
    blend_enabled: bool,
    receiver: raster::Receiver,
    meshes: Vec<Mesh>,
    // Keep authored textures alive and share repeated per-skin texture slots.
    textures: Vec<wgpu::Texture>,
}
impl Draw {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        depth: Option<(wgpu::TextureFormat, wgpu::CompareFunction)>,
        depth_write: bool,
        sampler: &wgpu::Sampler,
        asset: &ModelAsset,
        poses: &[PosedMesh],
        frame: Frame,
        mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Self, String> {
        let prepared = model_draw::prepare(asset, poses)?;
        // Validate every frame/material before allocating or uploading anything.
        let params: Vec<_> = prepared
            .iter()
            .map(|m| frame.params(m.material))
            .collect::<Result<_, _>>()?;
        let receiver = if frame.blend_enabled {
            raster::Receiver::new(device, format, depth, depth_write)
        } else {
            raster::Receiver::new_opaque(device, format, depth, depth_write)
        }?;
        let mut textures: Vec<wgpu::Texture> = Vec::new();
        let mut cache: std::collections::HashMap<[String; 2], (usize, bool)> =
            std::collections::HashMap::new();
        let mut meshes = Vec::new();
        for (m, p) in prepared.iter().zip(&params) {
            let (texture, alpha_texture) = if let Some(&index) = cache.get(&m.texture_paths) {
                index
            } else {
                let mut failures = Vec::new();
                let mut loaded = None;
                for path in &m.texture_paths {
                    let result = read(path).and_then(|bytes| {
                        let alpha = crate::model_dds::Dds::parse(&bytes)?.format.alpha_texture();
                        model_texture_gpu::upload(
                            device,
                            queue,
                            &bytes,
                            model_texture_gpu::UploadMode::Auto,
                        )
                        .map(|texture| (texture, alpha))
                    });
                    match result {
                        Ok(t) => {
                            loaded = Some(t);
                            break;
                        }
                        Err(e) => failures.push(format!("{path}: {e}")),
                    }
                }
                let (texture, alpha) = loaded.ok_or_else(|| failures.join("; "))?;
                let index = textures.len();
                textures.push(texture);
                cache.insert(m.texture_paths.clone(), (index, alpha));
                (index, alpha)
            };
            let view = textures[texture].create_view(&Default::default());
            let vertices = vertices(m, frame.vertex_fog);
            meshes.push(Mesh {
                key: (m.skin, m.mesh),
                paths: m.texture_paths.clone(),
                indices: m.indices.to_vec(),
                vertices: vertices.len(),
                alpha_texture,
                mesh: raster::Mesh::new(device, &vertices, m.indices)?,
                binding: receiver.binding(device, &view, sampler, p)?,
            });
        }
        Ok(Self {
            blend_enabled: frame.blend_enabled,
            receiver,
            meshes,
            textures,
        })
    }
    /// All validation precedes queue writes: an invalid last mesh must not
    /// leave earlier meshes displaying half of a new animation frame.
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        asset: &ModelAsset,
        poses: &[PosedMesh],
        frame: Frame,
    ) -> Result<(), String> {
        if frame.blend_enabled != self.blend_enabled {
            return Err("model pass blending changed; rebuild its pipeline".into());
        }
        let prepared = model_draw::prepare(asset, poses)?;
        if prepared.len() != self.meshes.len() {
            return Err("model draw mesh count changed".into());
        }
        let mut updates = Vec::with_capacity(prepared.len());
        for (m, gpu) in prepared.iter().zip(&self.meshes) {
            if (m.skin, m.mesh) != gpu.key
                || m.texture_paths != gpu.paths
                || m.indices != gpu.indices
                || m.vertices.len() != gpu.vertices
            {
                return Err("model draw topology/texture assignment changed".into());
            }
            updates.push((vertices(m, frame.vertex_fog), frame.params(m.material)?));
        }
        for (gpu, (vertices, params)) in self.meshes.iter().zip(updates) {
            gpu.mesh.update(queue, &vertices)?;
            gpu.binding.update(queue, &params)?;
        }
        Ok(())
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for m in &self.meshes {
            self.receiver.draw(pass, &m.mesh, &m.binding);
        }
    }
    pub fn alpha_texture(&self, mesh: usize) -> Option<bool> {
        self.meshes.get(mesh).map(|m| m.alpha_texture)
    }
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
}
fn vertices(m: &model_draw::PreparedMesh<'_>, fog: f32) -> Vec<raster::Vertex> {
    m.vertices
        .iter()
        .map(|v| raster::Vertex {
            position: v.position,
            normal: v.normal,
            uv: v.uv,
            fog,
        })
        .collect()
}
