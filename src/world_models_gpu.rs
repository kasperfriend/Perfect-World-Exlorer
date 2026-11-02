//! Ordinary world receiving-model submission in main and clipped water passes.
//! Owns the real scene/model effect light producers (ECWLD effects and hook-free
//! ECM EventType-100 roots) and the receiving queues they publish into.
//! Never sends baked terrain/BMD/tree vertices through receiving-model lighting.
use crate::{
    gfx_owner,
    model_draw_gpu::{Draw, Frame},
    model_queue,
    scene_lights::{DynamicLight, LightPublications, SceneLights, StaticGrid},
    world_models::Assets,
};
use std::{collections::HashMap, fs::File, path::Path};

/// Manager priority handed to scene/model effect lights. The native manager
/// priority source for these owners was not recovered from the supplied
/// executable, so production does not suppress any element: an invented
/// threshold would silently drop authored lights. The gate itself is real and
/// covered by the owner regressions.
pub const OWNER_PRIORITY: i32 = i32::MAX;

/// Identity space for ECM model-event owners, kept disjoint from scene effects.
const MODEL_EVENT_IDENTITY: u64 = 1 << 40;

/// One light-producing owner installed from the scene or from a model event.
struct Producer {
    identity: u64,
    /// ECWLD effect time eligibility; `None` for model events and all-day roots.
    valid_time: Option<i32>,
    position: [f32; 3],
}

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
    publications: LightPublications,
    owners: gfx_owner::Manager,
    producers: Vec<Producer>,
    /// Explorer position and effect activation radius in world units.
    view: Option<(f32, f32, f32)>,
    /// Retail alpha insertion order as (slot, mesh) pairs.
    alpha: Vec<model_queue::AlphaCandidate>,
    reported: Vec<String>,
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
            publications: LightPublications::default(),
            owners: gfx_owner::Manager::new(),
            producers: Vec::new(),
            view: None,
            alpha: Vec::new(),
            reported: Vec::new(),
        })
    }
    /// Attach the real GFX archive so installed producers can load their roots.
    /// Root text is GBK-decoded here; the owner manager never guesses an
    /// encoding and refuses binary containers explicitly.
    pub fn attach_gfx_archive(&mut self, element: &Path) -> Result<(), String> {
        let mut pack = File::open(element.join("gfx.pck")).map_err(|e| format!("gfx.pck: {e}"))?;
        let entries: HashMap<String, crate::pck::Entry> = crate::pck::directory(&mut pack)?
            .into_iter()
            .map(|e| (crate::ecmodel::normalize(&e.name), e))
            .collect();
        self.owners.set_reader(move |path: &str| {
            let entry = entries
                .get(path)
                .ok_or_else(|| "missing gfx.pck entry".to_string())?;
            let bytes = crate::pck::read_entry(&mut pack, entry)?;
            if !bytes.starts_with(b"MOXT") {
                return Err("binary GFX container is not implemented".into());
            }
            let (text, _, bad) = encoding_rs::GBK.decode(&bytes);
            if bad {
                return Err("invalid GBK GFX text".into());
            }
            Ok(text.into_owned())
        });
        Ok(())
    }
    /// Attach an arbitrary root reader (diagnostics and GPU regressions).
    pub fn attach_gfx_reader(
        &mut self,
        read: impl FnMut(&str) -> Result<String, String> + 'static,
    ) {
        self.owners.set_reader(read);
    }
    /// Install the actual ECWLD effect placements as scene-light producers.
    /// The owners stay pending and are loaded one root per update, as 0x5b1a00.
    pub fn install_scene_effects(&mut self, scene: &crate::scene::Scene) -> Result<usize, String> {
        let mut installed = 0;
        for block in &scene.blocks {
            for effect in &block.effects {
                let transform = gfx_owner::placement_transform(
                    effect.position,
                    effect.direction,
                    effect.up,
                    effect.scale,
                )?;
                let identity = self.producers.len() as u64;
                self.owners.insert(gfx_owner::Request {
                    identity,
                    path: gfx_owner::root_path(&effect.path),
                    transform,
                    speed: effect.speed,
                    priority: OWNER_PRIORITY,
                    start_ms: 0,
                    span_ms: None,
                    outer: None,
                    active: false,
                    origin: format!(
                        "world.ecwld ({},{}) export {}",
                        block.row, block.col, effect.export_id
                    ),
                })?;
                self.producers.push(Producer {
                    identity,
                    valid_time: Some(effect.valid_time),
                    position: effect.position,
                });
                installed += 1;
            }
        }
        Ok(installed)
    }
    /// Install ECM EventType-100 roots that need no skeleton hook. A recovered
    /// non-empty HookName is refused explicitly instead of being attached to the
    /// model origin: hook resolution is not implemented.
    pub fn install_model_events(&mut self) -> Result<usize, String> {
        let mut installed = 0;
        for (index, instance) in self.assets.instances.iter().enumerate() {
            let asset = &self.assets.assets[instance.asset];
            let transform = *self.slots[index].transform.as_ref().map_err(Clone::clone)?;
            for (event_index, event) in asset.descriptor.events.iter().enumerate() {
                let Some(path) = event.gfx_path() else {
                    continue;
                };
                let hook = event
                    .fields
                    .iter()
                    .find(|(key, _)| key == "HookName")
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("");
                if !hook.is_empty() {
                    self.reported.push(format!(
                        "{} event {event_index}: hook {hook:?} attachment is not implemented",
                        instance.placement.path
                    ));
                    continue;
                }
                let start_ms = event_milliseconds(event, "StartTime")?.max(0.);
                let span_ms = match event_milliseconds(event, "TimeSpan")? {
                    value if value < 0. => None,
                    value => Some(value.trunc() as u32),
                };
                let identity = MODEL_EVENT_IDENTITY + self.producers.len() as u64;
                self.owners.insert(gfx_owner::Request {
                    identity,
                    path: gfx_owner::root_path(&path),
                    transform: transform.to_cols_array(),
                    speed: asset.descriptor.render_settings().default_speed,
                    priority: OWNER_PRIORITY,
                    start_ms: start_ms as u32,
                    span_ms,
                    outer: None,
                    active: false,
                    origin: format!(
                        "{} export {} event {event_index}",
                        instance.placement.path, instance.placement.export_id
                    ),
                })?;
                self.producers.push(Producer {
                    identity,
                    valid_time: None,
                    position: instance.placement.position,
                });
                installed += 1;
            }
        }
        Ok(installed)
    }
    /// Explorer position plus the radius in which installed producers are
    /// active. Effects outside the radius are unloaded without publishing.
    pub fn set_view_center(&mut self, x: f32, z: f32, radius: f32) {
        self.view = Some((x, z, radius.max(0.)));
    }
    pub fn gfx_status(&self) -> gfx_owner::Status {
        self.owners.status()
    }
    /// Direct publication entry point for an already-resolved producer.
    /// Production uses the owner manager below; this stays public because the
    /// publication bridge regressions drive it with explicit identities.
    pub fn publish_dynamic_light(&mut self, identity: u64, light: DynamicLight) {
        self.publications.publish(identity, light);
    }
    /// Execute one decoded retail light element and publish its current params.
    /// The caller owns the scaled tick, resolved parent, visibility/priority and
    /// destruction (`unregister_dynamic_light`).
    pub fn tick_gfx_light(
        &mut self,
        identity: u64,
        emitter: &mut crate::gfx_light::Emitter,
        scaled_delta_ms: u32,
        visible: bool,
        priority: i32,
        parent: [f32; 16],
        outer: Option<[f32; 4]>,
    ) -> Result<bool, String> {
        if let Some(light) = emitter.tick(scaled_delta_ms, visible, priority, parent, outer)? {
            self.publish_dynamic_light(identity, light);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn unregister_dynamic_light(&mut self, identity: u64) {
        self.publications.unregister(identity);
    }
    pub fn gfx_failures(&self) -> Vec<String> {
        let mut failures = self.owners.failures().to_vec();
        failures.extend(self.reported.iter().cloned());
        failures
    }
    fn apply_activation(&mut self, night: f32) {
        if self.producers.is_empty() {
            return;
        }
        let (x, z, radius) = self.view.unwrap_or((0., 0., -1.));
        let updates: Vec<(u64, bool)> = self
            .producers
            .iter()
            .map(|producer| {
                let dx = producer.position[0] - x;
                let dz = producer.position[2] - z;
                let inside = dx * dx + dz * dz <= radius * radius;
                let visible = match producer.valid_time {
                    Some(mode) => crate::scene::effect_visible(mode, night),
                    None => true,
                };
                (producer.identity, inside && visible)
            })
            .collect();
        for (identity, active) in updates {
            self.owners.set_active(identity, active);
        }
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
        self.update_pass(device, queue, frame, center, lights, night, delta_ms, true);
    }
    /// The first/main pass advances the frame, ticks producers and consumes
    /// publications. Reflection/refraction only update pass-dependent
    /// matrices/fog/clip; they must reuse the same lighting snapshot, playback
    /// time and alpha order.
    pub fn update_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: Frame,
        center: [f32; 3],
        lights: &SceneLights,
        night: f32,
        delta_ms: u32,
        advance: bool,
    ) {
        if advance {
            // Producers publish before the grid consumes, so a light created on
            // this tick already lights this frame's receivers.
            self.apply_activation(night);
            self.owners.update(delta_ms, &mut self.publications);
            self.grid.update(lights, center);
            self.publications.consume_into(&mut self.grid);
        }
        for (instance, slot) in self.assets.instances.iter_mut().zip(&mut self.slots) {
            if slot.failed {
                continue;
            }
            let asset = &self.assets.assets[instance.asset];
            let result = (|| -> Result<(), String> {
                let transform = *slot.transform.as_ref().map_err(Clone::clone)?;
                let clock = instance.playback.as_mut().map_err(|e| e.clone())?;
                if advance {
                    clock.tick(delta_ms)?;
                }
                let posed = match clock.sample() {
                    Some((action, time)) => asset.pose_action_at_time(action, time, transform)?,
                    None => asset.pose_bind(transform)?,
                };
                let mut f = frame;
                let point = self.grid.query(instance.placement.position, night);
                // Ordinary loaded ECMs enable the specular selector through
                // 0x6cfa64 -> 0x7924b0/0x7924c1; the point constants depend on
                // whether the scene query returned a candidate.
                let assignment = crate::model_receiver_class::assign(
                    crate::model_receiver_class::MeshKind::Weighted,
                    true,
                    point.is_some(),
                )?;
                f.specular_variant = assignment.directional_specular;
                f.lighting.point_enabled = assignment.point_constants;
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
                self.reported.push(format!(
                    "world model {} ({}): {e}",
                    instance.placement.export_id, instance.placement.path
                ));
                slot.failed = true;
                slot.draw = None;
            }
        }
        if advance {
            self.rebuild_alpha_order(center);
        }
    }
    /// Retail alpha insertion walk over this frame's alpha meshes. Ordering uses
    /// the eight transformed initial-AABB-corner distance; the per-mesh
    /// priority weights of the original insertion routine were not recovered, so
    /// they stay zero and the comparison falls through to distance.
    fn rebuild_alpha_order(&mut self, eye: [f32; 3]) {
        let mut candidates: Vec<model_queue::AlphaCandidate> = Vec::new();
        let mut failures: Vec<(usize, String)> = Vec::new();
        for (slot, entry) in self.slots.iter().enumerate() {
            let Some(draw) = &entry.draw else {
                continue;
            };
            let Ok(transform) = &entry.transform else {
                continue;
            };
            for &mesh in draw.alpha_meshes() {
                let Some((min, max)) = draw.mesh_bounds(mesh) else {
                    continue;
                };
                match model_queue::alpha_corner_distance(
                    min,
                    max,
                    transform.to_cols_array_2d(),
                    eye,
                ) {
                    Ok(distance) => candidates.push(model_queue::AlphaCandidate {
                        slot,
                        mesh,
                        node: model_queue::AlphaNode {
                            model: slot,
                            mesh_weight: 0,
                            skin_weight: 0,
                            group: 0,
                            group_weight: 0,
                            min,
                            max,
                            distance,
                        },
                    }),
                    Err(e) => failures.push((slot, e)),
                }
            }
        }
        match model_queue::alpha_order(&candidates) {
            Ok(order) => self.alpha = order,
            Err(e) => {
                self.alpha.clear();
                self.reported.push(format!("alpha queue: {e}"));
            }
        }
        for (slot, error) in failures {
            self.reported
                .push(format!("alpha queue slot {slot}: {error}"));
            if let Some(entry) = self.slots.get_mut(slot) {
                entry.failed = true;
                entry.draw = None;
            }
            self.alpha.retain(|candidate| candidate.slot != slot);
        }
    }
    /// Include dependency failures in the UI; never count a skipped asset ready.
    pub fn status(&self) -> (usize, usize, usize) {
        let ready = self.slots.iter().filter(|s| s.draw.is_some()).count();
        let failed = self.slots.iter().filter(|s| s.failed).count() + self.assets.unloaded.len();
        (ready, failed, self.slots.len() + self.assets.unloaded.len())
    }
    /// Opaque meshes first, then the retail alpha list in its insertion order.
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for slot in &self.slots {
            if let Some(draw) = &slot.draw {
                draw.draw_opaque(pass);
            }
        }
        for candidate in &self.alpha {
            if let Some(Some(draw)) = self.slots.get(candidate.slot).map(|s| &s.draw) {
                draw.draw_alpha_mesh(pass, candidate.mesh);
            }
        }
    }
    pub fn alpha_order(&self) -> Vec<(usize, usize)> {
        self.alpha.iter().map(|c| (c.slot, c.mesh)).collect()
    }
}

/// Event milliseconds from an ECM event field. Missing fields are explicit
/// failures: the light owner must not invent a start time or a lifetime.
fn event_milliseconds(event: &crate::ecmodel::EcmEvent, key: &str) -> Result<f32, String> {
    let Some((_, value)) = event.fields.iter().find(|(name, _)| name == key) else {
        return Err(format!("GFX event is missing {key}"));
    };
    let value: f32 = value
        .parse()
        .map_err(|_| format!("invalid GFX event {key}: {value}"))?;
    if !value.is_finite() {
        return Err(format!("nonfinite GFX event {key}"));
    }
    Ok(value)
}
