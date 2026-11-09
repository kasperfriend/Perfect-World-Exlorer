//! Live logging the viewer can be told to do while it runs.
//!
//! Every subsystem that has ever needed a report while debugging a scene —
//! effect roots, card geometry, UV windows, textures, alpha, the ground drape,
//! model elements, lights, culling, the scene's own placements, the render
//! passes, timing — has a named *channel* here. Channels are off unless the user
//! ticks them in **Settings ▸ Logging**, or unless `PWE_LOG` names them, and the
//! cost of a disabled channel is one relaxed atomic load per call site.
//!
//! Three sinks, each with a different job:
//!
//! * **a file per channel** in `<client root>/pwe-logs/`, appended and rotated
//!   at 8 MiB, so a session can be read after the fact and handed over whole;
//! * **a rolling tail** of the last few hundred lines, which the settings tab
//!   shows live so a defect can be watched while it happens;
//! * **per-channel counters** (lines written, lines suppressed by the throttle),
//!   so "no output" can be told apart from "nothing happened".
//!
//! A channel with a throttle drops lines instead of buffering them: the point of
//! the throttle is to keep a 20 ms effect frame from turning a 1024-particle
//! scene into a 1024-line-per-frame log. DROPPED lines are counted, and the
//! counts are shown next to each checkbox.
//!
//! The module owns no game state. Call sites decide what a line says; this file
//! only decides whether it is written, where, and how often.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Which page of the Logging tab a channel is listed under. The subtabs are the
/// client's own seam: effects, the world that places them, the models inside
/// them, the passes that draw them, and the clock everything runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Effects,
    Scene,
    Models,
    Render,
    Timing,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Self::Effects,
        Self::Scene,
        Self::Models,
        Self::Render,
        Self::Timing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Effects => "Effects",
            Self::Scene => "Scene",
            Self::Models => "Models",
            Self::Render => "Render",
            Self::Timing => "Timing",
        }
    }

    /// One line about what the page covers, shown under the subtabs.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Effects => {
                "The animated effect graph: roots, elements, cards, UV windows, \
                 textures, colour, particles, trails and lights."
            }
            Self::Scene => {
                "What the world places: effect records, ornaments, water, terrain \
                 and the sky, all as the files describe them."
            }
            Self::Models => {
                "Effect models and world models: what was asked for, what loaded, \
                 what was rejected and why."
            }
            Self::Render => {
                "The passes themselves: how many quads and instances reached each \
                 one, what was culled, and what the GPU was given."
            }
            Self::Timing => {
                "Clocks and budgets: frame time, effect clocks, play speed, load and \
                 memory."
            }
        }
    }
}

/// A named stream of log lines. The discriminants are the bit positions of
/// [`mask`], so the list can grow to 64 without any other file changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    // ---- Effects: the animated graph itself --------------------------------
    /// Roots loaded, activated, released; the record behind them.
    EffectRoots = 0,
    /// The element table of each root: id, name, flags, keypoint count.
    EffectElements = 1,
    /// One line per element per throttle window: keypoint, basis, transform.
    EffectFrames = 2,
    /// Every card that was emitted: corners, centre, size and which basis.
    EffectCards = 3,
    /// Flipbook and tile windows: mode, grid, interval, speed, clock, corners.
    EffectUv = 4,
    /// Texture resolution: the authored name, the archive path, hit or miss.
    EffectTextures = 5,
    /// Colour and alpha before and after every fold, with the cutoffs.
    EffectAlpha = 6,
    /// The ground drape: entry test, cell grid, per-vertex heights, misses.
    EffectGround = 7,
    /// Particle emitters: spawns, deaths, volume, direction, speed.
    EffectParticles = 8,
    /// Trails, ribbons, fractals, waves and lightning segments.
    EffectTrails = 9,
    /// Model elements (160/161): file, load result, placement.
    EffectModels = 10,
    /// Lights a root publishes: position, colour, radius, flicker.
    EffectLights = 11,
    /// Why an owner is or is not drawn: radius, alpha, flags, budget.
    EffectCulling = 12,
    /// Element and root clocks: start, loop, end, play speed, bind state.
    EffectClock = 13,
    /// The 2D decal path (100/101/102/190) and the screen-space branch.
    EffectDecals = 14,
    /// Failures and warnings that are not fatal, in the order they happened.
    EffectProblems = 15,
    // ---- Scene: what the map places ----------------------------------------
    /// `world.ecwld` effect records as parsed: path, block, placement.
    SceneEffects = 16,
    /// Ornament and scenery records: model, placement, scale, flips.
    SceneOrnaments = 17,
    /// Water areas, surfaces and the records behind them.
    SceneWater = 18,
    /// Terrain blocks: residency, level, window, texture pages.
    SceneTerrain = 19,
    /// Fog, sky, sun, weather and the region data driving them.
    SceneSky = 20,
    /// Sounds the map places and which of them the viewer can play.
    SceneSounds = 21,
    // ---- Models ------------------------------------------------------------
    /// Model requests: path, size, parse result.
    ModelLoads = 22,
    /// Materials, textures and bone sets of a loaded model.
    ModelMaterials = 23,
    /// Animation playback, poses and attachment events.
    ModelAnimation = 24,
    /// The model queue: admitted, pending, rejected, one per frame.
    ModelQueue = 25,
    // ---- Render ------------------------------------------------------------
    /// Effect draws: quads submitted per pass, batch counts, buffer sizes.
    RenderEffects = 26,
    /// World/scenery/terrain draws and the culling decisions behind them.
    RenderWorld = 27,
    /// Water and shore passes.
    RenderWater = 28,
    /// Bloom, fog, gamma and the post chain.
    RenderPost = 29,
    /// Texture uploads and the decoded-texture caches.
    RenderTextures = 30,
    /// Frame timing per stage and the frame budget.
    TimingFrame = 31,
    /// Load timing: world, terrain, models, effects.
    TimingLoad = 32,
    /// Resident bytes and cache occupancy.
    TimingMemory = 33,
    /// Input, camera and navigation state once per second.
    TimingCamera = 34,
    /// Audio: zone track, ambience, effect sounds.
    AudioState = 35,
}

pub const CHANNELS: usize = 36;

impl Channel {
    pub const ALL: [Channel; CHANNELS] = [
        Self::EffectRoots,
        Self::EffectElements,
        Self::EffectFrames,
        Self::EffectCards,
        Self::EffectUv,
        Self::EffectTextures,
        Self::EffectAlpha,
        Self::EffectGround,
        Self::EffectParticles,
        Self::EffectTrails,
        Self::EffectModels,
        Self::EffectLights,
        Self::EffectCulling,
        Self::EffectClock,
        Self::EffectDecals,
        Self::EffectProblems,
        Self::SceneEffects,
        Self::SceneOrnaments,
        Self::SceneWater,
        Self::SceneTerrain,
        Self::SceneSky,
        Self::SceneSounds,
        Self::ModelLoads,
        Self::ModelMaterials,
        Self::ModelAnimation,
        Self::ModelQueue,
        Self::RenderEffects,
        Self::RenderWorld,
        Self::RenderWater,
        Self::RenderPost,
        Self::RenderTextures,
        Self::TimingFrame,
        Self::TimingLoad,
        Self::TimingMemory,
        Self::TimingCamera,
        Self::AudioState,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn page(self) -> Page {
        match self {
            Self::EffectRoots
            | Self::EffectElements
            | Self::EffectFrames
            | Self::EffectCards
            | Self::EffectUv
            | Self::EffectTextures
            | Self::EffectAlpha
            | Self::EffectGround
            | Self::EffectParticles
            | Self::EffectTrails
            | Self::EffectModels
            | Self::EffectLights
            | Self::EffectCulling
            | Self::EffectClock
            | Self::EffectDecals
            | Self::EffectProblems => Page::Effects,
            Self::SceneEffects
            | Self::SceneOrnaments
            | Self::SceneWater
            | Self::SceneTerrain
            | Self::SceneSky
            | Self::SceneSounds => Page::Scene,
            Self::ModelLoads | Self::ModelMaterials | Self::ModelAnimation | Self::ModelQueue => {
                Page::Models
            }
            Self::RenderEffects
            | Self::RenderWorld
            | Self::RenderWater
            | Self::RenderPost
            | Self::RenderTextures => Page::Render,
            Self::TimingFrame
            | Self::TimingLoad
            | Self::TimingMemory
            | Self::TimingCamera
            | Self::AudioState => Page::Timing,
        }
    }

    /// Stable file name and settings token. Changing one of these renames a log
    /// file and forgets a saved tick, so they are plain ASCII and never reused.
    pub fn key(self) -> &'static str {
        match self {
            Self::EffectRoots => "effect-roots",
            Self::EffectElements => "effect-elements",
            Self::EffectFrames => "effect-frames",
            Self::EffectCards => "effect-cards",
            Self::EffectUv => "effect-uv",
            Self::EffectTextures => "effect-textures",
            Self::EffectAlpha => "effect-alpha",
            Self::EffectGround => "effect-ground",
            Self::EffectParticles => "effect-particles",
            Self::EffectTrails => "effect-trails",
            Self::EffectModels => "effect-models",
            Self::EffectLights => "effect-lights",
            Self::EffectCulling => "effect-culling",
            Self::EffectClock => "effect-clock",
            Self::EffectDecals => "effect-decals",
            Self::EffectProblems => "effect-problems",
            Self::SceneEffects => "scene-effects",
            Self::SceneOrnaments => "scene-ornaments",
            Self::SceneWater => "scene-water",
            Self::SceneTerrain => "scene-terrain",
            Self::SceneSky => "scene-sky",
            Self::SceneSounds => "scene-sounds",
            Self::ModelLoads => "model-loads",
            Self::ModelMaterials => "model-materials",
            Self::ModelAnimation => "model-animation",
            Self::ModelQueue => "model-queue",
            Self::RenderEffects => "render-effects",
            Self::RenderWorld => "render-world",
            Self::RenderWater => "render-water",
            Self::RenderPost => "render-post",
            Self::RenderTextures => "render-textures",
            Self::TimingFrame => "timing-frame",
            Self::TimingLoad => "timing-load",
            Self::TimingMemory => "timing-memory",
            Self::TimingCamera => "timing-camera",
            Self::AudioState => "audio-state",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EffectRoots => "Roots: load, activate, release",
            Self::EffectElements => "Element table of each root",
            Self::EffectFrames => "Element frames: keypoint, basis, transform",
            Self::EffectCards => "Cards: corners, centre, size, basis",
            Self::EffectUv => "UV windows: flipbook and tile",
            Self::EffectTextures => "Textures: resolved, missing, blend pair",
            Self::EffectAlpha => "Colour and alpha folds",
            Self::EffectGround => "Ground drape: cells, heights, misses",
            Self::EffectParticles => "Particles: spawns, volume, direction",
            Self::EffectTrails => "Trails, ribbons, fractals, waves",
            Self::EffectModels => "Model elements: file and placement",
            Self::EffectLights => "Lights a root publishes",
            Self::EffectCulling => "Activation radius and visibility",
            Self::EffectClock => "Clocks, loops, play speed, binds",
            Self::EffectDecals => "2D and screen-space decal path",
            Self::EffectProblems => "Non-fatal effect failures and warnings",
            Self::SceneEffects => "Effect records from world.ecwld",
            Self::SceneOrnaments => "Ornaments and scenery records",
            Self::SceneWater => "Water areas and surfaces",
            Self::SceneTerrain => "Terrain blocks and residency",
            Self::SceneSky => "Sky, sun, fog and weather",
            Self::SceneSounds => "Sounds the map places",
            Self::ModelLoads => "Model requests and parse results",
            Self::ModelMaterials => "Model materials and textures",
            Self::ModelAnimation => "Model animation and attachments",
            Self::ModelQueue => "Model queue: admitted and rejected",
            Self::RenderEffects => "Effect draws and batches",
            Self::RenderWorld => "World and scenery draws",
            Self::RenderWater => "Water passes",
            Self::RenderPost => "Bloom, fog, gamma, post chain",
            Self::RenderTextures => "Texture uploads and caches",
            Self::TimingFrame => "Frame time per stage",
            Self::TimingLoad => "Load timing",
            Self::TimingMemory => "Resident bytes",
            Self::TimingCamera => "Camera and input state",
            Self::AudioState => "Audio state",
        }
    }

    /// The tooltip. Every channel says what its lines contain and what they are
    /// for, because the whole point is that the user can pick one without having
    /// to read the source first.
    pub fn help(self) -> &'static str {
        match self {
            Self::EffectRoots => {
                "Each effect root the world places, as the loader sees it: path, \
                 key, record position/direction/up/scale/alpha/play speed, the \
                 activation decision and the moment it is released. Start here \
                 when an effect is missing, doubled or in the wrong place."
            }
            Self::EffectElements => {
                "The element table of a root once it is built: element id, class, \
                 Texture, Width/Height, TexRow/TexCol/TexInterval/TexSpeed, blend \
                 pair, and the keypoint count. This is what pairs a screenshot \
                 artefact with the element that drew it."
            }
            Self::EffectFrames => {
                "One line per element per throttle window: the sampled keypoint \
                 position, direction, scale and rotation, plus the world basis the \
                 card is drawn in. Use it when a card is in the right place but \
                 the wrong way round."
            }
            Self::EffectCards => {
                "Every emitted card: the four world corners, the centre, the two \
                 edges and which basis built it (element, axial billboard or \
                 screen billboard). Obeys the throttle, so a 1024-particle root \
                 reports a sample per window instead of a flood."
            }
            Self::EffectUv => {
                "The UV window of each card: flipbook cell index or tile repeat \
                 count, the TexRow/TexCol grid, TexInterval, TexSpeed, the clock \
                 the window was taken from and the four UV corners."
            }
            Self::EffectTextures => {
                "Where a texture name resolved to, whether the archive had it, and \
                 the blend pair the element draws with. A missing texture is \
                 substituted with a 1x1 white pixel, which is what makes a card \
                 look like a solid colour."
            }
            Self::EffectAlpha => {
                "The colour and alpha folds: authored colour, root alpha, placement \
                 alpha, distance fade, the scene alpha clamp, the final RGBA and \
                 whether the card survived the cutoff."
            }
            Self::EffectGround => {
                "The ground branch of `Fill_Verts_3D`: whether the sheet qualified, \
                 the cell count and size, the heights sampled at the lattice, and \
                 why a drape fell back to the flat quad."
            }
            Self::EffectParticles => {
                "Particle emitters: spawn and death counts, the volume, emission \
                 direction and speed, the lifetime, colour track and grounded \
                 flag. Spawn *and* kill together, because a particle that never \
                 dies is a leak and one that dies at once is invisible."
            }
            Self::EffectTrails => {
                "Trail, ribbon, fractal, wave and lightning segments: how many are \
                 alive, their length and their colour ramp."
            }
            Self::EffectModels => {
                "Element 160/161 model loads: the model file named by the element, \
                 whether the archive had it, and the transform the element gives \
                 it relative to the root."
            }
            Self::EffectLights => {
                "Lights a root publishes for the scene: colour, position, radius, \
                 attenuation and flicker phase. Lights are published even when \
                 world effects are off, so this channel works with them disabled."
            }
            Self::EffectCulling => {
                "Why an owner is or is not drawn this frame: distance to the camera, \
                 activation radius, alpha, flags, whether the player failed to \
                 build and the one-per-frame load budget."
            }
            Self::EffectClock => {
                "Root and element clocks: elapsed milliseconds, play speed, loop \
                 flag, start and end times, repeat counts and bind state. This is \
                 the channel for anything that is frozen or runs at the wrong \
                 rate."
            }
            Self::EffectDecals => {
                "The 2D decal classes and the screen-space branch: which id took \
                 which path, the viewport it was laid out in and the corners it \
                 produced."
            }
            Self::EffectProblems => {
                "Every non-fatal effect failure and warning, with the root path and \
                 element id, in the order it happened. The status line only counts \
                 them; this says which one."
            }
            Self::SceneEffects => {
                "The `world.ecwld` effect records themselves: order in the block \
                 table, block, path, position, direction, up, scale, alpha and the \
                 day/night window. Compare with the effect-roots channel to see \
                 what the world asked for and what the loader did."
            }
            Self::SceneOrnaments => {
                "Ornament and scenery records: model, position, direction, up, \
                 scale, flip flags and the block they belong to."
            }
            Self::SceneWater => {
                "Water areas: rectangle, height, surface, type and the record behind \
                 them, plus whether the pass drew or skipped them."
            }
            Self::SceneTerrain => {
                "Terrain blocks: resident window, level chosen, texture pages and \
                 the LOD comparison between the mesh and the rules."
            }
            Self::SceneSky => {
                "Sky, sun, fog and weather: the area's flag word, the selected sky \
                 row, the fog colour and distance, and the sun direction in use."
            }
            Self::SceneSounds => {
                "Sounds placed by the map: zone, name, position and whether the \
                 viewer has a file for them."
            }
            Self::ModelLoads => {
                "Model requests in the order they were made: path, byte size, \
                 version, skin and bone counts, and the parse result."
            }
            Self::ModelMaterials => {
                "Materials of loaded models: texture set, blend, alpha test, \
                 lighting route and backface flags."
            }
            Self::ModelAnimation => {
                "Animation playback: action, frame, pose source, and the attachment \
                 events a model fires while it plays."
            }
            Self::ModelQueue => {
                "The world-model and effect loading queues: how many are pending, \
                 which root was admitted this frame, and which were rejected."
            }
            Self::RenderEffects => {
                "Effect draws: quads submitted, batches, instance counts and the \
                 buffer sizes the pass was given."
            }
            Self::RenderWorld => {
                "World, scenery and tree draws: instance counts per pass and what \
                 the culling said about them."
            }
            Self::RenderWater => "Water and shore passes: what was drawn and where.",
            Self::RenderPost => {
                "The post chain: which effects ran, their parameters and the target \
                 sizes."
            }
            Self::RenderTextures => {
                "Texture uploads and the decoded-texture caches: hits, misses, \
                 evictions and formats."
            }
            Self::TimingFrame => {
                "Frame time per stage, once a second: CPU frame, GPU submit, effect \
                 update, and the frame budget left."
            }
            Self::TimingLoad => {
                "Load timings: world load, terrain build, model preload and the \
                 effect graph, in milliseconds."
            }
            Self::TimingMemory => {
                "Resident bytes: terrain grid, texture caches, the effect graph and \
                 the model set."
            }
            Self::TimingCamera => {
                "Camera and input state once a second: position, yaw, distance, \
                 speed, and which menu page is open."
            }
            Self::AudioState => "Zone music, ambience and effect sounds.",
        }
    }

    /// Minimum milliseconds between two lines of this channel. Structural
    /// channels (loads, tables, placements) are 0 — they happen once. Per-frame
    /// channels are throttled so a heavy scene cannot turn logging into the
    /// bottleneck it is supposed to explain.
    pub fn throttle_ms(self) -> u32 {
        match self {
            Self::EffectRoots
            | Self::EffectElements
            | Self::EffectTextures
            | Self::EffectModels
            | Self::EffectProblems
            | Self::SceneEffects
            | Self::SceneOrnaments
            | Self::SceneSounds
            | Self::ModelLoads
            | Self::ModelMaterials
            | Self::ModelQueue
            | Self::TimingLoad => 0,
            Self::EffectCards
            | Self::EffectUv
            | Self::EffectParticles
            | Self::EffectTrails
            | Self::EffectGround
            | Self::EffectLights => 250,
            Self::EffectFrames
            | Self::EffectAlpha
            | Self::EffectCulling
            | Self::EffectClock
            | Self::EffectDecals
            | Self::SceneWater
            | Self::SceneTerrain
            | Self::SceneSky
            | Self::ModelAnimation
            | Self::RenderEffects
            | Self::RenderWorld
            | Self::RenderWater
            | Self::RenderPost
            | Self::RenderTextures => 500,
            Self::TimingFrame | Self::TimingMemory | Self::TimingCamera | Self::AudioState => 1000,
        }
    }
}

/// Write one line to a channel, formatting only when that channel is on.
///
/// `crate::diag!(EffectCards, "{} id={}", self.name, id)` costs one relaxed
/// atomic load when the box is clear, and the argument expressions are not even
/// evaluated then — which is the reason call sites may pass `format!`-style
/// arguments into a hot path at all.
#[macro_export]
macro_rules! diag {
    ($channel:ident, $($arg:tt)*) => {
        if $crate::diagnostics::enabled($crate::diagnostics::Channel::$channel) {
            $crate::diagnostics::emit(
                $crate::diagnostics::Channel::$channel,
                format!($($arg)*),
            );
        }
    };
}

/// The enabled mask, mirrored out of the registry so `enabled()` never locks.
static MASK: AtomicU64 = AtomicU64::new(0);
/// How many tail lines the settings tab keeps. Two hundred is a screenful of
/// monospace text and enough to see a repeating pattern without scrolling.
pub const TAIL_LINES: usize = 200;

/// A file is rotated once it passes this size, so a long session cannot fill the
/// disk with one channel.
const MAX_FILE_BYTES: u64 = 8 << 20;

struct Inner {
    dir: Option<PathBuf>,
    files: Vec<Option<File>>,
    written: Vec<u64>,
    counts: Vec<u64>,
    suppressed: Vec<u64>,
    last: Vec<Option<Instant>>,
    tail: VecDeque<String>,
    dropped_from_tail: u64,
    started: Instant,
    session: String,
}

impl Inner {
    fn new() -> Self {
        Self {
            dir: None,
            files: (0..CHANNELS).map(|_| None).collect(),
            written: vec![0; CHANNELS],
            counts: vec![0; CHANNELS],
            suppressed: vec![0; CHANNELS],
            last: (0..CHANNELS).map(|_| None).collect(),
            tail: VecDeque::with_capacity(TAIL_LINES),
            dropped_from_tail: 0,
            started: Instant::now(),
            session: String::new(),
        }
    }

    /// Open the channel's file, writing a one-line header the first time so an
    /// old log can never be mistaken for the current session.
    fn file(&mut self, channel: Channel) -> Option<&mut File> {
        let index = channel.index();
        if self.files[index].is_none() {
            let dir = self.dir.clone()?;
            std::fs::create_dir_all(&dir).ok()?;
            let path = dir.join(format!("{}.log", channel.key()));
            if self.written[index] > MAX_FILE_BYTES {
                let _ = std::fs::rename(&path, dir.join(format!("{}.log.1", channel.key())));
                self.written[index] = 0;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()?;
            let header = format!(
                "# {} -- {}\n# {}\n",
                channel.key(),
                self.session,
                channel.label()
            );
            file.write_all(header.as_bytes()).ok()?;
            self.written[index] += header.len() as u64;
            self.files[index] = Some(file);
        }
        self.files[index].as_mut()
    }
}

fn registry() -> &'static Mutex<Inner> {
    static REGISTRY: OnceLock<Mutex<Inner>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Inner::new()))
}

/// Whether the channel will write. One relaxed load; call sites gate on this
/// before they format anything.
pub fn enabled(channel: Channel) -> bool {
    MASK.load(Ordering::Relaxed) & (1u64 << channel.index()) != 0
}

/// Whether any channel at all is on, for call sites that would otherwise build
/// an expensive argument list.
pub fn any() -> bool {
    MASK.load(Ordering::Relaxed) != 0
}

/// Point the files at a directory. The settings tab shows this path; the default
/// is `<client root>/pwe-logs`.
pub fn set_dir(dir: impl Into<PathBuf>) {
    let dir = dir.into();
    let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    if inner.dir.as_deref() == Some(dir.as_path()) {
        return;
    }
    inner.dir = Some(dir);
    inner.files.iter_mut().for_each(|f| *f = None);
    inner.written.iter_mut().for_each(|w| *w = 0);
}

pub fn dir() -> Option<PathBuf> {
    let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    inner.dir.clone()
}

/// Turn channels on and off. Files are opened on the first line that reaches
/// them, so ticking a channel costs nothing until it has something to say.
pub fn set_mask(mask: u64) {
    MASK.store(mask, Ordering::Relaxed);
    if mask == 0 {
        let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
        inner.files.iter_mut().for_each(|f| *f = None);
    }
}

/// The description written into every file's header: client root, version and
/// whatever else identifies the run.
pub fn set_session(text: impl Into<String>) {
    let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    inner.session = text.into();
}

/// Write one line to a channel. Cheap to call when the channel is off; that is
/// the only reason call sites may pass a `format!` straight in.
pub fn emit(channel: Channel, line: impl AsRef<str>) {
    if !enabled(channel) {
        return;
    }
    let now = Instant::now();
    let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    let index = channel.index();
    let throttle = channel.throttle_ms();
    if throttle > 0 {
        if let Some(last) = inner.last[index] {
            if now.duration_since(last).as_millis() < throttle as u128 {
                inner.suppressed[index] = inner.suppressed[index].saturating_add(1);
                return;
            }
        }
        inner.last[index] = Some(now);
    }
    let stamp = now.duration_since(inner.started).as_secs_f32();
    let text = line.as_ref();
    let rendered = format!("[{stamp:8.3}] {text}\n");
    inner.counts[index] = inner.counts[index].saturating_add(1);
    if let Some(file) = inner.file(channel) {
        match file.write_all(rendered.as_bytes()) {
            Ok(()) => inner.written[index] += rendered.len() as u64,
            Err(_) => {
                inner.files[index] = None;
            }
        }
    }
    if inner.tail.len() == TAIL_LINES {
        inner.tail.pop_front();
        inner.dropped_from_tail = inner.dropped_from_tail.saturating_add(1);
    }
    inner
        .tail
        .push_back(format!("{}: {}", channel.key(), rendered.trim_end()));
}

/// The rolling tail as one block of text, oldest first.
pub fn tail(limit: usize) -> String {
    let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    let skip = inner.tail.len().saturating_sub(limit);
    inner
        .tail
        .iter()
        .skip(skip)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lines written for a channel this session.
pub fn count(channel: Channel) -> u64 {
    let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    inner.counts[channel.index()]
}

/// Tail lines that fell out of the ring because the tab was not scrolling. The
/// counter exists so "the tail starts mid-sentence" has an explanation.
pub fn tail_dropped() -> u64 {
    let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    inner.dropped_from_tail
}

/// Lines the throttle dropped for a channel this session.
pub fn suppressed(channel: Channel) -> u64 {
    let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    inner.suppressed[channel.index()]
}

/// Delete every log file of the current directory and start the tail over.
pub fn clear() {
    let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = inner.dir.clone() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("log") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    inner.files.iter_mut().for_each(|f| *f = None);
    inner.written.iter_mut().for_each(|w| *w = 0);
    inner.counts.iter_mut().for_each(|c| *c = 0);
    inner.suppressed.iter_mut().for_each(|c| *c = 0);
    inner.tail.clear();
    inner.dropped_from_tail = 0;
}

/// The folder a blank settings box means. Set once at startup from the client
/// root, so the settings window never has to know that root itself.
static DEFAULT_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remember `<client root>/pwe-logs`. First call wins: the path cannot change
/// while a session runs.
pub fn set_default_dir(dir: impl Into<PathBuf>) {
    let _ = DEFAULT_DIR.set(dir.into());
}

/// Point the logs at the directory the settings file names, or at the client
/// root's own folder when the box is empty, and make sure it exists.
pub fn apply_dir(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    let dir = if trimmed.is_empty() {
        DEFAULT_DIR.get().cloned()?
    } else {
        PathBuf::from(trimmed)
    };
    set_dir(dir.clone());
    std::fs::create_dir_all(&dir).ok();
    Some(dir)
}

/// Path the settings tab shows.
pub fn display_dir() -> String {
    match dir() {
        Some(path) => path.display().to_string(),
        None => "(not set)".into(),
    }
}

/// UI-only state: which subtab is selected and whether the tail follows the
/// newest line. Both live here so the settings window keeps its one-line call.
static UI_PAGE: AtomicU64 = AtomicU64::new(0);
static UI_FOLLOW: AtomicBool = AtomicBool::new(true);

/// The Logging page of the settings window: subtabs across the channel pages,
/// a tick per channel with what it costs and has produced so far, and a live tail
/// of everything the session has written.
///
/// Returns true when a tick or the directory changed, so the caller can both
/// apply the new mask and persist it.
pub fn ui(ui: &mut egui::Ui, mask: &mut u64, directory: &mut String) -> bool {
    let mut changed = false;
    ui.label(
        egui::RichText::new(
            "Ticks start and stop live logging while the viewer runs. Every channel \
             writes its own file in the folder below, with a timestamp on each line, \
             and the newest lines are shown here as they arrive.",
        )
        .small()
        .weak(),
    );
    ui.horizontal_wrapped(|ui| {
        ui.label("Folder:");
        let response = ui.add(
            egui::TextEdit::singleline(directory)
                .hint_text("empty = <client root>/pwe-logs")
                .desired_width(260.),
        );
        if response.changed() {
            changed = true;
        }
        if ui.button("Apply folder").clicked() {
            changed = true;
        }
        if ui.button("Clear files and tail").clicked() {
            clear();
        }
    });
    ui.label(egui::RichText::new(display_dir()).small().monospace());
    ui.separator();

    let mut page = UI_PAGE.load(Ordering::Relaxed) as usize;
    ui.horizontal_wrapped(|ui| {
        let width = ((ui.available_width() - 16.) / Page::ALL.len() as f32).clamp(70., 120.);
        for (index, option) in Page::ALL.iter().enumerate() {
            if ui
                .add_sized(
                    [width, 30.],
                    egui::SelectableLabel::new(page == index, option.label()),
                )
                .clicked()
            {
                page = index;
            }
        }
    });
    UI_PAGE.store(page as u64, Ordering::Relaxed);
    let page_option = Page::ALL[page];
    ui.label(
        egui::RichText::new(format!(
            "{}  —  {} of {} channels on",
            page_option.blurb(),
            Channel::ALL
                .iter()
                .filter(|c| c.page() == page_option && *mask & (1u64 << c.index()) != 0)
                .count(),
            Channel::ALL
                .iter()
                .filter(|c| c.page() == page_option)
                .count()
        ))
        .small()
        .weak(),
    );
    ui.horizontal(|ui| {
        if ui.button("All on this page").clicked() {
            for channel in Channel::ALL.iter().filter(|c| c.page() == page_option) {
                *mask |= 1u64 << channel.index();
            }
            changed = true;
        }
        if ui.button("None on this page").clicked() {
            for channel in Channel::ALL.iter().filter(|c| c.page() == page_option) {
                *mask &= !(1u64 << channel.index());
            }
            changed = true;
        }
        if ui.button("All off").clicked() {
            *mask = 0;
            changed = true;
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_source("logging-channels")
        .max_height(210.)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for channel in Channel::ALL.iter().filter(|c| c.page() == page_option) {
                let bit = 1u64 << channel.index();
                let mut on = *mask & bit != 0;
                ui.horizontal(|ui| {
                    let response = ui
                        .checkbox(&mut on, channel.label())
                        .on_hover_text(channel.help());
                    if response.changed() {
                        *mask = if on { *mask | bit } else { *mask & !bit };
                        changed = true;
                    }
                    let written = count(*channel);
                    let dropped = suppressed(*channel);
                    let throttle = channel.throttle_ms();
                    let detail = if written == 0 && dropped == 0 {
                        String::new()
                    } else if throttle == 0 {
                        format!("{written} lines")
                    } else {
                        format!("{written} lines, {dropped} throttled")
                    };
                    if !detail.is_empty() {
                        ui.label(egui::RichText::new(detail).small().weak());
                    }
                });
            }
        });
    ui.separator();
    let dropped = tail_dropped();
    if dropped > 0 {
        ui.label(
            egui::RichText::new(format!(
                "{dropped} older tail lines have rolled off; the files keep everything"
            ))
            .small()
            .weak(),
        );
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Live tail").strong());
        let mut follow = UI_FOLLOW.load(Ordering::Relaxed);
        if ui.checkbox(&mut follow, "Follow").changed() {
            UI_FOLLOW.store(follow, Ordering::Relaxed);
        }
        if ui.button("Copy tail").clicked() {
            ui.output_mut(|output| output.copied_text = tail(TAIL_LINES));
        }
    });
    egui::ScrollArea::vertical()
        .id_source("logging-tail")
        .max_height(200.)
        .auto_shrink([false, false])
        .stick_to_bottom(UI_FOLLOW.load(Ordering::Relaxed))
        .show(ui, |ui| {
            ui.monospace(tail(TAIL_LINES));
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The registry is process-wide, so the tests that change it take turns
    /// rather than fighting over the mask.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn every_channel_has_a_unique_key_and_fits_the_mask() {
        let mut keys: Vec<&str> = Channel::ALL.iter().map(|c| c.key()).collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "two channels share a key");
        for channel in Channel::ALL {
            assert!(
                channel.index() < 64,
                "{} does not fit the mask",
                channel.key()
            );
            assert!(!channel.label().is_empty());
            assert!(!channel.help().is_empty());
        }
        assert_eq!(Channel::ALL.len(), CHANNELS);
        // The discriminants are the bit positions, so a gap would silently move
        // a channel into another's bit.
        for (index, channel) in Channel::ALL.iter().enumerate() {
            assert_eq!(channel.index(), index, "{} is out of order", channel.key());
        }
    }

    #[test]
    fn every_effect_bullet_lands_on_the_effects_page() {
        for channel in Channel::ALL {
            if channel.key().starts_with("effect-") {
                assert_eq!(channel.page(), Page::Effects, "{}", channel.key());
            }
        }
    }

    #[test]
    fn a_disabled_channel_writes_nothing() {
        let _guard = lock();
        set_mask(0);
        let before = count(Channel::EffectCards);
        emit(Channel::EffectCards, "should not appear");
        assert_eq!(count(Channel::EffectCards), before);
        assert!(!enabled(Channel::EffectCards));
    }

    #[test]
    fn an_enabled_channel_writes_its_file_and_the_tail() {
        let _guard = lock();
        let dir = std::env::temp_dir().join(format!("pwe-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir(dir.clone());
        set_mask(1 << Channel::SceneSounds.index());
        emit(Channel::SceneSounds, "a line about a sound");
        let text = std::fs::read_to_string(dir.join("scene-sounds.log")).expect("log file");
        assert!(text.contains("a line about a sound"), "{text}");
        assert!(
            text.contains("scene-sounds --"),
            "the header names the run: {text}"
        );
        assert_eq!(count(Channel::SceneSounds), 1);
        assert!(tail(10).contains("a line about a sound"));
        // A second line appends rather than replacing.
        emit(Channel::SceneSounds, "and another");
        let text = std::fs::read_to_string(dir.join("scene-sounds.log")).unwrap();
        assert!(text.contains("and another"));
        set_mask(0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_throttled_channel_counts_what_it_dropped() {
        let _guard = lock();
        set_mask(1 << Channel::EffectParticles.index());
        let dropped = suppressed(Channel::EffectParticles);
        emit(Channel::EffectParticles, "first");
        emit(Channel::EffectParticles, "second");
        assert!(suppressed(Channel::EffectParticles) > dropped);
        set_mask(0);
    }
}
