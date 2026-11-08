//! Per-instance GPU draws for populated actors.
//!
//! Every live actor owns one [`model_draw_gpu::Draw`]; immutable texture
//! allocations and receivers are shared across the whole population, so twenty
//! villagers reusing one rig cost one set of textures and one receiver target.

use crate::{
    model_asset::{ModelAsset, PosedMesh},
    model_draw_gpu, model_texture_gpu,
};
use std::{collections::HashMap, sync::Arc};

pub struct Renderer {
    sampler: wgpu::Sampler,
    receivers: model_draw_gpu::ReceiverCache,
    textures: HashMap<String, (Arc<wgpu::Texture>, bool)>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Populator model sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            sampler,
            receivers: model_draw_gpu::ReceiverCache::default(),
            textures: HashMap::new(),
        }
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Builds one actor's draw with the host's own reverse-Z depth convention
    /// (`Depth32Float` + `Greater`, as the Walker add-on uses).
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        asset: &ModelAsset,
        poses: &[PosedMesh],
        frame: model_draw_gpu::Frame,
        mut read: impl FnMut(&str) -> Result<Vec<Vec<u8>>, String>,
    ) -> Result<model_draw_gpu::Draw, String> {
        let Self {
            sampler,
            receivers,
            textures,
        } = self;
        model_draw_gpu::Draw::with_textures(
            device,
            queue,
            format,
            Some((
                wgpu::TextureFormat::Depth32Float,
                wgpu::CompareFunction::Greater,
            )),
            true,
            sampler,
            asset,
            poses,
            frame,
            receivers,
            |path| {
                if let Some(cached) = textures.get(path) {
                    return Ok(cached.clone());
                }
                let candidates = read(path)?;
                let mut failures = Vec::new();
                let mut uploaded = None;
                for bytes in candidates {
                    match model_texture_gpu::upload_with_alpha(
                        device,
                        queue,
                        &bytes,
                        model_texture_gpu::UploadMode::Auto,
                    ) {
                        Ok((texture, alpha)) => {
                            uploaded = Some((texture, alpha));
                            break;
                        }
                        Err(error) => failures.push(error),
                    }
                }
                let (texture, alpha) = uploaded.ok_or_else(|| {
                    if failures.is_empty() {
                        format!("no archive contained texture {path}")
                    } else {
                        format!("{path}: {}", failures.join("; "))
                    }
                })?;
                let entry = (Arc::new(texture), alpha);
                textures.insert(path.to_owned(), entry.clone());
                Ok(entry)
            },
        )
    }

    pub fn forget_textures(&mut self) {
        self.textures.clear();
    }
}
