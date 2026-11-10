//! Live logging the viewer can be told to do while it runs.
//!
//! Every subsystem that has ever needed a report while debugging a scene —
//! effect roots, card geometry, UV windows, textures, alpha, the ground drape,
//! model elements, lights, culling, the scene's own placements, the render
//! passes, timing — has a named *channel* here. Channels are off unless the user
//! ticks them in **Settings ▸ Logging**, or unless `PWE_LOG` names them, and the
//! cost of a disabled channel is one relaxed atomic load per call site.
//!
//! Four sinks, each with a different job:
//!
//! * **a file per channel** in `<explorer folder>/pwe-logs/`, appended and
//!   rotated at 8 MiB, so a session can be read after the fact and handed over
//!   whole. The folder belongs to the explorer, not to the client it is reading:
//!   a 40 GB game install is not where a report should have to be found;
//! * **`session.log`**, written whether or not any channel is ticked: the run's
//!   header, every capture started, every mark placed (`F8`), every folder
//!   change. It is the one file that always explains the others;
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
use std::path::{Path, PathBuf};
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

    /// The groups of a page, in the order they are listed. Channels name their
    /// group, so the list here is the only place the order lives.
    pub fn groups(self) -> &'static [&'static str] {
        match self {
            Self::Effects => &["Where it is", "Cards", "Sub-elements", "Diagnosis"],
            Self::Scene => &["Placements", "Terrain, sky and water"],
            Self::Models => &["Loading", "Shading", "Animation"],
            Self::Render => &["Batches", "Post and uploads"],
            Self::Timing => &["Frame and camera", "Load and memory", "Audio"],
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

    /// Which heading inside its page the channel is listed under. A group is
    /// about what the line is *for*: where an effect is, what its cards are made
    /// of, what hangs off it, and what went wrong.
    pub fn group(self) -> &'static str {
        match self {
            Self::EffectRoots
            | Self::EffectElements
            | Self::EffectFrames
            | Self::EffectCulling
            | Self::EffectClock => "Where it is",
            Self::EffectCards
            | Self::EffectUv
            | Self::EffectDecals
            | Self::EffectTextures
            | Self::EffectAlpha => "Cards",
            Self::EffectParticles
            | Self::EffectTrails
            | Self::EffectModels
            | Self::EffectLights
            | Self::EffectGround => "Sub-elements",
            Self::EffectProblems => "Diagnosis",
            Self::SceneEffects | Self::SceneOrnaments | Self::SceneSounds => "Placements",
            Self::SceneTerrain | Self::SceneSky | Self::SceneWater => "Terrain, sky and water",
            Self::ModelLoads | Self::ModelQueue => "Loading",
            Self::ModelMaterials => "Shading",
            Self::ModelAnimation => "Animation",
            Self::RenderEffects | Self::RenderWorld | Self::RenderWater => "Batches",
            Self::RenderPost | Self::RenderTextures => "Post and uploads",
            Self::TimingFrame | Self::TimingCamera => "Frame and camera",
            Self::TimingLoad | Self::TimingMemory => "Load and memory",
            Self::AudioState => "Audio",
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

    /// Whether this build has a call site for the channel yet.
    ///
    /// The list is honest rather than aspirational: a ticked channel with no hook
    /// would write an empty file, and an empty file looks exactly like a
    /// subsystem that is working. The settings tab marks the rest so a report is
    /// never built on a channel that could not have said anything.
    pub fn wired(self) -> bool {
        matches!(
            self,
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
                | Self::EffectProblems
                | Self::SceneEffects
                | Self::ModelQueue
                | Self::RenderEffects
        )
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

/// One click's worth of channels: exactly the ones a particular report needs,
/// and nothing else, so the files stay short enough to read.
///
/// Every preset names a report that was actually filed against this viewer, so
/// "which boxes do I tick for the sun rays" never has to be asked again. Ticking
/// a preset replaces the mask instead of adding to it: a capture with a leftover
/// channel in it is a capture nobody can read.
pub struct Preset {
    pub name: &'static str,
    /// What the preset is for, shown as the button's tooltip.
    pub help: &'static str,
    /// Everything, ignoring the list — the button is still one of the set.
    pub all: bool,
    pub channels: &'static [Channel],
}

/// `树林光束` and the Archosaur pair share almost everything: what differs is
/// whether the UV window matters, which it does for a waterfall and does not for
/// a beam that has one keypoint.
const BEAM_CHANNELS: &[Channel] = &[
    Channel::EffectRoots,
    Channel::EffectElements,
    Channel::EffectFrames,
    Channel::EffectCards,
    Channel::EffectDecals,
    Channel::EffectTextures,
    Channel::EffectAlpha,
    Channel::EffectCulling,
];

pub const PRESETS: [Preset; 6] = [
    Preset {
        name: "Sun rays",
        help: "树林光束: which root placed the rays, which element built each card, \
               which basis the card was drawn on, and the texture and blend it got. \
               The direction of the shaft is in effect-cards.",
        all: false,
        channels: BEAM_CHANNELS,
    },
    Preset {
        name: "Archosaur beams",
        help: "天煞光束: the two placements that fire one after the other. \
               effect-frames prints each element's keypoint basis, which is where \
               a shaft that should be horizontal stops being horizontal.",
        all: false,
        channels: BEAM_CHANNELS,
    },
    Preset {
        name: "Waterfall",
        help: "水门_瀑布: the five sheets and their scroll. effect-uv prints the \
               tile window per frame and effect-clock prints the clock it advances \
               on; between them a frozen sheet says which half is wrong.",
        all: false,
        channels: &[
            Channel::SceneEffects,
            Channel::EffectRoots,
            Channel::EffectElements,
            Channel::EffectUv,
            Channel::EffectTextures,
            Channel::EffectClock,
            Channel::EffectCards,
        ],
    },
    Preset {
        name: "Crater colour",
        help: "A saturated disc at the bottom of a crater: effect-cards names the \
               root, effect-textures says whether the texture resolved, effect-alpha \
               says what the colour and alpha folds produced, and scene-effects \
               says what the map placed there.",
        all: false,
        channels: &[
            Channel::SceneEffects,
            Channel::EffectRoots,
            Channel::EffectCards,
            Channel::EffectTextures,
            Channel::EffectAlpha,
            Channel::EffectCulling,
            Channel::EffectClock,
        ],
    },
    Preset {
        name: "Trails and rings",
        help: "Ribbons, fractals, waves and spinning rings: the trail geometry, \
               the UV window that has to scroll along it, and the clock that turns \
               it. Generous and cheap, so it is the first thing to try on any \
               effect that moves when it should not.",
        all: false,
        channels: &[
            Channel::EffectRoots,
            Channel::EffectElements,
            Channel::EffectTrails,
            Channel::EffectUv,
            Channel::EffectClock,
            Channel::EffectCards,
        ],
    },
    Preset {
        name: "Everything",
        help: "Every channel at once, for a report that has not been narrowed down \
               yet. Heavy: expect hundreds of megabytes on a long walk.",
        all: true,
        channels: &[],
    },
];

impl Preset {
    /// The mask this preset means.
    pub fn mask(&self) -> u64 {
        if self.all {
            return Channel::ALL
                .iter()
                .fold(0u64, |mask, channel| mask | 1u64 << channel.index());
        }
        self.channels
            .iter()
            .fold(0u64, |mask, channel| mask | 1u64 << channel.index())
    }

    /// The files it will produce, for the tooltip and the session file.
    pub fn keys(&self) -> Vec<&'static str> {
        if self.all {
            return Channel::ALL.iter().map(|channel| channel.key()).collect();
        }
        self.channels.iter().map(|channel| channel.key()).collect()
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
/// is `<explorer folder>/pwe-logs`, next to the executable rather than inside the
/// client install the viewer happens to be reading.
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
    let text = text.into();
    {
        let mut inner = registry().lock().unwrap_or_else(|e| e.into_inner());
        inner.session = text.clone();
    }
    session(format!("==== SESSION {text} ===="));
}

/// The always-on timeline, `session.log` in the log folder.
///
/// It is written whether or not a single channel is ticked, and it is the one
/// file that explains the others a week later: the run's header, every capture
/// started, every mark placed, every folder change. It stays small because
/// nothing per-frame ever writes here.
pub fn session(text: impl AsRef<str>) {
    let Some(dir) = dir() else {
        return;
    };
    std::fs::create_dir_all(&dir).ok();
    let stamp = {
        let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
        inner.started.elapsed().as_secs_f32()
    };
    let line = format!("[{stamp:8.3}] {}\n", text.as_ref());
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("session.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Drop a marker into `session.log` and into every channel that is on.
///
/// The viewer binds this to `F8` and sends the camera pose with it, so "at this
/// spot, this happened" is a line every file agrees on instead of a paragraph in
/// a report that names no position.
pub fn mark(note: impl AsRef<str>) {
    let line = format!("---- MARK {} ----", note.as_ref());
    session(&line);
    for channel in Channel::ALL {
        if enabled(channel) {
            push(channel, &line, true);
        }
    }
}

/// One block of text describing the run, for the settings page's **Copy
/// report** button: the header, which channels are on, what each of them wrote,
/// and the live tail. This is the thing to paste into a report — it names the
/// build, the folder and the switches, which is most of what a log is missing
/// when it arrives alone.
pub fn report() -> String {
    let (session_text, dir_text) = {
        let inner = registry().lock().unwrap_or_else(|e| e.into_inner());
        (
            inner.session.clone(),
            match &inner.dir {
                Some(dir) => dir.display().to_string(),
                None => "(not set)".to_string(),
            },
        )
    };
    let on: Vec<&'static str> = Channel::ALL
        .iter()
        .filter(|channel| enabled(**channel))
        .map(|channel| channel.key())
        .collect();
    let mut out = String::new();
    out.push_str("Perfect World Explorer -- live log report\n");
    out.push_str(&format!("# session:  {session_text}\n"));
    out.push_str(&format!("# folder:   {dir_text} ({})\n", folder_summary()));
    out.push_str(&format!(
        "# channels: {} of {CHANNELS} on ({})\n",
        on.len(),
        if on.is_empty() {
            "none".to_string()
        } else {
            on.join(", ")
        }
    ));
    for channel in Channel::ALL.iter().filter(|channel| count(**channel) > 0) {
        out.push_str(&format!(
            "#   {:16} {:>7} lines {:>6} throttled  {}\n",
            channel.key(),
            count(*channel),
            suppressed(*channel),
            channel.label()
        ));
    }
    out.push_str("\n--- live tail ---\n");
    out.push_str(&tail(TAIL_LINES));
    out.push('\n');
    out
}

/// Write one line to a channel. Cheap to call when the channel is off; that is
/// the only reason call sites may pass a `format!` straight in.
pub fn emit(channel: Channel, line: impl AsRef<str>) {
    push(channel, line.as_ref(), false);
}

/// [`emit`], except that `force` writes even when the channel is off. Only a
/// mark uses it: a marker the user asked for must land in the file it was aimed
/// at, whatever the throttle and the mask say.
fn push(channel: Channel, line: &str, force: bool) {
    if !force && !enabled(channel) {
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
    let rendered = format!("[{stamp:8.3}] {line}\n");
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

/// The folder a blank settings box means. Set once at startup from
/// [`explorer_root`], so the settings window never has to know that path itself.
static DEFAULT_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remember `<explorer folder>/pwe-logs`. First call wins: the path cannot change
/// while a session runs.
pub fn set_default_dir(dir: impl Into<PathBuf>) {
    let _ = DEFAULT_DIR.set(dir.into());
}

/// The folder the explorer itself lives in.
///
/// Logs, and the settings file next to them, belong to the tool, not to the
/// game: the client root is a 40 GB install that may well be read-only, and
/// nobody should have to go looking for a report inside it.
pub fn explorer_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| explorer_root_from(&exe))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// [`explorer_root`] for a known executable path, split out so a test can point
/// it at a `target/debug` tree without touching the real one. A cargo build's
/// executable sits two levels below the checkout that built it, and logging into
/// `target/debug/pwe-logs` would hide the files under a folder cargo deletes.
fn explorer_root_from(exe: &Path) -> PathBuf {
    let Some(dir) = exe.parent() else {
        return PathBuf::from(".");
    };
    let cargo = matches!(
        dir.file_name().and_then(|name| name.to_str()),
        Some("debug") | Some("release")
    ) && dir
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some("target");
    if cargo {
        if let Some(root) = dir.parent().and_then(|target| target.parent()) {
            return root.to_path_buf();
        }
    }
    dir.to_path_buf()
}

/// The folder a blank box means, for the settings page's tooltip.
pub fn default_dir() -> Option<PathBuf> {
    DEFAULT_DIR.get().cloned()
}

/// What the folder holds right now: "7 files, 2.4 MB". The count is what tells a
/// ticked channel apart from a channel that wrote nothing.
///
/// Cached for half a second: the settings page asks once per frame, and a
/// directory scan per frame is a silly thing to do while a world streams in.
pub fn folder_summary() -> String {
    static CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((at, text)) = guard.as_ref() {
            if at.elapsed().as_millis() < 500 {
                return text.clone();
            }
        }
    }
    let text = scan_folder();
    {
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        *guard = Some((Instant::now(), text.clone()));
    }
    text
}

fn scan_folder() -> String {
    let Some(dir) = dir() else {
        return "no folder yet".to_string();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return "folder does not exist yet".to_string();
    };
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("log") {
            files += 1;
            bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        }
    }
    if files == 0 {
        "empty".to_string()
    } else {
        format!("{files} files, {}", human_bytes(bytes))
    }
}

fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

/// Show the log folder in the platform's file manager. Best effort: a headless
/// session simply does nothing.
pub fn open_folder() {
    let Some(dir) = dir() else {
        return;
    };
    std::fs::create_dir_all(&dir).ok();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
}

/// Point the logs at the directory the settings file names, or at the explorer's
/// own folder when the box is empty, and make sure it exists.
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

/// Whether a saved folder is the one older builds wrote to, `<client
/// root>/pwe-logs`, so a session that predates the move can be pointed at the
/// new default instead of silently keeping the old place.
pub fn is_legacy_default(text: &str, client_root: &Path) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && Path::new(trimmed) == client_root.join("pwe-logs").as_path()
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

/// The folder box. It is applied when the user says so, not on every keystroke:
/// creating a directory per typed character is how a log folder ends up as `C`,
/// `C:` and `C:\P`.
static UI_EDIT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn edit_slot() -> &'static Mutex<Option<String>> {
    UI_EDIT.get_or_init(|| Mutex::new(None))
}

fn edit_buffer(directory: &str) -> String {
    let mut slot = edit_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    slot.get_or_insert_with(|| directory.to_string()).clone()
}

fn set_edit_buffer(text: &str) {
    let mut slot = edit_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *slot = Some(text.to_string());
}

/// The five steps the page spells out, so "how do I log this properly" is
/// answered where the switches are rather than in a conversation.
const REPORT_STEPS: [&str; 6] = [
    "1. Click the capture that matches the defect. It ticks exactly the channels that \
        report needs and leaves everything else off.",
    "2. Fly or walk to the spot, get the artefact on screen, and press F8. The MARK line \
        carries the camera pose, so the log names the position instead of describing it.",
    "3. Wait a few seconds — two or three frames of a waterfall, one full turn of a ring — \
        then press F8 again to close the window you are reporting.",
    "4. Click Copy report. It carries the build, the folder, every switch that was on, \
        what each channel wrote, and the tail; paste that into the report.",
    "5. If the tail is not enough, attach the two or three files the report names from the \
        folder above — not the whole folder.",
    "One defect per capture, and Clear files in between: two captures in one folder read as \
        one long log with no boundary. A channel marked \"no hook in this build yet\" has no \
        call site, so ticking it produces nothing — worth saying in the report.",
];

/// The Logging page of the settings window.
///
/// Laid out in the order a report is made: where the files go, which capture is
/// running, then the channels grouped by what they are for, then the live tail.
/// Every control explains itself on hover, because the page exists so that nobody
/// has to read this file to use it.
///
/// Returns true when a tick, the folder or a capture changed, so the caller can
/// apply the new mask and persist it.
pub fn ui(ui: &mut egui::Ui, mask: &mut u64, directory: &mut String) -> bool {
    let mut changed = false;
    ui.label(
        egui::RichText::new(
            "Tick a channel and it starts writing its own file while the viewer runs. \
             A capture ticks the set one known report needs. F8 drops a MARK line with \
             the camera pose into session.log and into every channel that is on.",
        )
        .small()
        .weak(),
    );
    // ---- where the files go -------------------------------------------------
    let mut edit = edit_buffer(directory.as_str());
    ui.horizontal_wrapped(|ui| {
        ui.label("Folder:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut edit)
                .hint_text("<explorer folder>/pwe-logs")
                .desired_width(300.),
        );
        let entered =
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let apply = ui.button("Apply").on_hover_text(
            "Write the logs here from now on. An empty box means the explorer's own \
             pwe-logs folder, wherever the executable lives.",
        );
        if (apply.clicked() || entered) && edit.trim() != directory.trim() {
            set_edit_buffer(&edit);
            *directory = edit.trim().to_string();
            changed = true;
        }
        let default_help = format!(
            "Back to {}, next to the executable rather than inside the client install \
             the viewer is reading.",
            default_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|| "<explorer folder>/pwe-logs".to_string())
        );
        if ui
            .button("Explorer folder")
            .on_hover_text(default_help)
            .clicked()
        {
            edit.clear();
            set_edit_buffer("");
            if !directory.is_empty() {
                directory.clear();
                changed = true;
            }
        }
        if ui
            .button("Open")
            .on_hover_text("Show the folder in the file manager.")
            .clicked()
        {
            open_folder();
        }
        if ui
            .button("Clear files")
            .on_hover_text("Delete this folder's log files and start the tail over.")
            .clicked()
        {
            clear();
            session("---- CLEARED the log files ----");
        }
    });
    ui.label(
        egui::RichText::new(format!("{}  --  {}", display_dir(), folder_summary()))
            .small()
            .monospace(),
    );
    ui.separator();

    // ---- captures -----------------------------------------------------------
    ui.label(
        egui::RichText::new("CAPTURES -- one click ticks the set a report needs")
            .small()
            .strong(),
    );
    ui.horizontal_wrapped(|ui| {
        for preset in PRESETS.iter() {
            let keys = preset.keys();
            let help = format!(
                "{}\n\nTicks exactly {} channels and leaves the rest off:\n{}",
                preset.help,
                keys.len(),
                keys.join(", ")
            );
            if ui.button(preset.name).on_hover_text(help).clicked() {
                *mask = preset.mask();
                session(format!(
                    "---- CAPTURE {} -- {} channels: {} ----",
                    preset.name,
                    keys.len(),
                    keys.join(", ")
                ));
                changed = true;
            }
        }
        if ui
            .button("Nothing")
            .on_hover_text("Untick every channel; the files already written stay.")
            .clicked()
        {
            *mask = 0;
            changed = true;
        }
    });
    ui.separator();

    // ---- channels -----------------------------------------------------------
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
            "{}  --  {} of {} channels on",
            page_option.blurb(),
            Channel::ALL
                .iter()
                .filter(|channel| channel.page() == page_option
                    && *mask & (1u64 << channel.index()) != 0)
                .count(),
            Channel::ALL
                .iter()
                .filter(|channel| channel.page() == page_option)
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
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_source("logging-channels")
        .max_height(220.)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for group in page_option.groups() {
                let members: Vec<Channel> = Channel::ALL
                    .iter()
                    .copied()
                    .filter(|channel| channel.page() == page_option && channel.group() == *group)
                    .collect();
                if members.is_empty() {
                    continue;
                }
                let on = members
                    .iter()
                    .filter(|channel| *mask & (1u64 << channel.index()) != 0)
                    .count();
                ui.add_space(6.);
                ui.label(
                    egui::RichText::new(format!("{group}  --  {on} of {}", members.len()))
                        .small()
                        .strong(),
                );
                for channel in members {
                    let bit = 1u64 << channel.index();
                    let mut ticked = *mask & bit != 0;
                    let response = ui
                        .checkbox(&mut ticked, channel.label())
                        .on_hover_text(channel.help());
                    if response.changed() {
                        *mask = if ticked { *mask | bit } else { *mask & !bit };
                        changed = true;
                    }
                    if !channel.wired() {
                        ui.label(
                            egui::RichText::new("no hook in this build yet")
                                .small()
                                .weak()
                                .italics(),
                        );
                    }
                    let written = count(channel);
                    let dropped = suppressed(channel);
                    let detail = if written == 0 && dropped == 0 {
                        if ticked {
                            "on, nothing yet".to_string()
                        } else {
                            String::new()
                        }
                    } else if channel.throttle_ms() == 0 {
                        format!("{written} lines")
                    } else {
                        format!("{written} lines, {dropped} throttled")
                    };
                    if !detail.is_empty() {
                        ui.label(egui::RichText::new(detail).small().weak());
                    }
                }
            }
        });
    ui.separator();

    // ---- tail ---------------------------------------------------------------
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
        if ui
            .button("Copy tail")
            .on_hover_text("The tail alone, one line per entry.")
            .clicked()
        {
            ui.output_mut(|output| output.copied_text = tail(TAIL_LINES));
        }
        if ui
            .button("Copy report")
            .on_hover_text(
                "The build, the folder, every switch that is on, what each channel wrote, \
                 and the tail -- one block, ready to paste into a report.",
            )
            .clicked()
        {
            ui.output_mut(|output| output.copied_text = report());
        }
    });
    egui::ScrollArea::vertical()
        .id_source("logging-tail")
        .max_height(180.)
        .auto_shrink([false, false])
        .stick_to_bottom(UI_FOLLOW.load(Ordering::Relaxed))
        .show(ui, |ui| {
            ui.monospace(tail(TAIL_LINES));
        });
    ui.add_space(4.);
    egui::CollapsingHeader::new("How to capture a report")
        .default_open(false)
        .show(ui, |ui| {
            for step in REPORT_STEPS {
                ui.label(egui::RichText::new(step).small());
            }
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
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
    fn every_channel_sits_in_one_of_its_page_groups() {
        for page in Page::ALL {
            for channel in Channel::ALL.iter().filter(|channel| channel.page() == page) {
                assert!(
                    page.groups().contains(&channel.group()),
                    "{} names {:?}, which {} does not list",
                    channel.key(),
                    channel.group(),
                    page.label()
                );
            }
            // A heading with nothing under it is worse than no heading.
            for group in page.groups() {
                assert!(
                    Channel::ALL
                        .iter()
                        .any(|channel| channel.page() == page && channel.group() == *group),
                    "{} lists {group} with no channels",
                    page.label()
                );
            }
        }
    }

    #[test]
    fn every_preset_names_channels_that_exist() {
        for preset in PRESETS.iter() {
            assert!(!preset.name.is_empty());
            assert!(!preset.help.is_empty());
            let mask = preset.mask();
            assert!(mask != 0, "{} ticks nothing", preset.name);
            if preset.all {
                assert_eq!(mask.count_ones() as usize, CHANNELS);
                assert!(preset.channels.is_empty(), "{} is ambiguous", preset.name);
            } else {
                assert!(!preset.channels.is_empty());
                assert_eq!(mask.count_ones() as usize, preset.channels.len());
                for channel in preset.channels {
                    assert!(
                        Channel::ALL.contains(channel),
                        "{} is not a channel",
                        channel.key()
                    );
                }
                let keys = preset.keys();
                let mut sorted = keys.clone();
                sorted.sort_unstable();
                let before = sorted.len();
                sorted.dedup();
                assert_eq!(before, sorted.len(), "{} lists one twice", preset.name);
            }
        }
    }

    #[test]
    fn the_explorer_root_is_the_checkout_that_built_a_cargo_binary() {
        assert_eq!(
            explorer_root_from(Path::new(
                "/work/Perfect-World-Explorer/target/debug/pwe.exe"
            )),
            PathBuf::from("/work/Perfect-World-Explorer")
        );
        assert_eq!(
            explorer_root_from(Path::new(
                "/work/Perfect-World-Explorer/target/release/pwe.exe"
            )),
            PathBuf::from("/work/Perfect-World-Explorer")
        );
        // A shipped build: the executable's own folder is the root.
        assert_eq!(
            explorer_root_from(Path::new("/opt/PWE/Perfect-World-Explorer.exe")),
            PathBuf::from("/opt/PWE")
        );
        // `debug` without a `target` above it is just a folder name.
        assert_eq!(
            explorer_root_from(Path::new("/tmp/debug/pwe")),
            PathBuf::from("/tmp/debug")
        );
    }

    #[test]
    fn a_saved_client_root_folder_migrates_to_the_new_default() {
        let client = Path::new("/games/pw");
        assert!(is_legacy_default("/games/pw/pwe-logs", client));
        assert!(!is_legacy_default("", client));
        assert!(!is_legacy_default("/home/me/PWE/pwe-logs", client));
        assert!(!is_legacy_default("/games/pw/pwe-logs-old", client));
    }

    #[test]
    fn a_mark_lands_in_session_log_with_no_channel_ticked() {
        let _guard = lock();
        let dir = std::env::temp_dir().join(format!("pwe-mark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir(dir.clone());
        set_mask(0);
        mark("camera at 1 2 3");
        let text = std::fs::read_to_string(dir.join("session.log")).expect("session log");
        assert!(text.contains("MARK camera at 1 2 3"), "{text}");
        // And it reaches a channel that is on, past the throttle that would
        // otherwise drop the second line of the millisecond.
        set_mask(1 << Channel::EffectParticles.index());
        mark("second");
        let text = std::fs::read_to_string(dir.join("effect-particles.log")).expect("channel log");
        assert!(text.contains("MARK second"), "{text}");
        set_mask(0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_report_names_the_run_and_the_switches() {
        let _guard = lock();
        let dir = std::env::temp_dir().join(format!("pwe-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir(dir.clone());
        set_session("client root X | version 9.9.9 | mask 0x1");
        set_mask(PRESETS[0].mask());
        emit(Channel::EffectRoots, "a root");
        let text = report();
        assert!(text.contains("client root X"), "{text}");
        assert!(text.contains("effect-roots"), "{text}");
        assert!(text.contains("a root"), "{text}");
        assert!(text.contains("of 36 on"), "{text}");
        set_mask(0);
        let _ = std::fs::remove_dir_all(&dir);
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
