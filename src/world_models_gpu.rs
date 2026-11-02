//! Ordinary world receiving-model submission in main and clipped water passes.
//! Unsupported alpha/effect overrides and runtime FX lights remain explicit gaps.
//! Never sends baked terrain/BMD/tree vertices through receiving-model lighting.
use crate::{
    model_draw_gpu::{Draw, Frame},
    scene_lights::{SceneLights, StaticGrid},
    world_models::Assets,
};
use std::{collections::HashMap, fs::File, path::Path};
struct Slot {
    draw: Option<Draw>,
    failed: bool,
    transform: Result<glam::Mat4, String>,
}
pub struct World {
    assets: Assets,
    slots: Vec<Slot>,
    read: Box<dyn FnMut(&str) -> Result<Vec<u8>, String>>,
    bytes: HashMap<String, Vec<u8>>,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    grid: StaticGrid,
}
impl World {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        assets: Assets,
        element: &Path,
    ) -> Result<Self, String> {
        let mut pack = File::open(element.join("models.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<String, crate::pck::Entry> = crate::pck::directory(&mut pack)?
            .into_iter()
            .map(|e| (crate::ecmodel::normalize(&e.name), e))
            .collect();
        Self::with_reader(device, format, assets, move |path| {
            let entry = entries
                .get(path)
                .ok_or_else(|| format!("missing model texture: {path}"))?;
            crate::pck::read_entry(&mut pack, entry)
        })
    }
    pub fn with_reader(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        mut assets: Assets,
        read: impl FnMut(&str) -> Result<Vec<u8>, String> + 'static,
    ) -> Result<Self, String> {
        if assets
            .instances
            .iter()
            .any(|i| i.asset >= assets.assets.len())
        {
            return Err("world model instance asset index out of range".into());
        }
        let seed = match std::env::var("PW_MODEL_RANDOM_SEED") {
            Ok(value) => value
                .parse::<u32>()
                .map_err(|_| "invalid PW_MODEL_RANDOM_SEED")?,
            Err(_) => 1,
        };
        let mut random = crate::model_playback::SceneRandom::new(seed);
        for instance in &mut assets.instances {
            let initial = random.initial_tick();
            if let Ok(clock) = &mut instance.playback {
                clock.tick(initial)?;
            }
        }
        let slots = assets
            .instances
            .iter()
            .map(|i| Slot {
                draw: None,
                failed: false,
                transform: crate::world_models::transform(&i.placement),
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ordinary model wrap sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            assets,
            slots,
            read: Box::new(read),
            bytes: HashMap::new(),
            sampler,
            format: format.remove_srgb_suffix(),
            grid: StaticGrid::default(),
        })
    }
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: Frame,
        center: [f32; 3],
        lights: &SceneLights,
        night: f32,
        delta_ms: u32,
    ) {
        self.grid.update(lights, center);
        // No invented runtime effect lights. Their production/lifetime is an
        // open gate; this world harness currently supplies the static list only.
        self.grid.update_dynamic(&[]);
        for (instance, slot) in self.assets.instances.iter_mut().zip(&mut self.slots) {
            if slot.failed {
                continue;
            }
            let asset = &self.assets.assets[instance.asset];
            let result = (|| -> Result<(), String> {
                let settings = asset.descriptor.render_settings();
                if frame.blend_enabled
                    || crate::model_queue::classify(
                        false,
                        frame.lighting.transparency,
                        settings.source_blend,
                        settings.destination_blend,
                        false,
                        false,
                    ) != crate::model_queue::Queue::Opaque
                {
                    return Err(
                        "world receiver only accepts verified ordinary opaque queue state".into(),
                    );
                }
                if asset
                    .skins
                    .iter()
                    .flat_map(|s| &s.data.materials)
                    .any(|m| m.specular[..3].iter().any(|v| *v != 0.))
                {
                    return Err("world harness requires actual directional specular input for nonzero specular material".into());
                }
                let transform = *slot.transform.as_ref().map_err(Clone::clone)?;
                let clock = instance.playback.as_mut().map_err(|e| e.clone())?;
                clock.tick(delta_ms)?;
                let posed = match clock.sample() {
                    Some((action, time)) => asset.pose_action_at_time(action, time, transform)?,
                    None => asset.pose_bind(transform)?,
                };
                let mut f = frame;
                let point = self.grid.query(lights, instance.placement.position, night);
                f.lighting.point_enabled = point.is_some();
                if let Some(point) = point {
                    f.lighting.point_ambient = point.ambient;
                    f.lighting.point_diffuse = point.diffuse;
                    f.point_position = point.position;
                    f.attenuation = point.stored_attenuation;
                }
                if let Some(draw) = &slot.draw {
                    draw.update(queue, asset, &posed, f)?;
                } else {
                    let read = &mut self.read;
                    let bytes = &mut self.bytes;
                    slot.draw = Some(Draw::new(
                        device,
                        queue,
                        self.format,
                        Some((
                            wgpu::TextureFormat::Depth32Float,
                            wgpu::CompareFunction::Greater,
                        )),
                        true,
                        &self.sampler,
                        asset,
                        &posed,
                        f,
                        |path| {
                            if let Some(data) = bytes.get(path) {
                                return Ok(data.clone());
                            }
                            let data = read(path)?;
                            bytes.insert(path.into(), data.clone());
                            Ok(data)
                        },
                    )?);
                }
                Ok(())
            })();
            if let Err(e) = result {
                eprintln!(
                    "World model {} ({}): {e}",
                    instance.placement.export_id, instance.placement.path
                );
                slot.failed = true;
                slot.draw = None;
            }
        }
    }
    /// Include dependency failures in the UI; never count a skipped asset ready.
    pub fn status(&self) -> (usize, usize, usize) {
        let ready = self.slots.iter().filter(|s| s.draw.is_some()).count();
        let failed = self.slots.iter().filter(|s| s.failed).count() + self.assets.unloaded.len();
        (ready, failed, self.slots.len() + self.assets.unloaded.len())
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for slot in &self.slots {
            if let Some(draw) = &slot.draw {
                draw.draw(pass);
            }
        }
    }
}
