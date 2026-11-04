//! Retained ordinary model GPU draw/update ownership. One Draw is a single
//! render-pass instance: reflection/refraction need distinct uniforms so queue
//! writes cannot overwrite constants for a previously encoded pass.
//!
//! Queue routing follows `model_queue`: opaque meshes keep blending disabled,
//! alpha meshes use the model's authored D3D8 blend factors and are submitted
//! through the retail alpha insertion order by `world_models_gpu`. Hidden meshes
//! are not submitted at all.
use crate::{
    model_asset::{ModelAsset, PosedMesh},
    model_draw, model_material, model_queue,
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
    fn params(
        self,
        material: &crate::skin::Material,
        original_color: u32,
        diffuse_alpha_only: bool,
        face_shader: Option<&crate::model_asset::FaceShader>,
    ) -> Result<Params, String> {
        if self.alpha_compare > 8 || !self.vertex_fog.is_finite() {
            return Err("invalid model frame alpha comparison/vertex fog".into());
        }
        let scaled;
        let material = if original_color != u32::MAX {
            let scale = [
                ((original_color >> 16) & 255) as f32 / 255.,
                ((original_color >> 8) & 255) as f32 / 255.,
                (original_color & 255) as f32 / 255.,
                1.,
            ];
            scaled = crate::skin::Material {
                ambient: model_material::scale_rgba(material.ambient, scale),
                diffuse: model_material::scale_rgba(material.diffuse, scale),
                specular: model_material::scale_rgba(material.specular, scale),
                emissive: model_material::scale_rgba(material.emissive, scale),
                ..material.clone()
            };
            &scaled
        } else {
            material
        };
        let c = material.light_constants(self.lighting);
        let vector = |v: [f32; 3]| [v[0], v[1], v[2], 0.];
        let argb = |color: u32| {
            [
                ((color >> 16) & 255) as f32 / 255.,
                ((color >> 8) & 255) as f32 / 255.,
                (color & 255) as f32 / 255.,
                ((color >> 24) & 255) as f32 / 255.,
            ]
        };
        let mut face_mode = 0.;
        let mut face_color = [1.; 4];
        let mut face_skin = [1.; 4];
        if let Some(crate::model_asset::FaceShader::Brow {
            color, skin_color, ..
        }) = face_shader
        {
            face_mode = 1.;
            face_color = argb(*color);
            face_skin = argb(*skin_color);
        }
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
            alpha: [if diffuse_alpha_only { 0. } else { 1. }, face_mode, 0., 0.],
            face_color,
            face_skin,
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

/// Authored D3D8 blend factor to the pipeline equivalent. The unrecovered
/// saturate modes never reach here: `model_queue::blend_factor` rejects them.
fn blend_factor(factor: model_queue::Factor) -> wgpu::BlendFactor {
    match factor {
        model_queue::Factor::Zero => wgpu::BlendFactor::Zero,
        model_queue::Factor::One => wgpu::BlendFactor::One,
        model_queue::Factor::SrcColor => wgpu::BlendFactor::Src,
        model_queue::Factor::InvSrcColor => wgpu::BlendFactor::OneMinusSrc,
        model_queue::Factor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        model_queue::Factor::InvSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        model_queue::Factor::DestAlpha => wgpu::BlendFactor::DstAlpha,
        model_queue::Factor::InvDestAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        model_queue::Factor::DestColor => wgpu::BlendFactor::Dst,
        model_queue::Factor::InvDestColor => wgpu::BlendFactor::OneMinusDst,
    }
}

struct Mesh {
    key: (usize, usize),
    paths: [String; 2],
    indices: Vec<u16>,
    vertices: usize,
    alpha_texture: bool,
    face_shader: Option<crate::model_asset::FaceShader>,
    /// Initial (bind-pose) mesh AABB in model space, used by the retail alpha
    /// sort: 0x7a5460 measures the eight transformed initial-box corners.
    bounds: ([f32; 3], [f32; 3]),
    mesh: raster::Mesh,
    binding: raster::Binding,
}
pub struct Draw {
    blend_enabled: bool,
    receiver: raster::Receiver,
    alpha_receiver: Option<raster::Receiver>,
    /// Indexed by prepared-mesh position; `None` is a hidden queue mesh.
    meshes: Vec<Option<Mesh>>,
    alpha_meshes: Vec<usize>,
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
        let settings = asset.descriptor.render_settings();
        // Validate every frame/material before allocating or uploading anything.
        let params: Vec<_> = prepared
            .iter()
            .map(|m| frame.params(m.material, m.color, m.diffuse_alpha_only, m.face_shader))
            .collect::<Result<_, _>>()?;
        let mut textures: Vec<wgpu::Texture> = Vec::new();
        let mut cache: std::collections::HashMap<[String; 2], (usize, bool)> =
            std::collections::HashMap::new();
        let mut loaded: Vec<(usize, bool)> = Vec::with_capacity(prepared.len());
        for m in &prepared {
            let (texture, alpha_texture) = if let Some(&index) = cache.get(&m.texture_paths) {
                index
            } else {
                let mut failures = Vec::new();
                let mut result = None;
                for path in &m.texture_paths {
                    let attempt = read(path).and_then(|bytes| {
                        model_texture_gpu::upload_with_alpha(
                            device,
                            queue,
                            &bytes,
                            model_texture_gpu::UploadMode::Auto,
                        )
                    });
                    match attempt {
                        Ok(t) => {
                            result = Some(t);
                            break;
                        }
                        Err(e) => failures.push(format!("{path}: {e}")),
                    }
                }
                let (texture, alpha) = result.ok_or_else(|| failures.join("; "))?;
                let index = textures.len();
                textures.push(texture);
                cache.insert(m.texture_paths.clone(), (index, alpha));
                (index, alpha)
            };
            loaded.push((texture, alpha_texture));
        }
        let mut highlight_cache = std::collections::HashMap::<String, usize>::new();
        let mut highlights = Vec::with_capacity(prepared.len());
        for m in &prepared {
            let index = match m.face_shader {
                Some(crate::model_asset::FaceShader::Brow { highlight, .. }) => {
                    if highlight.is_empty() {
                        return Err("native brow shader has an empty highlight texture".into());
                    }
                    if let Some(&index) = highlight_cache.get(highlight) {
                        Some(index)
                    } else {
                        let bytes =
                            read(highlight).map_err(|error| format!("{highlight}: {error}"))?;
                        let (texture, _) = model_texture_gpu::upload_with_alpha(
                            device,
                            queue,
                            &bytes,
                            model_texture_gpu::UploadMode::Auto,
                        )
                        .map_err(|error| format!("{highlight}: {error}"))?;
                        let index = textures.len();
                        textures.push(texture);
                        highlight_cache.insert(highlight.clone(), index);
                        Some(index)
                    }
                }
                None => None,
            };
            highlights.push(index);
        }
        // Queue classification uses the recovered 1.4.2 rule. `0x7a5830`
        // reads `invisible`, `transparency` and the authored blend pair from
        // the model instance and the shader/alpha texture flags from the mesh's
        // texture record; the model instance carries a single alpha, so there
        // is no separate per-mesh transparency input to look for. The SKI
        // reader recovers only the alpha flag, so the shader flag stays false
        // instead of being inferred from the alpha channel.
        let model = model_queue::ModelState::new(
            false,
            frame.lighting.transparency,
            settings.source_blend,
            settings.destination_blend,
        )?;
        let mut states = Vec::with_capacity(prepared.len());
        for (_, alpha_texture) in &loaded {
            states.push(model.mesh_state(false, *alpha_texture)?);
        }
        let receiver = if frame.blend_enabled {
            raster::Receiver::new(device, format, depth, depth_write)
        } else {
            raster::Receiver::new_opaque(device, format, depth, depth_write)
        }?;
        let alpha_receiver = match states
            .iter()
            .find(|state| state.queue == model_queue::Queue::Alpha)
        {
            Some(state) => Some(raster::Receiver::new_blended(
                device,
                format,
                depth,
                // ALPHA_DEPTH_WRITE is false: the retail z-write state for the
                // alpha list was not recovered; see model_queue's register note.
                model_queue::ALPHA_DEPTH_WRITE,
                blend_factor(state.source),
                blend_factor(state.destination),
            )?),
            None => None,
        };
        let mut meshes = Vec::with_capacity(prepared.len());
        let mut alpha_meshes = Vec::new();
        for index in 0..prepared.len() {
            let m = &prepared[index];
            let state = states[index];
            if state.queue == model_queue::Queue::Hidden {
                meshes.push(None);
                continue;
            }
            let (texture, alpha_texture) = loaded[index];
            let bounds = initial_bounds(asset, m.skin, m.mesh)?;
            let view = textures[texture].create_view(&Default::default());
            let highlight =
                textures[highlights[index].unwrap_or(texture)].create_view(&Default::default());
            let vertices = vertices(m, frame.vertex_fog);
            let alpha = state.queue == model_queue::Queue::Alpha;
            let binding = match &alpha_receiver {
                Some(receiver) if alpha => {
                    receiver.binding(device, &view, &highlight, sampler, &params[index])
                }
                _ => receiver.binding(device, &view, &highlight, sampler, &params[index]),
            }?;
            if alpha {
                alpha_meshes.push(index);
            }
            meshes.push(Some(Mesh {
                key: (m.skin, m.mesh),
                paths: m.texture_paths.clone(),
                indices: m.indices.to_vec(),
                vertices: vertices.len(),
                alpha_texture,
                face_shader: m.face_shader.cloned(),
                bounds,
                mesh: raster::Mesh::new(device, &vertices, m.indices)?,
                binding,
            }));
        }
        Ok(Self {
            blend_enabled: frame.blend_enabled,
            receiver,
            alpha_receiver,
            meshes,
            alpha_meshes,
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
        for (index, m) in prepared.iter().enumerate() {
            let params = frame.params(m.material, m.color, m.diffuse_alpha_only, m.face_shader)?;
            if let Some(gpu) = &self.meshes[index] {
                if (m.skin, m.mesh) != gpu.key
                    || m.texture_paths != gpu.paths
                    || m.face_shader != gpu.face_shader.as_ref()
                    || m.indices != gpu.indices
                    || m.vertices.len() != gpu.vertices
                {
                    return Err("model draw topology/texture assignment changed".into());
                }
            }
            updates.push((index, vertices(m, frame.vertex_fog), params));
        }
        for (index, vertices, params) in updates {
            if let Some(gpu) = &self.meshes[index] {
                gpu.mesh.update(queue, &vertices)?;
                gpu.binding.update(queue, &params)?;
            }
        }
        Ok(())
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        self.draw_opaque(pass);
        for &mesh in &self.alpha_meshes {
            self.draw_alpha_mesh(pass, mesh);
        }
    }
    /// Non-alpha queue only. `World` submits the alpha list afterwards so the
    /// retail insertion order spans models, not just one model's meshes.
    pub fn draw_opaque<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for (index, mesh) in self.meshes.iter().enumerate() {
            let Some(mesh) = mesh else {
                continue;
            };
            // Alpha-list meshes were bound against the blended receiver, whose
            // pipeline owns a different implicit bind-group layout. Submitting
            // them here would be a layout mismatch, so `draw_alpha_mesh` owns
            // them instead.
            if self.alpha_meshes.contains(&index) {
                continue;
            }
            self.receiver.draw(pass, &mesh.mesh, &mesh.binding);
        }
    }
    pub fn draw_alpha_mesh<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, index: usize) {
        let (Some(receiver), Some(Some(mesh))) = (&self.alpha_receiver, self.meshes.get(index))
        else {
            return;
        };
        receiver.draw(pass, &mesh.mesh, &mesh.binding);
    }
    pub fn alpha_meshes(&self) -> &[usize] {
        &self.alpha_meshes
    }
    pub fn mesh_bounds(&self, index: usize) -> Option<([f32; 3], [f32; 3])> {
        self.meshes
            .get(index)
            .and_then(|m| m.as_ref())
            .map(|m| m.bounds)
    }
    pub fn alpha_texture(&self, mesh: usize) -> Option<bool> {
        self.meshes
            .get(mesh)
            .and_then(|m| m.as_ref())
            .map(|m| m.alpha_texture)
    }
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
}
fn initial_bounds(
    asset: &ModelAsset,
    skin: usize,
    mesh: usize,
) -> Result<([f32; 3], [f32; 3]), String> {
    let vertices = &asset
        .skins
        .get(skin)
        .and_then(|s| s.data.meshes.get(mesh))
        .ok_or("alpha bounds mesh index out of range")?
        .vertices;
    if vertices.is_empty() {
        return Err("alpha bounds mesh has no vertices".into());
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in vertices {
        for (axis, value) in v.position.iter().enumerate() {
            min[axis] = min[axis].min(*value);
            max[axis] = max[axis].max(*value);
        }
    }
    if min.iter().chain(max.iter()).any(|v| !v.is_finite()) {
        return Err("nonfinite initial mesh bounds".into());
    }
    Ok((min, max))
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
