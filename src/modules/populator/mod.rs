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
/// Overhead names are legible at the client's own scene window, so they default
/// to the same radius; the slider lets a user shorten them.
pub const NATIVE_NAME_DISTANCE: f32 = NATIVE_SPAWN_DISTANCE;
pub const MIN_NAME_DISTANCE: f32 = 20.;
/// Height of one carried name above the actor's feet, used when a rig exposes no
/// usable bind-pose extent.
const NAME_FALLBACK_HEIGHT: f32 = 1.9;
/// Clearance between the top of the head and the baseline of the name.
const NAME_LIFT: f32 = 0.25;
/// How far above the bare terrain a placement may be lifted onto a building
/// floor. Bridge decks and interior floors sit well inside this; a rooftop far
/// overhead is not a placement surface and is not reached.
const MAX_FLOOR_LIFT: f32 = 16.;
/// Horizontal half-width and half-height of the AABB a placement stands in. Only
/// the vertical half-extent changes the support result, and a humanoid box is
/// what the client uses for its own actors.
const ACTOR_EXTENTS: Vec3 = Vec3::new(0.4, 0.9, 0.4);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// World-space radius around the camera that keeps actors spawned.
    pub spawn_distance: f32,
    /// Hard cap on simultaneously drawn actors.
    pub max_visible: usize,
    /// Play the template's own `休闲` scene action.
    pub idle_animation: bool,
    /// Draw the template's own name above the actor, as the game does.
    pub show_names: bool,
    /// Distance at which overhead names are still drawn.
    pub name_distance: f32,
    /// Resolve a placement against real BMD floors (bridge decks, interior
    /// floors, walls) before falling back to the terrain sample.
    pub stand_on_objects: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            spawn_distance: NATIVE_SPAWN_DISTANCE,
            max_visible: DEFAULT_MAX_VISIBLE,
            idle_animation: true,
            show_names: true,
            name_distance: NATIVE_NAME_DISTANCE,
            stand_on_objects: true,
        }
    }
}

impl Settings {
    fn sanitized(self) -> Self {
        Self {
            spawn_distance: if self.spawn_distance.is_finite() {
                self.spawn_distance
                    .clamp(MIN_SPAWN_DISTANCE, MAX_SPAWN_DISTANCE)
            } else {
                NATIVE_SPAWN_DISTANCE
            },
            max_visible: self.max_visible.clamp(1, MAX_VISIBLE_LIMIT),
            name_distance: if self.name_distance.is_finite() {
                self.name_distance
                    .clamp(MIN_NAME_DISTANCE, MAX_SPAWN_DISTANCE)
            } else {
                NATIVE_NAME_DISTANCE
            },
            ..self
        }
    }
}

/// Real building hulls, borrowed by the host for one frame. The Populator only
/// asks where the floor is; the Walker module owns the geometry and the loader.
pub trait FloorSource {
    /// Lowest walkable building floor at or above `feet`, if the column holds
    /// one within `max_up`.
    fn lowest_floor(&self, feet: Vec3, extents: Vec3, max_up: f32) -> Option<f32>;
    /// Changes whenever more hulls have streamed in, so placements resolved
    /// against an incomplete world can be resolved again.
    fn floor_stamp(&self) -> u64;
}

/// One action clip wrapped on its own clock.
///
/// `ModelAsset::pose_action_at_time` takes absolute track time and clamps past
/// the last key, so an ever-growing clock freezes a client idle after a single
/// cycle - which is exactly what a rig without a schedulable combined action
/// did. Wrapping the clock inside the clip's own authored frame range keeps it
/// looping for as long as the actor stands there.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clip {
    start_ms: u32,
    length_ms: u32,
}

impl Clip {
    fn new(asset: &ModelAsset, action: usize) -> Option<Self> {
        let clip = asset.model.actions.get(action)?;
        let fps = clip
            .fps
            .filter(|fps| *fps > 0)
            .unwrap_or(asset.skeleton.fps);
        if fps <= 0
            || !clip.start.is_finite()
            || !clip.end.is_finite()
            || clip.start < 0.
            || clip.end < clip.start
        {
            return None;
        }
        // Same millisecond range as `Playback::new`: the client truncates both
        // endpoints and treats the clip as end - start + 1 milliseconds.
        let start = (clip.start * 1000. / fps as f32).trunc();
        let end = (clip.end * 1000. / fps as f32).trunc();
        let length = end - start + 1.;
        if !length.is_finite() || length < 1. || end >= u32::MAX as f32 {
            return None;
        }
        Some(Self {
            start_ms: start as u32,
            length_ms: length as u32,
        })
    }

    fn seconds(&self, clock: f32) -> f32 {
        if !clock.is_finite() || clock <= 0. {
            return self.start_ms as f32 * 0.001;
        }
        let elapsed = (clock as f64 * 1000.) as u64 % self.length_ms as u64;
        (self.start_ms as u64 + elapsed) as f32 * 0.001
    }
}

/// Module-owned `models.pck`/`textures.pck` rig and texture source.
struct Library {
    models: Archive,
    textures: Option<Archive>,
    tracks: TrackCache,
    assets: HashMap<String, Rc<ModelAsset>>,
    failures: HashMap<String, String>,
    /// Bind-pose head height per rig, for the overhead name. Computed once per
    /// template rather than once per actor.
    heads: HashMap<String, f32>,
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
            heads: HashMap::new(),
        })
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        read_archive(&mut self.models, self.textures.as_mut(), path)
    }

    /// Height of the rig's head above its feet in the authored bind pose. A rig
    /// whose bind pose cannot be evaluated reports the humanoid default and does
    /// not fail the actor; only the name's baseline moves.
    fn head(&mut self, key: &str, asset: &ModelAsset) -> f32 {
        if let Some(height) = self.heads.get(key) {
            return *height;
        }
        let height = asset
            .pose_bind(Mat4::IDENTITY)
            .ok()
            .and_then(|meshes| {
                meshes.iter().fold(None::<f32>, |best, mesh| {
                    mesh.vertices.iter().fold(best, |best, vertex| {
                        let y = vertex.position[1];
                        if !y.is_finite() {
                            return best;
                        }
                        Some(best.map_or(y, |best: f32| best.max(y)))
                    })
                })
            })
            .filter(|height| (0.5..=8.0).contains(height))
            .unwrap_or(NAME_FALLBACK_HEIGHT);
        self.heads.insert(key.to_string(), height);
        height
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
            heads: _,
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
    /// Wrapped clip range for `action`, on the module clock.
    clip: Option<Clip>,
    /// Fallback clock in seconds for actors without a `Playback`.
    clock: f32,
    /// Height of this rig's head above its feet, for the overhead name.
    head: f32,
    /// Whether this actor stands on a building floor rather than the terrain.
    on_floor: bool,
    /// Building-hull stamp the placement was resolved against.
    floor_stamp: u64,
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
            None => (
                self.action,
                self.clip
                    .map_or(self.clock, |clip| clip.seconds(self.clock)),
            ),
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

/// The height a placement stands at.
///
/// `task_npc.data` authors 0 for "the server supplies the height"; any other
/// value is the absolute height the game uses. Without an authored height the
/// actor stands on the lowest building floor above the terrain sample - a bridge
/// deck, a platform or an interior floor - and on the terrain when the column
/// holds no floor at all. That is what keeps the client's own placements out of
/// the bridge arches and the walls they were authored on top of.
fn stand_height(
    spawn: &Spawn,
    terrain: f32,
    floors: Option<&dyn FloorSource>,
    enabled: bool,
) -> f32 {
    if let Some(height) = spawn.height {
        return height;
    }
    if !enabled {
        return terrain;
    }
    let Some(floors) = floors else {
        return terrain;
    };
    floors
        .lowest_floor(
            Vec3::new(spawn.anchor[0], terrain, spawn.anchor[1]),
            ACTOR_EXTENTS,
            MAX_FLOOR_LIFT,
        )
        .unwrap_or(terrain)
}

/// Screen position of a world point under the frame's view-projection, or `None`
/// when the point is behind the eye or outside the viewport. The matrix is the
/// host's own world-pass matrix, so a label lands exactly over its actor.
fn project(point: Vec3, view_projection: Mat4, screen: egui::Rect) -> Option<egui::Pos2> {
    let clip = view_projection * point.extend(1.);
    // Behind the eye or on the near plane: nothing to project.
    if clip.w <= 0.001 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !(-1.0..=1.0).contains(&ndc.x) || !(-1.0..=1.0).contains(&ndc.y) {
        return None;
    }
    Some(egui::pos2(
        screen.left() + (ndc.x * 0.5 + 0.5) * screen.width(),
        screen.top() + (0.5 - ndc.y * 0.5) * screen.height(),
    ))
}

/// The template's own scene idle. `EcModel::scene_action` is the client's
/// 0x5b3a4f lookup, including its `休闲` fallback (0x95a750).
///
/// `Playback` only schedules a combined action that is a single base starting at
/// zero with an infinite or fixed loop count; anything else is the client's
/// event/transition scheduler, which this viewer does not implement. Those rigs
/// still have the base clip the combined action names, so that clip runs on the
/// module clock instead of the actor falling back to its bind pose.
fn idle_clock(
    asset: &ModelAsset,
    report: &mut IdleReport,
) -> (Option<usize>, Option<Clip>, Option<Playback>) {
    let combined = asset.descriptor.scene_action("休闲");
    if let Ok(playback) = Playback::new(asset, combined) {
        report.combined += 1;
        return (None, None, Some(playback));
    }
    let base = combined
        .and_then(|index| asset.descriptor.actions.get(index))
        .and_then(|combined| combined.bases.first())
        .map(|base| base.name.clone());
    let action = base
        .and_then(|name| {
            asset
                .model
                .actions
                .iter()
                .position(|action| action.name == name)
        })
        .or_else(|| {
            // No combined scene action at all: the rig's own idle clip by the
            // names the client's templates use.
            const IDLE_NAMES: [&str; 4] = ["待机", "站立", "休闲", "idle"];
            IDLE_NAMES.iter().find_map(|name| {
                asset
                    .model
                    .actions
                    .iter()
                    .position(|action| action.name.eq_ignore_ascii_case(name))
            })
        });
    let Some(action) = action else {
        report.bind_pose += 1;
        return (None, None, None);
    };
    match Clip::new(asset, action) {
        Some(clip) => {
            report.looped += 1;
            (Some(action), Some(clip), None)
        }
        None => {
            // A single-frame or malformed range: nothing to animate.
            report.bind_pose += 1;
            (None, None, None)
        }
    }
}

/// How the population's idle animations resolved, reported once per session so a
/// frozen rig is diagnosable instead of a guess.
#[derive(Clone, Copy, Debug, Default)]
struct IdleReport {
    combined: usize,
    looped: usize,
    bind_pose: usize,
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
    /// One-shot camera move the settings window can ask the host for.
    camera_request: Option<Vec3>,
    /// Console diagnostics already emitted, so a stuck rig is reported once.
    reported_spawn: bool,
    reported_failures: usize,
    /// How the idle animations resolved, and whether that was logged.
    idle_report: IdleReport,
    reported_idle: bool,
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
            camera_request: None,
            reported_spawn: false,
            reported_failures: 0,
            idle_report: IdleReport::default(),
            reported_idle: false,
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

    /// One-shot camera move requested by the settings window. The host owns the
    /// camera, so the module only asks.
    pub fn take_camera_request(&mut self) -> Option<Vec3> {
        self.camera_request.take()
    }

    /// Camera pose showing the nearest real placement, for the settings
    /// window's "go there" button: the module cannot verify what it cannot
    /// reach, and most of the map genuinely has no task NPC nearby.
    fn nearest_camera_target(&self) -> Option<Vec3> {
        let population = self.population.as_ref()?;
        let map = self.map.as_ref()?;
        let nearest = population.nearest([self.eye.x, self.eye.z], f32::MAX, 1);
        let (index, _) = nearest.first().copied()?;
        let spawn = population.spawns.get(index)?;
        let ground = spawn
            .height
            .unwrap_or_else(|| map.ground(spawn.anchor[0], spawn.anchor[1]));
        Some(Vec3::new(
            spawn.anchor[0] + 30.,
            ground + 6.,
            spawn.anchor[1] + 30.,
        ))
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
            Some(population) => {
                let in_range =
                    population.count_within([self.eye.x, self.eye.z], self.settings.spawn_distance);
                format!(
                    "{} client-placed NPCs ({} records total, {} server-placed): {} in range, {} drawn",
                    population.spawns.len(),
                    population.spawns.len() + population.unplaced + population.skipped,
                    population.unplaced,
                    in_range,
                    self.live.len()
                )
            }
            None => "Loading the real population...".into(),
        }
    }

    /// Whether this module needs the host's building hulls this frame.
    pub fn wants_floors(&self) -> bool {
        self.active && self.settings.stand_on_objects
    }

    /// Per-frame host hook: streams actors around the camera, resolves their
    /// placement against the host's building floors and advances the ones
    /// standing on a navigation path.
    pub fn update(&mut self, delta: f32, eye: Vec3, floors: Option<&dyn FloorSource>) {
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
            self.idle_report = IdleReport::default();
            self.reported_idle = false;
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.forget_textures();
            }
        }
        if self.map.is_none() {
            return;
        }
        if self.population.is_none() && self.load_error.is_none() {
            match Population::open(&self.element) {
                Ok(population) => {
                    eprintln!(
                        "Populator: {} client-placed NPCs, {} server-placed records, {} skipped",
                        population.spawns.len(),
                        population.unplaced,
                        population.skipped
                    );
                    self.population = Some(population);
                }
                Err(error) => {
                    eprintln!("Populator: population unavailable: {error}");
                    self.load_error = Some(error);
                }
            }
        }
        if self.population.is_none() {
            return;
        }
        if self.library.is_none() && self.library_error.is_none() {
            match Library::open(&self.element) {
                Ok(library) => self.library = Some(library),
                Err(error) => {
                    eprintln!("Populator: models.pck unavailable: {error}");
                    self.library_error = Some(error);
                }
            }
        }
        if self.library.is_none() {
            return;
        }
        self.stream(floors);
        self.advance(delta, floors);
        self.report();
    }

    /// Console diagnostics, so a user who never opens the settings window still
    /// sees whether the module is streaming and which rigs could not load.
    fn report(&mut self) {
        if !self.reported_spawn && !self.live.is_empty() {
            self.reported_spawn = true;
            eprintln!(
                "Populator: streaming {} actors within {} units",
                self.live.len(),
                self.settings.spawn_distance
            );
        }
        let failed = self
            .library
            .as_ref()
            .map_or(0, |library| library.failures.len());
        if let Some(library) = self.library.as_ref() {
            if failed > self.reported_failures {
                for (path, error) in library.failures.iter().skip(self.reported_failures).take(5) {
                    eprintln!("Populator: rig {path}: {error}");
                }
                self.reported_failures = failed;
            }
        }
        if !self.reported_idle && !self.live.is_empty() {
            self.reported_idle = true;
            eprintln!(
                "Populator: idle animation - {} combined actions scheduled, {} clips looped on the module clock, {} rigs left in their bind pose",
                self.idle_report.combined, self.idle_report.looped, self.idle_report.bind_pose
            );
        }
    }

    /// Reconciles the drawn set with the placements inside the spawn distance.
    /// Actors that leave the radius are dropped, so the module never holds rigs
    /// it is not drawing.
    fn stream(&mut self, floors: Option<&dyn FloorSource>) {
        let nearest = match self.population.as_ref() {
            Some(population) => population.nearest(
                [self.eye.x, self.eye.z],
                self.settings.spawn_distance,
                self.settings.max_visible,
            ),
            None => return,
        };
        let mut previous: Vec<Option<Live>> = std::mem::take(&mut self.live)
            .into_iter()
            .map(Some)
            .collect();
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
            if let Some(live) = self.spawn_instance(index, floors) {
                next.push(live);
            }
        }
        self.live = next;
    }

    fn spawn_instance(&mut self, index: usize, floors: Option<&dyn FloorSource>) -> Option<Live> {
        let spawn = self.population.as_ref()?.spawns.get(index)?.clone();
        let (position, on_floor) = self.position_for(&spawn, floors);
        // No client file records an NPC facing: `task_npc.data` stores id, X,
        // Z, height and task count, and the `NPC_ESSENCE` record has no
        // rotation column either. Every placement therefore keeps the rig's own
        // authored forward axis, which is what the client falls back to.
        let yaw = 0.;
        let follow = self.route_for(&spawn);
        let key = crate::ecmodel::normalize(&spawn.model);
        let library = self.library.as_mut()?;
        let asset = library.asset(&spawn.model).ok()?;
        let head = library.head(&key, asset.as_ref());
        let (action, clip, playback) = if self.settings.idle_animation {
            idle_clock(asset.as_ref(), &mut self.idle_report)
        } else {
            (None, None, None)
        };
        Some(Live {
            spawn: index,
            position,
            yaw,
            asset,
            action,
            playback,
            clip,
            clock: 0.,
            head,
            on_floor,
            floor_stamp: floors.map_or(0, |floors| floors.floor_stamp()),
            follow,
            draw: None,
            failure: None,
        })
    }

    /// World position of one placement, and whether it stands on a building
    /// floor rather than the terrain sample.
    fn position_for(&self, spawn: &Spawn, floors: Option<&dyn FloorSource>) -> (Vec3, bool) {
        let terrain = match self.map.as_ref() {
            Some(map) => map.ground(spawn.anchor[0], spawn.anchor[1]),
            None => 0.,
        };
        let height = stand_height(spawn, terrain, floors, self.settings.stand_on_objects);
        let on_floor = spawn.height.is_none() && height > terrain + 0.5;
        (
            Vec3::new(spawn.anchor[0], height, spawn.anchor[1]),
            on_floor,
        )
    }

    fn route_for(&self, spawn: &Spawn) -> Option<Follow> {
        let route = self.population.as_ref()?.routes.get(spawn.route?)?;
        Follow::new(route).ok()
    }

    fn advance(&mut self, delta: f32, floors: Option<&dyn FloorSource>) {
        if !delta.is_finite() || delta <= 0. {
            return;
        }
        self.resolve_floors(floors);
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
                } else if playback.stopped() {
                    // A scene actor re-triggers its idle: the template's own
                    // loop count schedules one pass, it does not mean the actor
                    // holds the last frame for the rest of the session.
                    playback.restart();
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

    /// Re-resolves placements the host's hull loader has caught up with.
    ///
    /// Building hulls stream in from `models.pck` after the first frame, so an
    /// actor spawned during loading may have resolved against an incomplete
    /// world and be standing on the terrain under a bridge. The stamp changes
    /// with every batch; only the actors resolved against an older one are
    /// moved again, and actors with an authored height never are.
    fn resolve_floors(&mut self, floors: Option<&dyn FloorSource>) {
        if !self.settings.stand_on_objects {
            return;
        }
        let Some(floors) = floors else {
            return;
        };
        let stamp = floors.floor_stamp();
        let Some(map) = self.map.as_ref() else {
            return;
        };
        let Some(population) = self.population.as_ref() else {
            return;
        };
        for live in &mut self.live {
            if live.floor_stamp == stamp {
                continue;
            }
            live.floor_stamp = stamp;
            let Some(spawn) = population.spawns.get(live.spawn) else {
                continue;
            };
            // An authored height is the game's own answer and never moves.
            if spawn.height.is_some() {
                continue;
            }
            let terrain = map.ground(spawn.anchor[0], spawn.anchor[1]);
            let height = stand_height(spawn, terrain, Some(floors), true);
            live.position.y = height;
            live.on_floor = height > terrain + 0.5;
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

    /// Actors standing on a real building floor instead of the terrain, so the
    /// settings window can show whether the support query actually engaged.
    fn on_floor_count(&self) -> usize {
        self.live.iter().filter(|live| live.on_floor).count()
    }

    fn failed_count(&self) -> usize {
        self.live
            .iter()
            .filter(|live| live.failure.is_some())
            .count()
    }

    /// Whether overhead names will be painted this frame, so the host only runs
    /// its egui pass when there is something to draw.
    pub fn names_active(&self) -> bool {
        self.active
            && self.settings.show_names
            && !self.live.is_empty()
            && self.population.is_some()
    }

    /// Client-style overhead names: the template's own name, projected over the
    /// actor's head in world space, faded with distance and shadowed the way the
    /// client's own labels are. The layer sits above the 3D scene and below the
    /// viewer's windows, so a menu still covers the labels.
    pub fn overlay_ui(&mut self, ctx: &egui::Context, view_projection: Mat4, screen: egui::Rect) {
        if !self.names_active() || self.settings.name_distance <= 0. {
            return;
        }
        let Some(population) = self.population.as_ref() else {
            return;
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("populator-nameplates"),
        ));
        let eye = self.eye;
        let range = self.settings.name_distance;
        let fade_from = range * 0.75;
        for live in &self.live {
            if live.failure.is_some() {
                continue;
            }
            let Some(spawn) = population.spawns.get(live.spawn) else {
                continue;
            };
            if spawn.name.is_empty() {
                continue;
            }
            let distance = (live.position - eye).length();
            if distance > range {
                continue;
            }
            let anchor = live.position + Vec3::Y * (live.head + NAME_LIFT);
            let Some(position) = project(anchor, view_projection, screen) else {
                continue;
            };
            let alpha = if distance <= fade_from {
                1.0
            } else {
                ((range - distance) / (range - fade_from)).clamp(0., 1.)
            };
            let font = egui::FontId::proportional(if distance < 40. { 14. } else { 12. });
            painter.text(
                position + egui::vec2(1., 1.),
                egui::Align2::CENTER_BOTTOM,
                &spawn.name,
                font.clone(),
                egui::Color32::from_black_alpha((alpha * 190.) as u8),
            );
            painter.text(
                position,
                egui::Align2::CENTER_BOTTOM,
                &spawn.name,
                font,
                egui::Color32::from_white_alpha((alpha * 245.) as u8),
            );
        }
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Populator");
        ui.label(egui::RichText::new(self.status()).small().weak());
        ui.separator();

        ui.heading("Real population");
        let mut go_to_nearest = false;
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
                let records = population.spawns.len() + population.unplaced + population.skipped;
                ui.label(format!(
                    "{npcs} client-placed NPCs, {monsters} monster placements, {linked} with task links, {} server-placed, {} skipped",
                    population.unplaced, population.skipped
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "task_npc.data: {} bytes, timestamp 0x{:08x}, {records} records",
                        population.table_size, population.table_timestamp
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
                let in_range =
                    population.count_within([self.eye.x, self.eye.z], self.settings.spawn_distance);
                if in_range == 0 {
                    let server_placed = if records > 0 {
                        100. * population.unplaced as f32 / records as f32
                    } else {
                        0.
                    };
                    ui.colored_label(
                        egui::Color32::from_rgb(240, 190, 80),
                        format!(
                            "Nothing is placed within {:.0} units of the camera. {server_placed:.0}% of this table is server-placed, and the client's own placements cluster in towns: widen the spawn distance, or use the button below.",
                            self.settings.spawn_distance
                        ),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!(
                            "{in_range} placements within {:.0} units of the camera",
                            self.settings.spawn_distance
                        ))
                        .small()
                        .weak(),
                    );
                }
                if ui.button("Go to the nearest placement").clicked() {
                    go_to_nearest = true;
                }
            }
            (None, None) => {
                ui.label("Reading elements.data and task_npc.data...");
            }
        }
        if go_to_nearest {
            if let Some(target) = self.nearest_camera_target() {
                self.camera_request = Some(target);
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
            egui::Slider::new(&mut self.settings.max_visible, 1..=MAX_VISIBLE_LIMIT).text("actors"),
        );
        ui.checkbox(&mut self.settings.idle_animation, "Idle animation")
            .on_hover_text(
                "Play the template's own `休闲` scene action. The client's combined scene action is used where it can be scheduled; otherwise the clip that combined action names loops on the module clock, and only a rig with neither stays in its bind pose.",
            );
        ui.checkbox(&mut self.settings.stand_on_objects, "Stand on building floors")
            .on_hover_text(
                "Resolve each placement against the client's own BMD collision hulls, so an NPC authored onto a bridge deck, platform or interior floor stands there instead of on the terrain under the geometry. Uses the same hulls as the Walker module's collision check and loads them when this module is enabled.",
            );
        ui.checkbox(&mut self.settings.show_names, "Names above heads")
            .on_hover_text("Draw each template's own name over the actor, as the client does.");
        ui.add_enabled(
            self.settings.show_names,
            egui::Slider::new(
                &mut self.settings.name_distance,
                MIN_NAME_DISTANCE..=MAX_SPAWN_DISTANCE,
            )
            .text("name distance")
            .suffix(" u"),
        );
        self.settings = self.settings.sanitized();
        ui.label(
            egui::RichText::new(format!(
                "{} drawn, {} on building floors, {} failed, {} rigs cached, {} textures cached",
                self.drawn_count(),
                self.on_floor_count(),
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
            let moving = self
                .live
                .iter()
                .filter(|live| live.follow.is_some())
                .count();
            ui.label(
                egui::RichText::new(&population.mob_placement)
                    .small()
                    .weak(),
            );
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
        ui.label(
            egui::RichText::new(
                "Facing is server data. task_npc.data stores id, X, Z, height and task count in a fixed 12-byte record, and the NPC_ESSENCE record has no rotation column, so every placement keeps the rig's own authored forward axis.",
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
        assert!(settings.show_names);
        assert!(settings.stand_on_objects);
        assert_eq!(settings.name_distance, NATIVE_NAME_DISTANCE);
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

    /// A stand-in for the Walker's building hulls: one floor at a fixed height.
    struct OneFloor(Option<f32>, u64);

    impl FloorSource for OneFloor {
        fn lowest_floor(&self, _feet: Vec3, _extents: Vec3, _max_up: f32) -> Option<f32> {
            self.0
        }

        fn floor_stamp(&self) -> u64 {
            self.1
        }
    }

    #[test]
    fn authored_height_wins_and_a_real_floor_beats_the_terrain() {
        let none = OneFloor(None, 1);
        let bridge = OneFloor(Some(146.), 1);
        // An authored height is the game's own value, never the terrain.
        assert_eq!(
            stand_height(&spawn(Some(276.)), 120., Some(&bridge), true),
            276.
        );
        // "0" means the server's height: trust a real floor above the terrain.
        assert_eq!(stand_height(&spawn(None), 120., Some(&bridge), true), 146.);
        // No floor in the column, or the option off, keeps the terrain.
        assert_eq!(stand_height(&spawn(None), 120., Some(&none), true), 120.);
        assert_eq!(stand_height(&spawn(None), 120., Some(&bridge), false), 120.);
        assert_eq!(stand_height(&spawn(None), 120., None, true), 120.);
    }

    #[test]
    fn an_idle_clip_wraps_inside_its_own_authored_range() {
        // Half a second into the clip, one second long: the sampled time must
        // stay inside 500..1500 ms however long the actor stands there, because
        // the sampler clamps past the last key instead of wrapping.
        let clip = Clip {
            start_ms: 500,
            length_ms: 1_000,
        };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(clip.seconds(0.), 0.5));
        assert!(close(clip.seconds(0.25), 0.75));
        assert!(close(clip.seconds(1.0), 0.5));
        assert!(close(clip.seconds(1.5), 1.0));
        assert!(close(clip.seconds(-1.), 0.5));
        // Long-running actors wrap instead of freezing on the last key.
        assert!(close(clip.seconds(3_601.25), 0.75));
    }

    #[test]
    fn a_world_point_projects_to_its_own_screen_position() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.));
        // An identity matrix projects clip space straight to the screen, so the
        // view axis lands in the middle and a point above it lands on the top
        // edge: the mapping a label uses for the actor's head.
        let centre = project(Vec3::new(0., 0., 0.5), Mat4::IDENTITY, screen)
            .expect("view-axis point is visible");
        assert!((centre.x - 400.).abs() < 1e-3);
        assert!((centre.y - 300.).abs() < 1e-3);
        let above = project(Vec3::new(0., 1., 0.5), Mat4::IDENTITY, screen).expect("visible");
        assert!((above.y - 0.).abs() < 1e-3);
        let left = project(Vec3::new(-0.5, 0., 0.5), Mat4::IDENTITY, screen).expect("visible");
        assert!((left.x - 200.).abs() < 1e-3);
        // Off screen and behind the eye are not labels to draw.
        assert!(project(Vec3::new(1.5, 0., 0.5), Mat4::IDENTITY, screen).is_none());
        let behind = Mat4::from_cols_array_2d(&[
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., -1.],
            [0., 0., 0., 0.],
        ]);
        assert!(project(Vec3::new(0., 0., 1.), behind, screen).is_none());
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
        populator.update(0.016, Vec3::ZERO, None);
        assert!(populator.live.is_empty());
        assert_eq!(populator.status(), "Waiting for a loaded world");
    }
}
