//! Populator add-on: spawns the real world population the way the game does.
//!
//! Scope, in the client's own terms: the shipped `element/data/task_npc.data`
//! places 1,572 quest NPCs, each one resolved to its `NPC_ESSENCE` template and
//! drawn with that template's `.ecm` rig out of `models.pck`. Mobs have no
//! client-side placement file (the server owns `gamedata/npcgen.data`), so the
//! module loads the 4,208 real `MONSTER_ESSENCE` templates and draws no mob
//! until a placement source is supplied. Actors move only along real navigation
//! paths from server data; nothing wanders.
//!
//! Everything here is module-owned: archives, rig cache, texture cache, GPU
//! draws, simulation and settings. The host supplies a camera position (which
//! populates the streamed set) and one render hook.

mod archive;
mod data;
mod draw;
mod patrol;

use self::{
    archive::Archive,
    data::{Population, Spawn, TemplateKind},
    draw::Renderer,
    patrol::{Follow, Motion},
};
use crate::{
    model_asset::{ModelAsset, PosedMesh, TrackCache},
    model_draw_gpu,
    model_playback::Playback,
    navigation::MapSpace,
};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, path::PathBuf, rc::Rc};

/// The client's shipped quality preset reserves a seven-block terrain window,
/// 7 x 64 = 420 world units (`view_settings::DetailSettings`, `world+0x1bc`;
/// preset table `0x40e41f`). The default spawn distance is therefore the value
/// the game itself uses, not a chosen one.
pub const NATIVE_SPAWN_DISTANCE: f32 = 420.;
pub const MIN_SPAWN_DISTANCE: f32 = 40.;
pub const MAX_SPAWN_DISTANCE: f32 = 1_200.;
const DEFAULT_MAX_VISIBLE: usize = 48;
const MAX_VISIBLE_LIMIT: usize = 96;
/// Rigs are decompressed from an 888 MB `models.pck` on the render thread, so a
/// teleport into a crowded town fills the population over a few frames instead
/// of stalling one.
const MAX_SPAWNS_PER_UPDATE: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// World-space radius around the camera that keeps actors spawned.
    pub spawn_distance: f32,
    /// Hard cap on simultaneously drawn actors.
    pub max_visible: usize,
    /// Play the template's own `休闲` scene action.
    pub idle_animation: bool,
    /// Face the camera instead of the placement's unrecorded facing.
    pub face_camera: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            spawn_distance: NATIVE_SPAWN_DISTANCE,
            max_visible: DEFAULT_MAX_VISIBLE,
            idle_animation: true,
            face_camera: false,
        }
    }
}

impl Settings {
    fn sanitized(self) -> Self {
        Self {
            spawn_distance: if self.spawn_distance.is_finite() {
                self.spawn_distance.clamp(MIN_SPAWN_DISTANCE, MAX_SPAWN_DISTANCE)
            } else {
                NATIVE_SPAWN_DISTANCE
            },
            max_visible: self.max_visible.clamp(1, MAX_VISIBLE_LIMIT),
            ..self
        }
    }
}

/// Module-owned `models.pck`/`textures.pck` rig and texture source.
struct Library {
    models: Archive,
    textures: Option<Archive>,
    tracks: TrackCache,
    assets: HashMap<String, Rc<ModelAsset>>,
    failures: HashMap<String, String>,
}

fn read_archive(
    models: &mut Archive,
    textures: Option<&mut Archive>,
    path: &str,
) -> Result<Vec<u8>, String> {
    let models_error = match models.read(path) {
        Ok(bytes) => return Ok(bytes),
        Err(error) => error,
    };
    match textures {
        Some(textures) => textures.read(path),
        // A missing textures.pck is not fatal: models.pck carries the player
        // and NPC texture sheets alongside the rigs.
        None => Err(models_error),
    }
}

impl Library {
    fn open(element: &std::path::Path) -> Result<Self, String> {
        let models = Archive::open(&element.join("models.pck"))?;
        let textures = Archive::open(&element.join("textures.pck")).ok();
        Ok(Self {
            models,
            textures,
            tracks: TrackCache::default(),
            assets: HashMap::new(),
            failures: HashMap::new(),
        })
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        read_archive(&mut self.models, self.textures.as_mut(), path)
    }

    fn asset(&mut self, path: &str) -> Result<Rc<ModelAsset>, String> {
        let key = crate::ecmodel::normalize(path);
        if let Some(asset) = self.assets.get(&key) {
            return Ok(asset.clone());
        }
        if let Some(error) = self.failures.get(&key) {
            return Err(error.clone());
        }
        let Self {
            models,
            textures,
            tracks,
            assets,
            failures,
        } = self;
        let mut read = |path: &str| read_archive(models, textures.as_mut(), path);
        let loaded = ModelAsset::load_cached(&key, &mut read, tracks);
        match loaded {
            Ok(asset) => {
                let asset = Rc::new(asset);
                assets.insert(key, asset.clone());
                Ok(asset)
            }
            Err(error) => {
                failures.insert(key, error.clone());
                Err(error)
            }
        }
    }
}

/// One spawned actor.
struct Live {
    /// Index into [`Population::spawns`].
    spawn: usize,
    position: Vec3,
    yaw: f32,
    asset: Rc<ModelAsset>,
    /// Ordinary action index, used when `Playback` cannot schedule the
    /// template's combined action.
    action: Option<usize>,
    playback: Option<Playback>,
    /// Fallback clock in seconds for actors without a `Playback`.
    clock: f32,
    /// Real navigation path, when the population supplies one.
    follow: Option<Follow>,
    draw: Option<model_draw_gpu::Draw>,
    /// Fatal per-actor error (rig, pose or draw); the actor is not submitted.
    failure: Option<String>,
}

impl Live {
    fn world(&self) -> Mat4 {
        transform(self.position, self.yaw)
    }

    /// Posed meshes for this frame. `Playback` supplies the native scene clock
    /// when the template's combined action is a single base; otherwise the
    /// ordinary `休闲` action runs on the module clock, and a rig that can
    /// evaluate neither falls back to its authored bind pose rather than
    /// disappearing.
    fn pose(&self) -> Result<Vec<PosedMesh>, String> {
        let world = self.world();
        let sample = self.playback.as_ref().and_then(Playback::sample);
        let (action, seconds) = match sample {
            Some((action, seconds)) => (Some(action), seconds),
            None => (self.action, self.clock),
        };
        if let Some(action) = action {
            if let Ok(meshes) = self.asset.pose_action_at_time(action, seconds, world) {
                return Ok(meshes);
            }
        }
        self.asset.pose_bind(world)
    }
}

/// Model-to-world matrix with the actor's local `+Z` as forward, matching
/// `walker::character::character_transform`.
pub(crate) fn transform(position: Vec3, yaw: f32) -> Mat4 {
    let direction = Vec3::new(yaw.sin(), 0., -yaw.cos());
    let right = Vec3::Y.cross(direction).normalize_or_zero();
    Mat4::from_cols(
        right.extend(0.),
        Vec3::Y.extend(0.),
        direction.extend(0.),
        position.extend(1.),
    )
}

/// `task_npc.data` authors 0 for "stand on the terrain"; any other value is the
/// absolute height the game uses, which can legitimately be above the terrain.
fn anchor_height(spawn: &Spawn, ground: f32) -> f32 {
    spawn.height.unwrap_or(ground)
}

/// The template's own scene idle. `EcModel::scene_action` is the client's
/// 0x5b3a4f lookup, including its `休闲` fallback (0x95a750).
fn idle_clock(asset: &ModelAsset) -> (Option<usize>, Option<Playback>) {
    let combined = asset.descriptor.scene_action("休闲");
    match Playback::new(asset, combined) {
        // `Playback` rejects multi-base combined actions; the same named
        // ordinary action still animates the actor on its own clock.
        Err(_) => (
            asset
                .model
                .actions
                .iter()
                .position(|action| action.name == "休闲"),
            None,
        ),
        Ok(playback) => (None, Some(playback)),
    }
}

pub struct Populator {
    element: PathBuf,
    /// The host world. `None` only for the registry's inert default instance.
    map: Option<MapSpace>,
    settings: Settings,
    active: bool,
    population: Option<Population>,
    load_error: Option<String>,
    library: Option<Library>,
    library_error: Option<String>,
    renderer: Option<Renderer>,
    live: Vec<Live>,
    eye: Vec3,
    reload_pending: bool,
}

impl Default for Populator {
    fn default() -> Self {
        Self {
            element: PathBuf::new(),
            map: None,
            settings: Settings::default(),
            active: false,
            population: None,
            load_error: None,
            library: None,
            library_error: None,
            renderer: None,
            live: Vec::new(),
            eye: Vec3::ZERO,
            reload_pending: false,
        }
    }
}

impl Populator {
    pub fn new(element: PathBuf, map: MapSpace) -> Self {
        Self {
            element,
            map: Some(map),
            ..Self::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, active: bool, eye: Vec3) {
        self.eye = eye;
        if self.active == active {
            return;
        }
        self.active = active;
        // Disabling drops every rig, draw and actor; the archives and the
        // placement tables stay for an instant re-enable.
        self.live.clear();
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn status(&self) -> String {
        if let Some(error) = &self.load_error {
            return format!("Population unavailable: {error}");
        }
        if let Some(error) = &self.library_error {
            return format!("models.pck unavailable: {error}");
        }
        if self.map.is_none() {
            return "Waiting for a loaded world".into();
        }
        match &self.population {
            Some(population) => format!(
                "{} real placements, {} drawn",
                population.spawns.len(),
                self.live.len()
            ),
            None => "Loading the real population...".into(),
        }
    }

    /// Per-frame host hook: streams actors around the camera and advances the
    /// ones standing on a navigation path.
    pub fn update(&mut self, delta: f32, eye: Vec3) {
        if !self.active {
            return;
        }
        self.eye = eye;
        if self.reload_pending {
            self.reload_pending = false;
            self.population = None;
            self.library = None;
            self.load_error = None;
            self.library_error = None;
            self.live.clear();
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.forget_textures();
            }
        }
        if self.map.is_none() {
            return;
        }
        if self.population.is_none() && self.load_error.is_none() {
            match Population::open(&self.element) {
                Ok(population) => self.population = Some(population),
                Err(error) => self.load_error = Some(error),
            }
        }
        if self.population.is_none() {
            return;
        }
        if self.library.is_none() && self.library_error.is_none() {
            match Library::open(&self.element) {
                Ok(library) => self.library = Some(library),
                Err(error) => self.library_error = Some(error),
            }
        }
        if self.library.is_none() {
            return;
        }
        self.stream();
        self.advance(delta);
    }

    /// Reconciles the drawn set with the placements inside the spawn distance.
    /// Actors that leave the radius are dropped, so the module never holds rigs
    /// it is not drawing.
    fn stream(&mut self) {
        let nearest = match self.population.as_ref() {
            Some(population) => population.nearest(
                [self.eye.x, self.eye.z],
                self.settings.spawn_distance,
                self.settings.max_visible,
            ),
            None => return,
        };
        let mut previous: Vec<Option<Live>> =
            std::mem::take(&mut self.live).into_iter().map(Some).collect();
        // Keep every actor that is still in range, in distance order, before
        // spending the frame's rig budget on new ones.
        let mut next = Vec::with_capacity(nearest.len());
        let mut missing = Vec::new();
        for (index, _) in nearest {
            let existing = previous
                .iter_mut()
                .find(|slot| matches!(slot, Some(live) if live.spawn == index))
                .and_then(|slot| slot.take());
            match existing {
                Some(live) => next.push(live),
                None => missing.push(index),
            }
        }
        for index in missing.into_iter().take(MAX_SPAWNS_PER_UPDATE) {
            if let Some(live) = self.spawn_instance(index) {
                next.push(live);
            }
        }
        self.live = next;
    }

    fn spawn_instance(&mut self, index: usize) -> Option<Live> {
        let spawn = self.population.as_ref()?.spawns.get(index)?.clone();
        let position = self.position_for(&spawn);
        let yaw = if self.settings.face_camera {
            let towards = self.eye - position;
            if towards.length_squared() > 1e-6 {
                patrol::yaw_towards(towards)
            } else {
                0.
            }
        } else {
            // No client file records an NPC facing; `task_npc.data` is id + XZ.
            0.
        };
        let follow = self.route_for(&spawn);
        let library = self.library.as_mut()?;
        let asset = library.asset(&spawn.model).ok()?;
        let (action, playback) = if self.settings.idle_animation {
            idle_clock(asset.as_ref())
        } else {
            (None, None)
        };
        Some(Live {
            spawn: index,
            position,
            yaw,
            asset,
            action,
            playback,
            clock: 0.,
            follow,
            draw: None,
            failure: None,
        })
    }

    fn position_for(&self, spawn: &Spawn) -> Vec3 {
        let ground = match self.map.as_ref() {
            Some(map) => map.ground(spawn.anchor[0], spawn.anchor[1]),
            None => 0.,
        };
        Vec3::new(
            spawn.anchor[0],
            anchor_height(spawn, ground),
            spawn.anchor[1],
        )
    }

    fn route_for(&self, spawn: &Spawn) -> Option<Follow> {
        let route = self.population.as_ref()?.routes.get(spawn.route?)?;
        Follow::new(route).ok()
    }

    fn advance(&mut self, delta: f32) {
        if !delta.is_finite() || delta <= 0. {
            return;
        }
        let delta_ms = (delta * 1000.).round().clamp(0., u32::MAX as f32) as u32;
        for live in &mut self.live {
            if live.failure.is_some() {
                continue;
            }
            live.clock += delta;
            if let Some(playback) = live.playback.as_mut() {
                // A scheduling error degrades to the ordinary-action clock
                // rather than hiding the actor.
                if playback.tick(delta_ms).is_err() {
                    live.playback = None;
                }
            }
            let Some(follow) = live.follow.as_mut() else {
                continue;
            };
            let before = live.position;
            match follow.step(&mut live.position, delta) {
                Motion::Walking => {
                    let moved = live.position - before;
                    if moved.length_squared() > 1e-8 {
                        live.yaw = patrol::yaw_towards(moved);
                    }
                    if let Some(map) = self.map.as_ref() {
                        live.position.y = map.ground(live.position.x, live.position.z);
                    }
                }
                // The route is over once its final dwell has been served; the
                // actor then keeps the last position and facing.
                Motion::Finished => live.follow = None,
                Motion::Waiting => {}
            }
        }
    }

    /// Adds the population to the host's model pass. The pass is only opened
    /// when at least one actor has a valid draw.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame: model_draw_gpu::Frame,
    ) {
        if !self.active || self.live.is_empty() {
            return;
        }
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(device));
        }
        let Some(library) = self.library.as_mut() else {
            return;
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        // Evaluate every pose before touching GPU state, so one broken rig
        // cannot stop the rest of the population from appearing.
        let mut posed: Vec<(usize, Vec<PosedMesh>)> = Vec::with_capacity(self.live.len());
        for index in 0..self.live.len() {
            let live = &mut self.live[index];
            if live.failure.is_some() {
                continue;
            }
            match live.pose() {
                Ok(meshes) => posed.push((index, meshes)),
                Err(error) => live.failure = Some(error),
            }
        }
        for (index, meshes) in posed {
            let live = &mut self.live[index];
            let asset = live.asset.clone();
            let built = match live.draw.take() {
                Some(draw) => match draw.update(queue, asset.as_ref(), &meshes, frame) {
                    Ok(()) => Some(draw),
                    Err(error) => {
                        live.failure = Some(error);
                        None
                    }
                },
                None => match renderer.build(
                    device,
                    queue,
                    format,
                    asset.as_ref(),
                    &meshes,
                    frame,
                    |path| library.read(path),
                ) {
                    Ok(draw) => Some(draw),
                    Err(error) => {
                        live.failure = Some(error);
                        None
                    }
                },
            };
            live.draw = built;
        }
        if !self.live.iter().any(|live| live.draw.is_some()) {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("add-on Populator pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        for live in &self.live {
            if let Some(draw) = &live.draw {
                draw.draw(&mut pass);
            }
        }
    }

    fn drawn_count(&self) -> usize {
        self.live.iter().filter(|live| live.draw.is_some()).count()
    }

    fn failed_count(&self) -> usize {
        self.live.iter().filter(|live| live.failure.is_some()).count()
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Populator");
        ui.label(egui::RichText::new(self.status()).small().weak());
        ui.separator();

        ui.heading("Real population");
        match (&self.population, &self.load_error) {
            (_, Some(error)) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            (Some(population), None) => {
                let npcs = population
                    .spawns
                    .iter()
                    .filter(|spawn| spawn.kind == TemplateKind::Npc)
                    .count();
                let monsters = population
                    .spawns
                    .iter()
                    .filter(|spawn| spawn.kind == TemplateKind::Monster)
                    .count();
                let linked = population
                    .spawns
                    .iter()
                    .filter(|spawn| spawn.task_count > 0)
                    .count();
                ui.label(format!(
                    "{npcs} NPC and {monsters} monster placements, {linked} with task links, {} skipped",
                    population.skipped
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "task_npc.data: {} bytes, timestamp 0x{:08x}, {} records",
                        population.table_size,
                        population.table_timestamp,
                        population.spawns.len() + population.skipped
                    ))
                    .small()
                    .weak(),
                );
                for line in &population.provenance {
                    ui.label(egui::RichText::new(line).small().weak());
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} NPC templates, {} monster templates",
                        population.npc_templates, population.monster_templates
                    ))
                    .small()
                    .weak(),
                );
            }
            (None, None) => {
                ui.label("Reading elements.data and task_npc.data...");
            }
        }
        if let Some(error) = &self.library_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(library) = &self.library {
            ui.label(
                egui::RichText::new(format!(
                    "models.pck: {} entries, {} rigs loaded, {} failed",
                    library.models.len(),
                    library.assets.len(),
                    library.failures.len()
                ))
                .small()
                .weak(),
            );
        }
        if ui.button("Reload from the element folder").clicked() {
            self.reload_pending = true;
        }
        if let Some(library) = &self.library {
            if !library.failures.is_empty() {
                egui::CollapsingHeader::new(format!("Failed rigs ({})", library.failures.len()))
                    .show(ui, |ui| {
                        for (path, error) in library.failures.iter().take(8) {
                            ui.label(egui::RichText::new(format!("{path}: {error}")).small());
                        }
                    });
            }
        }

        ui.separator();
        ui.heading("Spawn distance");
        ui.add(
            egui::Slider::new(
                &mut self.settings.spawn_distance,
                MIN_SPAWN_DISTANCE..=MAX_SPAWN_DISTANCE,
            )
            .text("units")
            .suffix(" u"),
        );
        ui.horizontal(|ui| {
            for (label, distance) in [
                ("Scenery 120", 120.),
                ("Native 420", NATIVE_SPAWN_DISTANCE),
                ("Wide 800", 800.),
            ] {
                if ui.button(label).clicked() {
                    self.settings.spawn_distance = distance;
                }
            }
        });
        ui.label(
            egui::RichText::new(
                "420 units is the client's own scene window: its quality presets keep a seven-block terrain radius (7 x 64 units, DetailSettings.radius at world+0x1bc). Shipped detail_scene preset 1 has ornament culling at 120 units.",
            )
            .small()
            .weak(),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.max_visible, 1..=MAX_VISIBLE_LIMIT)
                .text("actors"),
        );
        ui.checkbox(&mut self.settings.idle_animation, "Idle animation");
        ui.checkbox(
            &mut self.settings.face_camera,
            "Turn to face the camera (no placement file records a facing)",
        );
        self.settings = self.settings.sanitized();
        ui.label(
            egui::RichText::new(format!(
                "{} drawn, {} failed, {} rigs cached, {} textures cached",
                self.drawn_count(),
                self.failed_count(),
                self.library
                    .as_ref()
                    .map_or(0, |library| library.assets.len()),
                self.renderer.as_ref().map_or(0, Renderer::texture_count)
            ))
            .small()
            .weak(),
        );

        ui.separator();
        ui.heading("Population state");
        if let Some(population) = &self.population {
            let moving = self.live.iter().filter(|live| live.follow.is_some()).count();
            ui.label(egui::RichText::new(&population.mob_placement).small().weak());
            ui.label(
                egui::RichText::new(format!(
                    "{moving} actors on navigation paths. {}",
                    population.route_source
                ))
                .small()
                .weak(),
            );
        }
        ui.label(
            egui::RichText::new(
                "Spawn and appear only: no combat, dialogue or interaction. Positions, names, rigs and heights are the files' own; nothing is synthesised.",
            )
            .small()
            .weak(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(height: Option<f32>) -> Spawn {
        Spawn {
            kind: TemplateKind::Npc,
            id: 2109,
            name: "Apothecary Cheng".into(),
            model: "models/npcs/npc/男npc4/男npc4.ecm".into(),
            anchor: [-1571., 2184.],
            height,
            task_count: 12,
            route: None,
        }
    }

    #[test]
    fn default_spawn_distance_is_the_clients_own_scene_window() {
        let settings = Settings::default();
        assert_eq!(settings.spawn_distance, 420.);
        assert!(settings.idle_animation);
        assert!(!settings.face_camera);
        assert_eq!(settings.sanitized(), settings);
    }

    #[test]
    fn settings_are_clamped_to_their_supported_range() {
        let low = Settings {
            spawn_distance: -5.,
            max_visible: 0,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(low.spawn_distance, MIN_SPAWN_DISTANCE);
        assert_eq!(low.max_visible, 1);
        let high = Settings {
            spawn_distance: f32::INFINITY,
            max_visible: 10_000,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(high.spawn_distance, NATIVE_SPAWN_DISTANCE);
        assert_eq!(high.max_visible, MAX_VISIBLE_LIMIT);
    }

    #[test]
    fn authored_height_wins_and_zero_means_terrain() {
        assert_eq!(anchor_height(&spawn(Some(276.)), 120.), 276.);
        assert_eq!(anchor_height(&spawn(None), 120.), 120.);
    }

    #[test]
    fn transform_maps_local_axes_into_the_world() {
        let at = Vec3::new(3., 4., 5.);
        let identity = transform(at, 0.);
        assert!((identity.transform_point3(Vec3::ZERO) - at).length() < 1e-5);
        // Local +Z is forward, so yaw 0 points at world -Z.
        let forward = identity.transform_vector3(Vec3::Z);
        assert!((forward - Vec3::NEG_Z).length() < 1e-5);
        let up = identity.transform_vector3(Vec3::Y);
        assert!((up - Vec3::Y).length() < 1e-5);
        // A quarter turn puts forward on world +X.
        let quarter = transform(at, std::f32::consts::FRAC_PI_2);
        let forward = quarter.transform_vector3(Vec3::Z);
        assert!((forward - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn an_inert_module_reports_why_it_cannot_draw() {
        let mut populator = Populator::default();
        populator.set_active(true, Vec3::ZERO);
        populator.update(0.016, Vec3::ZERO);
        assert!(populator.live.is_empty());
        assert_eq!(populator.status(), "Waiting for a loaded world");
    }
}
