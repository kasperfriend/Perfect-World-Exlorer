//! Ordinary world receiving-model submission in main and clipped water passes.
//! Owns the real scene/model effect light producers (ECWLD effects and ECM
//! EventType-100 roots, including hook-attached ones) and the receiving queues
//! they publish into.
//! Never sends baked terrain/BMD/tree vertices through receiving-model lighting.
use crate::{
    gfx_owner,
    model_attachment,
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
    /// Skeleton hook this root is attached to (ECM `HookName`).
    hook: Option<HookBinding>,
}

/// A root bound to a hook of one of this world's models.
struct HookBinding {
    /// Index into `assets.instances` of the parent model.
    instance: usize,
    /// `HookName` plus the authored hook-local transform.
    record: model_attachment::Attachment,
    /// Authored hook-local matrix, resolved once at install time.
    local: glam::Mat4,
}

/// This frame's pose and deformed meshes for one slot. Secondary views reuse
/// it so reflection/refraction never advance playback or re-sample a hook.
struct SlotPose {
    pose: crate::skeleton_pose::Pose,
    meshes: Vec<crate::model_asset::PosedMesh>,
}

struct Slot {
    draw: Option<Draw>,
    failed: bool,
    transform: Result<glam::Mat4, String>,
    /// Last evaluated pose, reused by every pass of the same frame.
    posed: Option<Result<SlotPose, String>>,
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
                posed: None,
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
                    attachment: None,
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
                    hook: None,
                });
                installed += 1;
            }
        }
        Ok(installed)
    }
    /// Install ECM EventType-100 roots, including hook-attached ones.
    ///
    /// A hooked root keeps its authored hook-local transform and receives the
    /// parent's posed hook matrix every frame
    /// (`gfx_owner::Request::attachment`); a hook-free root is placed by the
    /// model transform the event inherits. An unknown hook name or a malformed
    /// hook record is an explicit failure, never the model origin.
    pub fn install_model_events(&mut self) -> Result<usize, String> {
        let mut installed = 0;
        for (index, instance) in self.assets.instances.iter().enumerate() {
            let asset = &self.assets.assets[instance.asset];
            let transform = *self.slots[index].transform.as_ref().map_err(Clone::clone)?;
            for (event_index, event) in asset.descriptor.events.iter().enumerate() {
                let Some(path) = event.gfx_path() else {
                    continue;
                };
                let described = format!(
                    "{} export {} event {event_index}",
                    instance.placement.path, instance.placement.export_id
                );
                let record = model_attachment::from_fields(&event.fields)
                    .map_err(|e| format!("{described}: {e}"))?;
                let local = match &record {
                    Some(record) => record.local()?,
                    None => glam::Mat4::IDENTITY,
                };
                let start_ms = event_milliseconds(event, "StartTime")?.max(0.);
                let span_ms = match event_milliseconds(event, "TimeSpan")? {
                    value if value < 0. => None,
                    value => Some(value.trunc() as u32),
                };
                let identity = MODEL_EVENT_IDENTITY + self.producers.len() as u64;
                self.owners.insert(gfx_owner::Request {
                    identity,
                    path: gfx_owner::root_path(&path),
                    // A hooked root's local frame is the hook record and its
                    // parent is installed per frame; a hook-free root is placed
                    // by the model transform the native event inherits.
                    transform: match &record {
                        Some(_) => local.to_cols_array(),
                        None => transform.to_cols_array(),
                    },
                    speed: asset.descriptor.render_settings().default_speed,
                    priority: OWNER_PRIORITY,
                    start_ms: start_ms as u32,
                    span_ms,
                    outer: None,
                    attachment: None,
                    active: false,
                    origin: described,
                })?;
                self.producers.push(Producer {
                    identity,
                    valid_time: None,
                    position: instance.placement.position,
                    hook: record.map(|record| HookBinding {
                        instance: index,
                        record,
                        local,
                    }),
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
            // 1. Playback advances once per frame and each model is posed once.
            self.advance_poses(delta_ms);
            // 2. Hook-attached producers resolve their parent from those poses
            //    (0x7b2520), then publish before the grid consumes, so a light
            //    created or moved on this tick already lights this frame.
            self.resolve_attachments();
            self.apply_activation(night);
            self.owners.update(delta_ms, &mut self.publications);
            self.grid.update(lights, center);
            self.publications.consume_into(&mut self.grid);
        }
        self.draw_models(device, queue, frame, night);
        if advance {
            self.rebuild_alpha_order(center);
        }
    }
    /// Tick every playback clock once and evaluate each model's pose once.
    /// 0x7b2520 resolves hooks from this pose, and every pass of the frame
    /// (main, reflection, refraction) reuses it.
    fn advance_poses(&mut self, delta_ms: u32) {
        for (instance, slot) in self.assets.instances.iter_mut().zip(&mut self.slots) {
            if slot.failed {
                continue;
            }
            let asset = &self.assets.assets[instance.asset];
            let result = (|| -> Result<SlotPose, String> {
                let transform = *slot.transform.as_ref().map_err(Clone::clone)?;
                let clock = instance.playback.as_mut().map_err(|e| e.clone())?;
                if delta_ms != 0 {
                    clock.tick(delta_ms)?;
                }
                let pose = match clock.sample() {
                    Some((action, time)) => asset.action_pose(action, time, transform)?,
                    None => asset.bind_pose(transform)?,
                };
                let meshes = asset.meshes_from_pose(&pose)?;
                Ok(SlotPose { pose, meshes })
            })();
            slot.posed = Some(result);
        }
    }
    /// Resolve every hook-attached producer against its parent's posed hook.
    /// A model that failed to pose, a model that does not own the named hook and
    /// a destroyed parent all surface as recorded failures instead of leaving a
    /// light attached to a stale or invented transform.
    fn resolve_attachments(&mut self) {
        let mut resolved: Vec<(u64, Result<Option<[f32; 16]>, String>)> = Vec::new();
        for producer in &self.producers {
            let Some(binding) = &producer.hook else {
                continue;
            };
            let origin = format!("producer {}", producer.identity);
            let outcome = (|| -> Result<Option<[f32; 16]>, String> {
                let slot = self
                    .slots
                    .get(binding.instance)
                    .ok_or_else(|| format!("{origin}: parent slot is gone"))?;
                let parent = self
                    .assets
                    .instances
                    .get(binding.instance)
                    .ok_or_else(|| format!("{origin}: parent instance is gone"))?;
                let posed = match &slot.posed {
                    Some(Ok(posed)) => posed,
                    Some(Err(e)) => return Err(format!("{origin}: parent pose failed: {e}")),
                    None => return Err(format!("{origin}: parent has no pose")),
                };
                let asset = self
                    .assets
                    .assets
                    .get(parent.asset)
                    .ok_or_else(|| format!("{origin}: parent asset is gone"))?;
                let hook = asset.hook_matrix(&posed.pose, &binding.record.hook)?;
                let composed = model_attachment::world(hook, binding.local)?;
                Ok(Some(composed.to_cols_array()))
            })();
            resolved.push((producer.identity, outcome));
        }
        let mut detached = Vec::new();
        for (identity, outcome) in resolved {
            match outcome {
                Ok(parent) => {
                    if !self.owners.set_attachment(identity, parent) {
                        detached.push(identity);
                    }
                }
                Err(e) => {
                    self.reported.push(e);
                    self.owners.remove(identity, &mut self.publications);
                    detached.push(identity);
                }
            }
        }
        // A producer whose owner is gone or whose hook cannot be resolved is
        // dropped instead of retrying against a stale parent every frame.
        self.producers.retain(|p| !detached.contains(&p.identity));
    }
    /// Upload this frame's already-evaluated poses. Runs for every pass without
    /// touching playback, producer clocks or publications.
    fn draw_models(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: Frame,
        night: f32,
    ) {
        let Self {
            assets,
            slots,
            read,
            bytes,
            sampler,
            format,
            grid,
            reported,
            ..
        } = self;
        for (instance, slot) in assets.instances.iter().zip(slots.iter_mut()) {
            if slot.failed {
                continue;
            }
            let asset = &assets.assets[instance.asset];
            let outcome = draw_one(
                device,
                queue,
                frame,
                night,
                instance.placement.position,
                asset,
                slot,
                grid,
                sampler,
                *format,
                read,
                bytes,
            );
            if let Err(e) = outcome {
                reported.push(format!(
                    "world model {} ({}): {e}",
                    instance.placement.export_id, instance.placement.path
                ));
                slot.failed = true;
                slot.draw = None;
            }
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

/// Upload one slot's already-evaluated pose. Split out of `World::draw_models`
/// so the alpha-order snapshot and the attachment parent are read from the same
/// pose the pass draws.
#[allow(clippy::too_many_arguments)]
fn draw_one(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: Frame,
    night: f32,
    position: [f32; 3],
    asset: &crate::model_asset::ModelAsset,
    slot: &mut Slot,
    grid: &StaticGrid,
    sampler: &wgpu::Sampler,
    format: wgpu::TextureFormat,
    read: &mut Box<dyn FnMut(&str) -> Result<Vec<u8>, String>>,
    bytes: &mut HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    let posed = match &slot.posed {
        Some(Ok(posed)) => posed,
        Some(Err(e)) => return Err(e.clone()),
        None => return Err("model has no pose for this pass".into()),
    };
    let mut f = frame;
    let point = grid.query(position, night);
    // Ordinary loaded ECMs enable the specular selector through
    // 0x6cfa64 -> 0x7924b0/0x7924c1; the point constants depend on whether the
    // scene query returned a candidate.
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
        return draw.update(queue, asset, &posed.meshes, f);
    }
    let created = Draw::new(
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
        &posed.meshes,
        f,
        |path| {
            if let Some(data) = bytes.get(path) {
                return Ok(data.clone());
            }
            let data = read(path)?;
            bytes.insert(path.into(), data.clone());
            Ok(data)
        },
    )?;
    slot.draw = Some(created);
    Ok(())
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
