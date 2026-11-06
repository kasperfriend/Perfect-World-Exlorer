//! Runtime player for the retail `.gfx` container: the clocks, the controllers,
//! the particle systems, the ribbons and the billboards that the client's
//! `A3DGFXMan` / `A3DParticleSystem` pair drives.
//!
//! # What is recovered and what is interpreted
//!
//! Every rule with a disassembly citation was read out of `elementclient.exe`
//! and is reproduced instruction for instruction. Where the binary could not be
//! followed — the emitter shape classes, the noise table generator and the
//! billboard basis — the comment says so instead of hiding it.
//!
//! * Clocks. The tick receives an integer millisecond delta and the particle
//!   system converts it at `0x71a1f1` (`fild dt; fmul 0.001`), then scales it by
//!   the root's live `PlaySpeed` copy at root+0x1ec (`0x71a217`).
//! * Lifetime. `0x71a247`: `age += dt; if (age >= ttl) unlink`, with the age at
//!   node+0x00 and the TTL at node+0x04.
//! * Quota. `0x715c09`: with `Quota > 0`, `count = min(count, quota - live)`;
//!   -1 is unlimited. Live count at +0x1a8, quota at +0x1a4.
//! * Acceleration. `0x7165b0` integrates trapezoidally:
//!   `speed += ParAcc*dt; d = (speed_old + speed_new)*0.5*dt; pos += d*dir`,
//!   `dir` being the particle's own unit direction (node+0x44) with the
//!   displacement mirrored into node+0x50. The second term uses `Acc` along the
//!   authored `AccDir` (settings+0x1c) with its own accumulator at node+0x5c.
//! * Controllers 100 and 101 (`0x6eaf60`, `0x6eb230`) integrate the same
//!   trapezoid, re-read instruction by instruction: `v = Vel + Acc*age`, then
//!   `dspl = (2*v + Acc*dt) * dt * 0.5` — the client adds `v` to itself, adds
//!   `Acc*dt`, multiplies by `dt` and finally by the 0.5 it loads from
//!   `0x8d5894`. Controller 100 adds `dspl*Dir` to the position (temp+0x00)
//!   *and* to the velocity accumulator (temp+0x28); controller 101 adds `dspl`
//!   to temp+0x24, the rotation slot — an angular rate, which is why the corpus
//!   writes `Vel: 1.745329` (100 degrees per second). Both skip the step while
//!   `age < StartTime`, and both compare `EndTime` against the 0.0 at
//!   `0x8d58a0`, so *any* negative end means "forever" (the corpus only ever
//!   writes -1) and a non-negative end stops the controller once `age` passes
//!   it.
//! * Controllers 102/103 (`0x6eb400`, `0x6eb860`): the same `dspl` feeds the
//!   axis-angle helper `0x7b6fd0(&Axis, dspl)` and is applied around `Pos`.
//! * Controller 104 (`0x6ebba0`): `target = Pos + velocity_accumulator`,
//!   `n = normalize(position - target)`, `position += n*dspl`; `Vel == -1`
//!   disables it and `dspl == -1` snaps the position onto the target
//!   (`0x6ebd0a`).
//! * Controller 105 (`0x6ebf70`): the packed ARGB at temp+0x0c is unpacked into
//!   red, green, blue, alpha in that order (`0x6ebfab`), so `ColorDelta`'s four
//!   integers are R, G, B, A and the corpus's `ColorDelta: 0, 0, 0, -1000` is an
//!   alpha fade. Per channel `acc = delta*dt + remainder; i = (int)acc;
//!   remainder = acc - i; channel = clamp(channel + i, 0, 255)`, with one
//!   remainder per channel at ctrl+0x24..0x30 that is cleared when the byte
//!   saturates (`0x6ec00b`, `0x6ec02a`), and the four bytes repacked at
//!   `0x6ec03f`.
//! * Controller 106 (`0x6ecad0`): `scale += ScaleChage*dt`, clamped to the
//!   authored `[min, max]` (defaults 0.01 and 9999).
//! * Controller 108: the colour ramp is the client's own 128-entry PC24 table,
//!   reused here through `gfx_color_track`, which this repository already
//!   verifies against the native construction at `0x6ec240`.
//! * Affector order. `0x716240` copies the node into a 0x3c byte temp struct,
//!   calls every affector's `Update` (vtable slot 1) in list order and copies
//!   the result back; this player mutates the particle in the same order.
//!
//! * Birth. The emitter factory is `0x723ab0`: `id - 100` indexes a six-case jump
//!   table at `0x723de4`, so each particle element owns an emitter of ids
//!   100..105 and 120..125, and every emitter shares the same vtable slots —
//!   1 samples the direction (`0x723440`), 2 the TTL (`0x7237e0`), 3 the colour
//!   (`0x7237f0`), 5 the scale (`0x723a20`), 6 the rotation (`0x723a50`) and 8
//!   places the new particle (element 123's is `0x727e40`). The particle object
//!   calls them from its own emit routine `0x715b50`.
//! * Spawn volume. `0x728270` rejection-samples the unit ball — three
//!   `rand() * (2/32767) - 1` draws retried while `x*x + y*y + z*z > 1` — and
//!   multiplies it by the matrix at emitter+0xc0, which `0x729670` fills from
//!   `AreaSize` as `diag(AreaSize * 0.5)`, then adds the emitter position at
//!   +0x68. `AreaSize` is therefore an ellipsoid of semi-axes `AreaSize/2`.
//! * Cone. `0x723440` draws `alpha = rand() * (1/32767) * Angle` and
//!   `beta = rand() * (2*pi/32767)` and applies both through the axis-angle
//!   quaternion helper `0x7b6fd0`, so the polar angle is uniform in
//!   `[0, Angle]` and `Angle` is never clamped.
//!
//! * Colour. `0x7237f0` returns equal endpoints untouched and otherwise takes
//!   one of two samplers on the `IsUseHSVInterp` byte at emitter+0xb0
//!   (`0x723804`): four independent `rand()` draws interpolated per channel, or
//!   a single shared draw interpolated in the HSV the parser stored at +0x38
//!   and +0x48 (`0x723812`, converting back through `0x7898f0`).
//!
//! Three native behaviours are deliberately not copied, each with its evidence
//! recorded in `Findings/`: the emission-count detail factor gated at
//! `0x715b88`, which multiplies by a global the binary initializes at run time;
//! `ParIniDir`, which the parser turns into a quaternion with the
//! Euler-to-quaternion routine `0x7b6f10` rather than treating as a direction
//! vector; and the rotation the unbound emission basis takes from the
//! quaternion at emitter+0x74, whose writer is the same unknown that blocks
//! `ParIniDir`. A fourth is copied only for the class it is decoded for:
//! `IsSurface` places particles on the emitter's own `AreaSize` volume, and the
//! samplers of ids 123 (`0x727f00`, an ellipsoid) and 124 (`0x729be0`, a
//! cylinder) are reproduced, but ids 120 and 121 have samplers of their own
//! that are not.

use crate::{
    ecmodel,
    gfx_color_track::ColorTrack,
    gfx_file::{
        Child, Common, Control, ControlData, Cylinder, Element, Grid, KeyPoint, Lightning,
        Particle, Root, Spec, Sprite,
    },
};
use glam::{Mat4, Quat, Vec3};

/// Nesting limit for element-200 children. The client resolves child paths at
/// load time and has no cycle guard, so a self-referencing root would recurse
/// until the stack goes.
pub const MAX_CHILD_DEPTH: usize = 6;

/// Per-emitter particle ceiling. `Quota` is authoritative when smaller; this
/// only bounds a malformed value.
pub const MAX_PARTICLES: usize = 1024;

/// Per-ribbon vertex ceiling, for the same reason.
pub const MAX_SEGMENTS: usize = 512;

/// The verified static-decal alpha cutoff: below 5/255 the client drops the
/// quad instead of drawing an invisible one.
const ALPHA_CUTOFF: f32 = 5. / 255.;

/// One textured, blended quad in world space, ready for the effect pass.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectQuad {
    /// Archive path (`gfx/textures/...`), already normalized.
    pub texture: String,
    /// D3D8 `D3DBLEND` values, exactly as authored.
    pub source_blend: u32,
    pub destination_blend: u32,
    pub depth_test: bool,
    /// The element's `RenderLayer`; the pass sorts by it before blending.
    pub layer: i32,
    /// World-space corners in the client's quad order: top-left, top-right,
    /// bottom-left, bottom-right.
    pub positions: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// Linear RGB and alpha, already folded with the root and placement alpha.
    pub color: [f32; 4],
}

/// Camera basis handed to the player so billboards face the viewer.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub position: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
}

impl Default for View {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            right: Vec3::X,
            up: Vec3::Y,
            forward: Vec3::Z,
        }
    }
}

impl View {
    /// Recover the camera basis from an inverted combined view-projection
    /// matrix, which is all a renderer normally has on hand. The right-handed
    /// view matrix looks down its own negative Z axis.
    pub fn from_inverse_view_projection(inverse: Mat4) -> Self {
        if !inverse.is_finite() {
            return Self::default();
        }
        Self {
            position: inverse.transform_point3(Vec3::ZERO),
            right: inverse.x_axis.truncate().normalize_or_zero(),
            up: inverse.y_axis.truncate().normalize_or_zero(),
            forward: -inverse.z_axis.truncate().normalize_or_zero(),
        }
    }
}

/// Deterministic xorshift32. The client calls the CRT `rand()`; a fixed seed
/// keeps replays and tests reproducible instead of frame dependent.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.
    }

    /// Uniform in `[-1, 1]`.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2. - 1.
    }

    /// Uniform in the closed interval spanned by `low` and `high`. The two are
    /// not an ordered pair: 817 retail elements author `ScaleMin` above
    /// `ScaleMax` (0.5 down to 0.1), and `low + (high - low) * u` covers the same
    /// interval whichever way round they were written.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        if !low.is_finite() || !high.is_finite() {
            return 0.;
        }
        low + (high - low) * self.unit()
    }
}

/// What the player did this frame, for the status line and the audits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub quads: usize,
    pub particles: usize,
    pub emitters: usize,
    pub ribbons: usize,
    pub children: usize,
    /// Element 160: parsed and timed, but the model queue owns model drawing.
    pub models: usize,
    /// Element 170: parsed and timed; the viewer has no effect audio path.
    pub sounds: usize,
    /// Element 130 belongs to `gfx_light`, which this player does not duplicate.
    pub lights: usize,
    /// Element 190 and any id the client never instantiates.
    pub unrendered: usize,
}

impl Stats {
    fn merge(&mut self, other: &Stats) {
        self.quads += other.quads;
        self.particles += other.particles;
        self.emitters += other.emitters;
        self.ribbons += other.ribbons;
        self.children += other.children;
        self.models += other.models;
        self.sounds += other.sounds;
        self.lights += other.lights;
        self.unrendered += other.unrendered;
    }
}

/// A live particle, laid out like the client's node so the controller code
/// reads the same way as the disassembly.
#[derive(Clone, Copy, Debug)]
struct ParticleState {
    /// node+0x00
    age: f32,
    /// node+0x04
    ttl: f32,
    /// node+0x08: rotation in radians, which controller 101 drives.
    rotation: f32,
    /// node+0x0c
    color: u32,
    /// node+0x10
    position: Vec3,
    /// node+0x1c
    previous: Vec3,
    /// node+0x44
    direction: Vec3,
    /// node+0x50: accumulated displacement, controller 104's moving target.
    displacement: Vec3,
    /// node+0x5c: speed along `AccDir`.
    acc_speed: f32,
    /// node+0x60: speed along `direction`, seeded from `Speed`.
    speed: f32,
    /// node+0x6c
    scale: f32,
    /// Sub-integer carry of the colour controller, one per channel
    /// (ctrl+0x24..0x30).
    color_remainder: [f32; 4],
    seed: u32,
}

#[derive(Clone, Debug)]
struct Emitter {
    particles: Vec<ParticleState>,
    /// Fractional emission carry, so a 20/s emitter at 60 Hz emits one particle
    /// every third frame instead of twenty per frame.
    carry: f32,
    rng: Rng,
    /// One entry per affector: the client's 128-entry colour table, built once
    /// at load instead of once per particle per frame.
    ramps: Vec<Option<ColorTrack>>,
}

#[derive(Clone, Copy, Debug)]
struct RibbonVertex {
    position: Vec3,
    /// Milliseconds old.
    age: f32,
}

/// A history strip needs no randomness of its own: the wobble retail ribbons
/// show comes from the element's noise controllers, which move `placement`, and
/// therefore move every cross-section this strip records.
#[derive(Clone, Debug)]
struct RibbonState {
    vertices: Vec<RibbonVertex>,
    carry: f32,
}

#[derive(Clone, Debug)]
struct FractalState {
    /// Milliseconds since the last rebuild.
    timer: f32,
    /// One noise offset per strip vertex, regenerated on the authored interval.
    offsets: Vec<Vec3>,
    rng: Rng,
}

#[derive(Clone, Debug)]
struct BoltState {
    segments: Vec<(Vec3, Vec3, f32, f32)>,
    timer: f32,
    rng: Rng,
}

/// A nested root. `Player` is not `Clone` because its particle and strip state
/// must never be duplicated by accident: the client clones a root only to place
/// a second independent instance.
struct ChildState {
    player: Box<Player>,
    out_color: bool,
    loop_flag: bool,
    play_speed: f32,
}

/// Per-class runtime state.
enum Runtime {
    /// Stateless: the clock alone decides the geometry.
    Clock,
    Particles(Emitter),
    /// Elements 110 and 180: a strip through the element's own motion history.
    Ribbon(RibbonState),
    /// Elements 150 and 152: an authored strip from `StartPos` to `EndPos`,
    /// rebuilt and re-deformed every `Interval` milliseconds.
    Fractal(FractalState),
    Bolts(BoltState),
    Child(ChildState),
}

/// Controller state that survives frames. Everything here is a *delta* on the
/// sampled keypoint value, except the overrides, which replace it: that is what
/// the client's temp struct does when it is rebuilt from the keypoint each frame
/// and then modified by the controllers.
#[derive(Clone, Debug)]
struct CtrlState {
    offset: Vec3,
    velocity: Vec3,
    rotation: f32,
    orbit: f32,
    scale_extra: f32,
    color_offset: [i32; 4],
    /// Sub-integer carry of controller 105, one per channel (ctrl+0x24..0x30).
    color_remainder: [f32; 4],
    noise: Vec3,
    scale_override: Option<f32>,
    color_override: Option<u32>,
    path_override: Option<Vec3>,
    path_time: f32,
    ramp: Option<ColorTrack>,
    scale_ramp: Option<(f32, Vec<(f32, f32)>)>,
    path_points: Option<Vec<Vec3>>,
    path_direction: bool,
}

/// Resolve one element-200 nested root.
///
/// `MAX_CHILD_DEPTH` and the explicit path stack stop a cycle: the corpus has no
/// self-referencing child, but a hand-edited file could, and the client would
/// recurse until its stack went.
fn child_runtime(
    child: &Child,
    rng: &mut Rng,
    load_child: &mut dyn FnMut(&str) -> Result<Root, String>,
    depth: usize,
    stack: &mut Vec<String>,
) -> Result<ChildState, String> {
    if depth >= MAX_CHILD_DEPTH {
        return Err(format!(
            "GFX child {} nests deeper than {MAX_CHILD_DEPTH}",
            child.path
        ));
    }
    let path = child_path(&child.path)?;
    if stack.contains(&path) {
        return Err(format!("GFX child {path} references itself"));
    }
    let nested = load_child(&path).map_err(|error| format!("GFX child {path}: {error}"))?;
    stack.push(path);
    let player = Player::with_depth(nested, rng.next_u32(), load_child, depth + 1, stack)?;
    stack.pop();
    Ok(ChildState {
        player: Box::new(player),
        out_color: child.out_color,
        loop_flag: child.loop_flag,
        play_speed: child.play_speed,
    })
}

/// Build the client's 128-entry colour table for one ctrl-108 payload.
///
/// The container stores the controller bounds and every stop span in
/// milliseconds while `gfx_color_track::sample` takes seconds, so both are
/// converted here. `gfx_color_track` is the verified PC24 table and refuses a
/// non-positive span, and the corpus authors 414 zero spans, 14 `-1` spans and
/// 12 uninitialized ones; those stops are dropped and the rest of the ramp still
/// animates, which is closer to the retail picture than no ramp at all. A ramp
/// whose stops are all non-positive keeps the authored `Color` unchanged.
fn color_track(start: f32, end: f32, original: u32, stops: &[(u32, f32)]) -> Option<ColorTrack> {
    let start_s = start / 1000.;
    let end_s = end / 1000.;
    if let Ok(track) = ColorTrack::new(start_s, end_s, original, stops) {
        return Some(track);
    }
    let positive: Vec<(u32, f32)> = stops
        .iter()
        .copied()
        .filter(|(_, span)| span.is_finite() && *span > 0.)
        .collect();
    if positive.is_empty() {
        return None;
    }
    ColorTrack::new(start_s, end_s, original, &positive).ok()
}

impl CtrlState {
    fn new() -> Self {
        Self {
            offset: Vec3::ZERO,
            velocity: Vec3::ZERO,
            rotation: 0.,
            orbit: 0.,
            scale_extra: 0.,
            color_offset: [0; 4],
            color_remainder: [0.; 4],
            noise: Vec3::ZERO,
            scale_override: None,
            color_override: None,
            path_override: None,
            path_time: 0.,
            ramp: None,
            scale_ramp: None,
            path_points: None,
            path_direction: false,
        }
    }

    /// Bind the controller payloads that need a lookup table built once.
    fn bind(&mut self, key: &KeyPoint) {
        for control in &key.controls {
            match &control.data {
                ControlData::ColorRamp { original, stops } => {
                    self.ramp = color_track(control.start, control.end, *original, stops);
                }
                ControlData::ScaleRamp { first, stops } => {
                    self.scale_ramp = Some((*first, stops.clone()));
                }
                ControlData::Path { points, calc_dir } => {
                    self.path_points = Some(points.clone());
                    self.path_direction = *calc_dir;
                }
                _ => {}
            }
        }
    }
}

/// The sampled keypoint values for one frame, after interpolation.
#[derive(Clone, Copy, Debug)]
struct Sampled {
    position: Vec3,
    color: u32,
    scale: f32,
    direction: Quat,
    rad_2d: f32,
    /// Milliseconds into the active keypoint.
    offset: f32,
}

struct ElementRuntime {
    /// Index into `Root::elements`.
    index: usize,
    /// The keypoint the controller state was seeded from; -1 reseeds.
    seeded: isize,
    state: CtrlState,
    runtime: Runtime,
}

/// Everything one element needs to place and colour its geometry.
struct ElementContext<'a> {
    /// The element's authored id. The client branches on it everywhere: `0x723ab0`
    /// hands ids 120 to 125 six different emitter factories, and the surface
    /// samplers differ per class, so a divergence that applies to one class has to
    /// be keyed off the id rather than applied to all of them.
    id: u32,
    /// The parent basis, the root scale and the sampled keypoint orientation.
    placement: Mat4,
    view: &'a View,
    common: &'a Common,
    version: u32,
    /// Sampled keypoint colour after the controllers.
    color: u32,
    /// Root alpha, placement alpha and distance fade folded together.
    color_scale: f32,
    /// Sampled keypoint scale after the controllers.
    scale: f32,
    /// The sampled keypoint scale times the root `PlaySpeed`, *before* the
    /// controllers. The client multiplies exactly those two floats at `0x71a35b`
    /// (bound) and `0x71a501` (unbound) and hands the product to the basis
    /// builder as its one scalar argument; the affector update `0x716240` runs
    /// only afterwards, at `0x71a37c`, so a controller's scale override never
    /// reaches the emission volume.
    emission_scale: f32,
    rotation: f32,
    /// Milliseconds on the element clock, for flipbooks and UV scroll.
    clock: f32,
}

impl ElementContext<'_> {
    /// Final RGBA: the packed colour's channels times the folded alpha.
    fn rgba(&self) -> [f32; 4] {
        rgba(self.color, self.color_scale)
    }
}

/// Unpack a client ARGB dword and fold the root/placement alpha into it.
///
/// The fold happens in 0..255 units and truncates, because that is what the
/// verified static path does (`gfx_decal.rs:440`, from the client's own
/// per-vertex `D3DCOLOR` packing) — a sprite at `DefaultAlpha: 0.5` is 127/255,
/// not 127.5/255, and the animated player has to land on the same byte or the
/// two readers would disagree on every half-transparent effect.
fn rgba(color: u32, scale: f32) -> [f32; 4] {
    let alpha = (((color >> 24) & 0xff) as f32 * scale)
        .trunc()
        .clamp(0., 255.);
    [
        ((color >> 16) & 0xff) as f32 / 255.,
        ((color >> 8) & 0xff) as f32 / 255.,
        (color & 0xff) as f32 / 255.,
        alpha / 255.,
    ]
}

/// One live root instance.
pub struct Player {
    root: Root,
    /// Milliseconds on the root clock, already scaled by `PlaySpeed`.
    time: f32,
    finished: bool,
    elements: Vec<ElementRuntime>,
    quads: Vec<EffectQuad>,
    stats: Stats,
    rng: Rng,
    textures: Vec<String>,
    /// Elements that could not be resolved, most often a nested `GfxPath` the
    /// archive does not carry. 82 of the corpus's 1,530 child references (31
    /// distinct paths, for example `人物\技能\巫师\技能\…` with a doubled
    /// directory and `4品法宝_冷凝术_ 子效果.gfx` with a stray space) point at
    /// files that do not ship, so a missing child is a named warning and the rest
    /// of the root still plays rather than the whole effect being dropped.
    warnings: Vec<String>,
}

impl Player {
    /// Build a player. `load_child` resolves an element-200 `GfxPath` to a
    /// parsed root and is called during construction, exactly like the client's
    /// `A3DGFXMan::LoadGFXFromFile` resolving nested paths at load time.
    pub fn new(
        root: Root,
        seed: u32,
        load_child: &mut dyn FnMut(&str) -> Result<Root, String>,
    ) -> Result<Self, String> {
        Self::with_depth(root, seed, load_child, 0, &mut Vec::new())
    }

    /// Elements that could not be resolved at load time.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn with_depth(
        root: Root,
        seed: u32,
        load_child: &mut dyn FnMut(&str) -> Result<Root, String>,
        depth: usize,
        stack: &mut Vec<String>,
    ) -> Result<Self, String> {
        let mut rng = Rng::new(seed);
        let mut warnings: Vec<String> = Vec::new();
        let mut elements = Vec::with_capacity(root.elements.len());
        let mut textures = Vec::new();
        for (index, element) in root.elements.iter().enumerate() {
            if !element.common.texture.is_empty() {
                let path = texture_path(&element.common.texture)?;
                if !textures.contains(&path) {
                    textures.push(path);
                }
            }
            let runtime = match &element.spec {
                Spec::Particle(_) => Runtime::Particles(Emitter {
                    particles: Vec::new(),
                    carry: 0.,
                    rng: Rng::new(rng.next_u32()),
                    ramps: element
                        .affectors
                        .iter()
                        .map(|control| match &control.data {
                            ControlData::ColorRamp { original, stops } => {
                                color_track(control.start, control.end, *original, stops)
                            }
                            _ => None,
                        })
                        .collect(),
                }),
                Spec::Ribbon(_) | Spec::Wave(_) => Runtime::Ribbon(RibbonState {
                    vertices: Vec::new(),
                    carry: 0.,
                }),
                Spec::Fractal(_) | Spec::Trail(_) => Runtime::Fractal(FractalState {
                    timer: f32::INFINITY,
                    offsets: Vec::new(),
                    rng: Rng::new(rng.next_u32()),
                }),
                Spec::Lightning(_) => Runtime::Bolts(BoltState {
                    segments: Vec::new(),
                    timer: f32::INFINITY,
                    rng: Rng::new(rng.next_u32()),
                }),
                Spec::Child(child) => {
                    match child_runtime(child, &mut rng, load_child, depth, stack) {
                        Ok(state) => {
                            // A nested root's own unresolved children are reported
                            // through the parent, prefixed with the path that led
                            // there, so the warning names the whole chain.
                            for warning in state.player.warnings() {
                                warnings.push(format!("{}: {warning}", child.path));
                            }
                            Runtime::Child(state)
                        }
                        Err(error) => {
                            warnings.push(format!("element {index} ({}): {error}", child.path));
                            Runtime::Clock
                        }
                    }
                }
                _ => Runtime::Clock,
            };
            let mut state = CtrlState::new();
            if let Some(key) = element.keypoints.first() {
                state.bind(key);
            }
            elements.push(ElementRuntime {
                index,
                seeded: -1,
                state,
                runtime,
            });
        }
        Ok(Self {
            root,
            time: 0.,
            finished: false,
            elements,
            quads: Vec::new(),
            stats: Stats::default(),
            rng,
            warnings,
            textures,
        })
    }

    /// Textures this root needs, so the pass can upload them before frame one.
    pub fn textures(&self) -> &[String] {
        &self.textures
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn quads(&self) -> &[EffectQuad] {
        &self.quads
    }

    /// True once a play-once root has run past its last keypoint.
    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn time_ms(&self) -> f32 {
        self.time
    }

    /// Restart the clock. Live particles stay, exactly as they do when the
    /// client restarts a looping root.
    pub fn reset(&mut self) {
        self.time = 0.;
        self.finished = false;
        for element in &mut self.elements {
            element.seeded = -1;
            element.state = CtrlState::new();
        }
    }

    /// Advance the whole root by `dt_ms` milliseconds and rebuild the quad list.
    /// `transform` places the root in the world; `scale` and `alpha` are the
    /// placement's own multipliers (an ECM event's `GfxScale` and `GfxAlpha`, or
    /// 1.0 for a scene placement).
    pub fn advance(
        &mut self,
        dt_ms: f32,
        view: &View,
        transform: Mat4,
        scale: f32,
        alpha: f32,
    ) -> Result<(), String> {
        if !dt_ms.is_finite() || dt_ms < 0. {
            return Err(format!("invalid effect delta {dt_ms}"));
        }
        if !scale.is_finite() || scale <= 0. {
            return Err(format!("invalid effect scale {scale}"));
        }
        if !alpha.is_finite() || alpha < 0. {
            return Err(format!("invalid effect alpha {alpha}"));
        }
        if !transform.is_finite() {
            return Err("non-finite effect transform".into());
        }
        self.time += dt_ms * self.root.play_speed;
        let mut rng = self.rng;
        rng.next_u32();
        let mut stats = Stats::default();
        let mut out = Vec::new();

        // The parent basis is normalized exactly like the verified static decal
        // path, so a non-uniform placement cannot shear the billboards.
        let base = normalize_basis(transform)
            * Mat4::from_scale(Vec3::splat(self.root.default_scale * scale));
        let color_scale =
            self.root.default_alpha * alpha * distance_fade(&self.root, view.position, base);
        let time = self.time;
        let play_speed = self.root.play_speed;
        let version = self.root.version;
        let face_to_viewer = self.root.face_to_viewer;

        // The controllers integrate in seconds of scaled time: the same
        // millisecond delta the root clock used, times the root `PlaySpeed`
        // (0x71a217 reads root+0x1ec), divided by 1000.
        let dt_s = dt_ms * play_speed / 1000.;
        // Every element has to be over: a root with a 100 ms flash next to a
        // one second sprite is still playing while the flash is done.
        let mut finished = !self.elements.is_empty();
        for runtime in &mut self.elements {
            let element = &self.root.elements[runtime.index];
            let done = advance_element(
                element,
                runtime,
                time,
                dt_s,
                play_speed,
                version,
                face_to_viewer,
                view,
                base,
                color_scale,
                &mut rng,
                &mut stats,
                &mut out,
            )?;
            finished = finished && done;
        }
        self.rng = rng;
        stats.quads = out.len();
        self.stats = stats;
        self.quads = out;
        self.finished = finished && !self.loops();
        Ok(())
    }

    /// True when some element repeats open-endedly, so the root never reports
    /// itself finished however long it runs.
    fn loops(&self) -> bool {
        self.root
            .elements
            .iter()
            .any(|element| element.common.repeat_count <= 0)
    }
}

/// Advance one element; returns true when its play-once clock has run out.
#[allow(clippy::too_many_arguments)]
fn advance_element(
    element: &Element,
    runtime: &mut ElementRuntime,
    time: f32,
    dt_s: f32,
    play_speed: f32,
    version: u32,
    face_to_viewer: bool,
    view: &View,
    base: Mat4,
    color_scale: f32,
    rng: &mut Rng,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) -> Result<bool, String> {
    let local = time - element.start_time;
    if local < 0. {
        return Ok(false);
    }
    let cycle = element_cycle(element);
    let (phase, done) = element_phase(element, local, cycle);
    if done {
        return Ok(true);
    }
    let Some((index, key, sampled)) = sample_keypoints(element, phase) else {
        // A phase no keypoint covers — the gap of a `RepeatDelay`, or the tail
        // of a finite clock — draws nothing, but it does not end the element:
        // `element_phase` is the only authority on that, and it already reported
        // `done` above when the play count ran out.
        return Ok(false);
    };
    if runtime.seeded != index as isize {
        runtime.seeded = index as isize;
        runtime.state = CtrlState::new();
        runtime.state.bind(key);
    }
    apply_controls(&mut runtime.state, key, &sampled, dt_s, rng);

    let scale = runtime
        .state
        .scale_override
        .unwrap_or(sampled.scale + runtime.state.scale_extra);
    let color = runtime.state.color_override.unwrap_or(sampled.color);
    let offset = runtime
        .state
        .path_override
        .unwrap_or(runtime.state.offset + runtime.state.noise);
    // The interpolated keypoint orientation, not the raw one: a root that
    // authors two keypoints with different `Direction` quaternions turns
    // smoothly between them, and ctrl 100's `Dir` orbit spins on top of that.
    let direction = sampled.direction * Quat::from_axis_angle(Vec3::Z, runtime.state.orbit);
    let placement = base * Mat4::from_rotation_translation(direction, sampled.position + offset);
    let context = ElementContext {
        id: element.id,
        placement,
        view,
        common: &element.common,
        version,
        color: apply_color_offset(color, &runtime.state.color_offset),
        color_scale,
        scale: if scale.is_finite() && scale > 0. {
            scale
        } else {
            1.
        },
        emission_scale: sampled.scale * play_speed,
        // `Rad_2D` is the authored 2D spin of this keypoint; ctrl 101
        // integrates its `Vel`/`Acc` into the same angle (node+0x08 is a
        // rotation, not a speed).
        rotation: sampled.rad_2d + runtime.state.rotation,
        clock: phase,
    };
    match &mut runtime.runtime {
        Runtime::Clock => emit_clock(element, &context, face_to_viewer, stats, out),
        Runtime::Particles(emitter) => {
            let Spec::Particle(particle) = &element.spec else {
                return Err("particle runtime on a non-particle element".into());
            };
            emit_particles(
                emitter,
                particle,
                &element.affectors,
                element.generation_sequence.as_deref(),
                &context,
                dt_s,
                stats,
                out,
            );
        }
        Runtime::Ribbon(ribbon) => emit_ribbon(ribbon, element, &context, dt_s, stats, out),
        Runtime::Fractal(fractal) => emit_fractal(fractal, element, &context, dt_s, stats, out),
        Runtime::Bolts(bolts) => {
            let Spec::Lightning(bolt) = &element.spec else {
                return Err("bolt runtime on a non-lightning element".into());
            };
            emit_bolts(bolts, bolt, &context, dt_s, stats, out);
        }
        Runtime::Child(child) => {
            let child_alpha = if child.out_color {
                color_scale * ((context.color >> 24) & 0xff) as f32 / 255.
            } else {
                color_scale
            };
            let child_dt = dt_s * 1000. * child.play_speed;
            if child.loop_flag || !child.player.finished() {
                child
                    .player
                    .advance(child_dt, view, placement, context.scale, child_alpha)?;
                out.extend(child.player.quads.iter().cloned());
                stats.merge(&child.player.stats);
            }
            stats.children += 1;
        }
    }
    Ok(done)
}

/// Total length of one play of the element clock, including the repeat delay.
fn element_cycle(element: &Element) -> f32 {
    match element.duration_ms() {
        Some(span) => span + element.common.repeat_delay.max(0.),
        None => f32::INFINITY,
    }
}

/// Map an element's local clock onto its keypoint clock, honouring
/// `RepeatCount`: 1 plays once, -1 loops forever, N plays N times.
fn element_phase(element: &Element, local: f32, cycle: f32) -> (f32, bool) {
    if !cycle.is_finite() {
        return (local, false);
    }
    if cycle <= 0. {
        return (0., element.common.repeat_count == 1);
    }
    let repeats = element.common.repeat_count;
    if repeats == 1 {
        return (local.min(cycle), local >= cycle);
    }
    // The corpus only ever writes 1, -1 or a positive count (up to INT_MAX for
    // "effectively forever"), so a zero or negative count is read as an
    // open-ended loop rather than as "play zero times".
    let total = if repeats <= 0 {
        f32::INFINITY
    } else {
        cycle * repeats as f32
    };
    if local >= total {
        return (cycle, true);
    }
    (local % cycle, false)
}

/// The active keypoint, its index and the interpolated values for this frame.
fn sample_keypoints(element: &Element, phase: f32) -> Option<(usize, &KeyPoint, Sampled)> {
    let mut clock = 0f32;
    for (index, key) in element.keypoints.iter().enumerate() {
        let span = if key.time_span < 0. {
            f32::INFINITY
        } else {
            key.time_span
        };
        if span.is_infinite() || phase < clock + span {
            let offset = (phase - clock).max(0.);
            let sampled = if key.interpolate_mode == 0 || span.is_infinite() {
                Sampled {
                    position: key.position,
                    color: key.color,
                    scale: key.scale,
                    direction: key.direction,
                    rad_2d: key.rad_2d,
                    offset,
                }
            } else {
                let blend = (offset / span).clamp(0., 1.);
                match element.keypoints.get(index + 1) {
                    Some(next) => Sampled {
                        position: key.position.lerp(next.position, blend),
                        color: blend_color(key.color, next.color, blend),
                        scale: key.scale + (next.scale - key.scale) * blend,
                        direction: key.direction.slerp(next.direction, blend),
                        rad_2d: key.rad_2d + (next.rad_2d - key.rad_2d) * blend,
                        offset,
                    },
                    None => Sampled {
                        position: key.position,
                        color: key.color,
                        scale: key.scale,
                        direction: key.direction,
                        rad_2d: key.rad_2d,
                        offset,
                    },
                }
            };
            return Some((index, key, sampled));
        }
        clock += span;
    }
    None
}

/// Channel-wise blend of two packed colours, rounded to the nearest byte the
/// way the client's ramp table rounds.
fn blend_color(from: u32, to: u32, blend: f32) -> u32 {
    let mut packed = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let a = ((from >> shift) & 0xff) as f32;
        let b = ((to >> shift) & 0xff) as f32;
        let value = (a + (b - a) * blend).round().clamp(0., 255.) as u32;
        packed |= value << shift;
    }
    packed
}

/// The order controller 105's four integers index the packed colour in.
///
/// `0x6ebfab` unpacks temp+0x0c into four bytes — bits 16..23 first, then 8..15,
/// then 0..7, then 24..31 — and `0x6ec03f` packs them back as
/// `alpha<<24 | red<<16 | green<<8 | blue`. So `ColorDelta`'s first number is
/// red and its fourth is alpha, which is why the corpus writes fades as
/// `ColorDelta: 0, 0, 0, -1000`.
const COLOR_SHIFTS: [u32; 4] = [16, 8, 0, 24];

/// One channel of one controller-105 step, decoded at `0x6ebfd4`.
///
/// The rate is multiplied by the frame delta, the channel's own sub-integer
/// remainder is added, the integer part is applied to the byte and the fraction
/// is carried to the next frame — the remainders live at ctrl+0x24, +0x28, +0x2c
/// and +0x30, one per channel. When the byte saturates the client clears that
/// channel's remainder (`0x6ec00b` and `0x6ec02a`), so a fade that has hit the
/// floor does not resume from a large accumulated fraction.
fn color_delta(channel: u8, rate: i32, dt: f32, remainder: &mut f32) -> u8 {
    if rate == 0 {
        return channel;
    }
    let accumulated = rate as f32 * dt + *remainder;
    let steps = accumulated.trunc() as i32;
    *remainder = accumulated - steps as f32;
    let summed = channel as i32 + steps;
    if summed > 255 {
        *remainder = 0.;
        return 255;
    }
    if summed < 0 {
        *remainder = 0.;
        return 0;
    }
    summed as u8
}

fn apply_color_offset(color: u32, offset: &[i32; 4]) -> u32 {
    let mut packed = 0u32;
    for (index, shift) in COLOR_SHIFTS.into_iter().enumerate() {
        let channel = ((color >> shift) & 0xff) as i32 + offset[index];
        let channel = channel.clamp(0, 255) as u32;
        packed |= channel << shift;
    }
    packed
}

/// Run every controller of the active keypoint for one frame.
fn apply_controls(
    state: &mut CtrlState,
    key: &KeyPoint,
    sampled: &Sampled,
    dt: f32,
    rng: &mut Rng,
) {
    if dt <= 0. {
        return;
    }
    let age = sampled.offset / 1000.;
    for control in &key.controls {
        let age_ms = sampled.offset;
        if age_ms < control.start {
            continue;
        }
        if control.end >= 0. && age_ms > control.end {
            continue;
        }
        // The controllers integrate against their own clock, which starts at
        // their StartTime: `0x6eaf60` reads temp+0x34, the node age.
        let local = (age_ms - control.start) / 1000.;
        match &control.data {
            ControlData::Motion { dir, vel, acc } => {
                let dspl = displacement(*vel, *acc, local, dt);
                state.offset += *dir * dspl;
                state.velocity += *dir * dspl;
            }
            ControlData::Speed { vel, acc } => {
                state.rotation += displacement(*vel, *acc, local, dt);
            }
            ControlData::Orbit {
                pos,
                axis,
                vel,
                acc,
            } => {
                let dspl = displacement(*vel, *acc, local, dt);
                state.orbit += dspl;
                let axis = if axis.length_squared() > 1e-12 {
                    axis.normalize()
                } else {
                    Vec3::Y
                };
                let rotation = Quat::from_axis_angle(axis, dspl);
                state.offset = rotation * (state.offset - *pos) + *pos;
            }
            ControlData::Attract { pos, vel, acc } => {
                if *vel == -1. {
                    continue;
                }
                let dspl = displacement(*vel, *acc, local, dt);
                let target = *pos + state.velocity;
                if dspl == -1. {
                    // The client's snap sentinel at 0x6ebd0a.
                    state.offset = target;
                    continue;
                }
                let delta = state.offset - target;
                let length = delta.length();
                if length > 1e-6 {
                    state.offset += delta / length * dspl;
                }
            }
            ControlData::ColorDelta { delta } => {
                // The offsets are folded into the keypoint colour on the way
                // out, so the byte being mutated is the one currently on screen.
                let base = state.color_override.unwrap_or(sampled.color);
                for (index, rate) in delta.iter().enumerate() {
                    let shift = COLOR_SHIFTS[index];
                    let shown = (((base >> shift) & 0xff) as i32 + state.color_offset[index])
                        .clamp(0, 255) as u8;
                    let updated = color_delta(shown, *rate, dt, &mut state.color_remainder[index]);
                    state.color_offset[index] += updated as i32 - shown as i32;
                }
            }
            ControlData::ScaleChange { rate, min, max } => {
                let total = (sampled.scale + state.scale_extra + rate * dt).clamp(*min, *max);
                state.scale_extra = total - sampled.scale;
            }
            ControlData::Fractal {
                amplitude,
                wave_length,
                persistence,
                octaves,
                buffer_length,
                ..
            } => {
                // The noise controllers displace the element by an fBm sample
                // of its own clock. The client's table generator was not
                // located; the parameters are the authored ones.
                let sample = fbm(
                    rng.next_u32(),
                    age,
                    *wave_length,
                    *persistence,
                    (*octaves).max(1) as usize,
                    *buffer_length,
                );
                state.noise = Vec3::new(sample, sample * 0.5, sample * 0.25) * *amplitude;
            }
            ControlData::ColorRamp { .. } => {
                // Open question, recorded in `Findings/`: `ColorTrack::sample`
                // gates on its own `start`/`end` and then walks its tables from
                // zero, so shifting the argument moves the gate too — while
                // `gfx_light` samples the same track type with unshifted
                // keypoint seconds, and controllers 100/101 were read as gating
                // on `age < StartTime` while integrating absolute `age`.
                // Settling it needs the controller-108 update routine; nothing
                // is changed here on a guess.
                if let Some(track) = &state.ramp {
                    state.color_override = track.sample(age - control.start / 1000.);
                }
            }
            ControlData::ScaleRamp { .. } => {
                if let Some((first, stops)) = &state.scale_ramp {
                    state.scale_override =
                        Some(sample_ramp(*first, stops, age - control.start / 1000.));
                }
            }
            ControlData::Path { .. } => {
                let Some(points) = &state.path_points else {
                    continue;
                };
                if points.len() < 2 {
                    continue;
                }
                state.path_time += dt;
                let total = path_length(points);
                if total <= 0. {
                    continue;
                }
                let (position, tangent) = sample_path(points, state.path_time % total);
                state.path_override = Some(position);
                if state.path_direction {
                    state.rotation = tangent.angle_between(Vec3::X);
                }
            }
        }
    }
}

/// The client's displacement integral, shared by controllers 100, 101, 102, 103
/// and 104: `v = vel + acc*t`, `dspl = v*dt + 0.5*(2*v*dt + acc*dt*dt)`.
/// One controller step, in seconds.
///
/// The controllers integrate their velocity trapezoidally over the frame: with
/// `v = Vel + Acc * age` the displacement is `0.5 * (2*v*dt + Acc*dt²)`, which is
/// the mean of the velocities at both ends of the interval times `dt`, and which
/// reduces to plain `v*dt` when `Acc` is zero. That reduction is what pins the
/// scale: ctrl 101's retail `Vel: 1.745329` is 100 degrees per second expressed
/// in radians, so a zero-acceleration controller must turn exactly that fast.
fn displacement(vel: f32, acc: f32, age: f32, dt: f32) -> f32 {
    let v = vel + acc * age;
    0.5 * (2. * v * dt + acc * dt * dt)
}

fn sample_ramp(first: f32, stops: &[(f32, f32)], seconds: f32) -> f32 {
    let mut remaining = seconds.max(0.) * 1000.;
    let mut from = first;
    for (value, duration) in stops {
        if *duration <= 0. {
            from = *value;
            continue;
        }
        if remaining < *duration {
            return from + (*value - from) * (remaining / duration).clamp(0., 1.);
        }
        remaining -= duration;
        from = *value;
    }
    from
}

fn path_length(points: &[Vec3]) -> f32 {
    points
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).length())
        .sum()
}

fn sample_path(points: &[Vec3], distance: f32) -> (Vec3, Vec3) {
    let mut remaining = distance.max(0.);
    for pair in points.windows(2) {
        let delta = pair[1] - pair[0];
        let length = delta.length();
        if length <= 1e-9 {
            continue;
        }
        if remaining <= length {
            let direction = delta / length;
            return (pair[0] + direction * remaining, direction);
        }
        remaining -= length;
    }
    let last = points.last().copied().unwrap_or(Vec3::ZERO);
    let direction = points
        .last()
        .zip(points.first())
        .map(|(last, first)| (*last - *first))
        .filter(|delta| delta.length_squared() > 1e-9)
        .map_or(Vec3::X, |delta| delta.normalize());
    (last, direction)
}

/// fBm value noise over a permutation of `buffer_length` entries with the
/// authored wavelength, persistence and octave count.
fn fbm(
    seed: u32,
    x: f32,
    wave_length: f32,
    persistence: f32,
    octaves: usize,
    buffer_length: u32,
) -> f32 {
    if !x.is_finite() || octaves == 0 {
        return 0.;
    }
    let length = buffer_length.max(2) as i64;
    let mut amplitude = 1.;
    let mut frequency = 1. / wave_length.abs().max(1e-3);
    let mut total = 0.;
    let mut norm = 0.;
    for octave in 0..octaves.min(16) {
        let scaled = x * frequency;
        let index = scaled.floor() as i64;
        let fraction = scaled - index as f32;
        let a = noise_at(seed, index, length, octave);
        let b = noise_at(seed, index + 1, length, octave);
        // Smoothstep, the classic Perlin fade curve.
        let blend = fraction * fraction * (3. - 2. * fraction);
        total += (a + (b - a) * blend) * amplitude;
        norm += amplitude;
        amplitude *= persistence;
        frequency *= 2.;
    }
    if norm <= 0. {
        0.
    } else {
        total / norm
    }
}

fn noise_at(seed: u32, index: i64, length: i64, octave: usize) -> f32 {
    let wrapped = index.rem_euclid(length) as u32;
    let mut x = seed
        .wrapping_mul(1_664_525)
        .wrapping_add(wrapped.wrapping_mul(1_013_904_223))
        .wrapping_add((octave as u32).wrapping_mul(0x9e37_79b9));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 32_768. - 1.
}

/// Normalize a placement basis, exactly as the verified static decal path does,
/// so a scaled parent cannot shear the billboards.
fn normalize_basis(parent: Mat4) -> Mat4 {
    let axis = |axis: Vec3| -> Vec3 {
        let length = axis.length();
        if length.is_finite() && length > 1e-12 {
            axis / length
        } else {
            Vec3::ZERO
        }
    };
    Mat4::from_cols(
        axis(parent.x_axis.truncate()).extend(0.),
        axis(parent.y_axis.truncate()).extend(0.),
        axis(parent.z_axis.truncate()).extend(0.),
        parent.w_axis,
    )
}

/// `FadeByDist` fades the root out between its authored distances, measured from
/// the camera to the root origin.
fn distance_fade(root: &Root, camera: Vec3, base: Mat4) -> f32 {
    if !root.fade_by_dist {
        return 1.;
    }
    let distance = (camera - base.transform_point3(Vec3::ZERO)).length();
    let span = root.fade_end - root.fade_start;
    if span <= 0. {
        return if distance <= root.fade_start { 1. } else { 0. };
    }
    (1. - (distance - root.fade_start) / span).clamp(0., 1.)
}

/// Texture path of an element, resolved against the archive's texture root the
/// same way `gfx_decal` resolves it (`Gfx\Textures` in the client).
pub fn texture_path(texture: &str) -> Result<String, String> {
    let path = ecmodel::normalize(texture);
    if path.is_empty() || !is_safe(&path) {
        return Err(format!("unsafe GFX texture path: {texture}"));
    }
    Ok(format!("gfx/textures/{path}"))
}

/// Archive path of a child root (`GfxPath`), relative to the `gfx` root.
pub fn child_path(path: &str) -> Result<String, String> {
    let normalized = ecmodel::normalize(path);
    if normalized.is_empty() || !is_safe(&normalized) {
        return Err(format!("unsafe GFX child path: {path}"));
    }
    Ok(if normalized.starts_with("gfx/") {
        normalized
    } else {
        format!("gfx/{normalized}")
    })
}

fn is_safe(path: &str) -> bool {
    !path
        .split('/')
        .any(|part| part == "." || part == ".." || part.contains(':'))
}

/// The flipbook cell for this clock, plus the authored UV scroll and flips.
/// `TexRow`/`TexCol` greater than one select a cell every `TexInterval`
/// milliseconds, row by row, which is how the retail smoke and fire sequences
/// are stored.
fn flipbook_uv(common: &Common, clock_ms: f32) -> [[f32; 2]; 4] {
    let rows = common.tex_row.max(1);
    let cols = common.tex_col.max(1);
    let cells = rows * cols;
    let mut uv = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
    if cells > 1 {
        let interval = if common.tex_interval == 0 {
            1.
        } else {
            common.tex_interval as f32
        };
        let limit = if common.tex_anim_max_time > 0. {
            ((common.tex_anim_max_time / interval) as u32).clamp(1, cells)
        } else {
            cells
        };
        let index = ((clock_ms.max(0.) / interval).floor() as u32) % limit;
        let row = index / cols;
        let col = index % cols;
        let u0 = col as f32 / cols as f32;
        let v0 = row as f32 / rows as f32;
        let du = 1. / cols as f32;
        let dv = 1. / rows as f32;
        uv = [[u0, v0], [u0 + du, v0], [u0, v0 + dv], [u0 + du, v0 + dv]];
    }
    // Two `TexSpeed` values under one key scroll U and V continuously.
    let scroll_u = common.tex_speed[0] * clock_ms / 1000.;
    let scroll_v = common.tex_speed[1] * clock_ms / 1000.;
    for point in &mut uv {
        point[0] += scroll_u;
        point[1] += scroll_v;
    }
    if common.reverse_u {
        for point in &mut uv {
            point[0] = 1. - point[0];
        }
    }
    if common.reverse_v {
        for point in &mut uv {
            point[1] = 1. - point[1];
        }
    }
    if common.exchange_uv {
        for point in &mut uv {
            point.swap(0, 1);
        }
    }
    uv
}

/// The static decal path proved this gate: before version 76 the client always
/// depth tests, from 76 on `ZEnable` decides.
fn depth_test(common: &Common, version: u32) -> bool {
    version < 76 || common.depth_test
}

fn push_quad(
    out: &mut Vec<EffectQuad>,
    context: &ElementContext,
    texture: &str,
    positions: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    color: [f32; 4],
) {
    if texture.is_empty() {
        return;
    }
    if positions
        .iter()
        .flatten()
        .chain(uv.iter().flatten())
        .chain(color.iter())
        .any(|value| !value.is_finite())
    {
        return;
    }
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    out.push(EffectQuad {
        texture: texture.to_string(),
        source_blend: context.common.source_blend,
        destination_blend: context.common.destination_blend,
        depth_test: depth_test(context.common, context.version),
        layer: context.common.render_layer,
        positions,
        uv,
        color,
    });
}

/// The stateless classes: sprites, cylinders and grids. Lights belong to
/// `gfx_light`, models to the model queue and sounds to the audio path; all
/// three are counted so the status line stays honest.
fn emit_clock(
    element: &Element,
    context: &ElementContext,
    face_to_viewer: bool,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) {
    match &element.spec {
        Spec::Sprite(sprite) => emit_sprite(sprite, context, face_to_viewer, out),
        Spec::Cylinder(cylinder) => emit_cylinder(cylinder, context, out),
        Spec::Grid(grid) => emit_grid(grid, context, out),
        Spec::Light(_) => stats.lights += 1,
        Spec::Sound(_) => stats.sounds += 1,
        Spec::Model(_) => stats.models += 1,
        Spec::Curve(_) | Spec::Unknown { .. } => stats.unrendered += 1,
        // These classes carry their own runtime and never reach this function.
        Spec::Ribbon(_)
        | Spec::Trail(_)
        | Spec::Fractal(_)
        | Spec::Wave(_)
        | Spec::Particle(_)
        | Spec::Lightning(_)
        | Spec::Child(_) => {}
    }
}

fn emit_sprite(
    sprite: &Sprite,
    context: &ElementContext,
    face_to_viewer: bool,
    out: &mut Vec<EffectQuad>,
) {
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    // `RotFromView` decides where the orientation comes from: 598 retail sprites
    // take it from the camera, the other 8,271 keep the authored keypoint
    // direction, which is what the verified static path does. The root's
    // `FaceToViewer` forces the camera basis for every sprite in the root.
    let billboard = sprite.rotate_from_view || face_to_viewer;
    let basis = if billboard {
        QuadBasis::Billboard {
            right: context.view.right,
            up: context.view.up,
            forward: context.view.forward,
        }
    } else {
        QuadBasis::Element
    };
    let positions = quad_corners(
        context,
        basis,
        Vec3::new(0., 0., sprite.z_offset),
        sprite.width,
        sprite.height,
        sprite.origin,
        sprite.no_scale,
        context.rotation,
        1.,
    );
    let uv = flipbook_uv(context.common, context.clock);
    push_quad(out, context, &texture, positions, uv, color);
}

/// The four world-space corners of a quad, using the anchor and per-axis
/// `NoScale` rules the verified static decal path proved: the quad spans
/// `2*Width` by `2*Height` with its anchor at the `OrgPt` fractions.
#[allow(clippy::too_many_arguments)]
/// Where a quad's plane comes from.
#[derive(Clone, Copy)]
enum QuadBasis {
    /// Camera-facing: the quad spans the given view axes and its 2D spin is
    /// about the view forward, so it never leaves the screen plane.
    Billboard {
        right: Vec3,
        up: Vec3,
        forward: Vec3,
    },
    /// Element-space: the quad lies in the element's own XY plane and its 2D
    /// spin is about its own Z axis, exactly like the verified static decal.
    Element,
}

/// Build the four world-space corners of one quad, in the client's order
/// (top-left, top-right, bottom-left, bottom-right).
///
/// `extra` multiplies both axes on top of the sampled keypoint scale: particles
/// carry a scale of their own (the emitter's `ScaleMin`/`ScaleMax` range plus
/// the `ScaleChage` controller), which `NoScale` does not suppress.
#[allow(clippy::too_many_arguments)]
fn quad_corners(
    context: &ElementContext,
    basis: QuadBasis,
    center: Vec3,
    width: f32,
    height: f32,
    origin: [f32; 2],
    no_scale: [bool; 2],
    rotation: f32,
    extra: f32,
) -> [[f32; 3]; 4] {
    let scale_x = (if no_scale[0] { 1. } else { context.scale }) * extra;
    let scale_y = (if no_scale[1] { 1. } else { context.scale }) * extra;
    // The corner offsets must match the verified static decal exactly, because
    // that is the one sprite path with native evidence behind it. `gfx_decal`
    // doubles the authored dimensions and splits them at `OrgPt`:
    //   width  = [w0, 2w - w0]   with w0 = Width  * 2 * OrgPt[0]
    //   height = [h0, 2h - h0]   with h0 = Height * 2 * OrgPt[1]
    // and its corners are (-w0, +height[1]), (+width[1], +height[1]),
    // (-w0, -height[0]), (+width[1], -height[0]) — so the *top* extent is the
    // complement of the origin split, not the split itself. Only a pivot away
    // from the centre shows the difference, and 704 corpus OrgPt lines are off
    // centre.
    let w0 = width * 2. * origin[0];
    let w1 = width * 2. - w0;
    let h0 = height * 2. * origin[1];
    let h1 = height * 2. - h0;
    let local = [
        Vec3::new(-w0 * scale_x, h1 * scale_y, 0.),
        Vec3::new(w1 * scale_x, h1 * scale_y, 0.),
        Vec3::new(-w0 * scale_x, -h0 * scale_y, 0.),
        Vec3::new(w1 * scale_x, -h0 * scale_y, 0.),
    ];
    match basis {
        // Element space: the placement carries the parent basis, the root and
        // keypoint transforms, so the corners only need the 2D spin. This is the
        // verified `parent * rotation_translation(direction, position) * local`.
        QuadBasis::Element => {
            let spin = if rotation == 0. {
                Mat4::IDENTITY
            } else {
                Mat4::from_rotation_z(rotation)
            };
            let transform = context.placement * spin;
            local.map(|corner| transform.transform_point3(corner + center).to_array())
        }
        // A billboard's axes are unit length, so the uniform parent scale they
        // were normalized out of has to be put back on the extents.
        QuadBasis::Billboard { right, up, forward } => {
            let basis_scale = context.placement.x_axis.truncate().length();
            let (right, up) = if rotation == 0. {
                (right, up)
            } else {
                let spin = Quat::from_axis_angle(forward, rotation);
                (spin * right, spin * up)
            };
            let world = context.placement.transform_point3(center);
            local.map(|corner| {
                (world + right * (corner.x * basis_scale) + up * (corner.y * basis_scale))
                    .to_array()
            })
        }
    }
}

fn emit_cylinder(cylinder: &Cylinder, context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let sections = cylinder.sections.max(3) as usize;
    let radius = cylinder.radius
        * if cylinder.no_radius_scale {
            1.
        } else {
            context.scale
        };
    let height = cylinder.height
        * if cylinder.no_height_scale {
            1.
        } else {
            context.scale
        };
    if !(radius.is_finite() && height.is_finite()) || radius <= 0. || height <= 0. {
        return;
    }
    let base_y = if cylinder.origin_at_center {
        -height * 0.5
    } else {
        0.
    };
    let matrix = context.placement;
    let uv = flipbook_uv(context.common, context.clock);
    let mut positions = [[0f32; 3]; 4];
    for index in 0..sections {
        let a0 = index as f32 / sections as f32 * std::f32::consts::TAU + cylinder.pitch;
        let a1 = (index + 1) as f32 / sections as f32 * std::f32::consts::TAU + cylinder.pitch;
        let corners = [
            Vec3::new(a0.cos() * radius, base_y, a0.sin() * radius),
            Vec3::new(a1.cos() * radius, base_y, a1.sin() * radius),
            Vec3::new(a1.cos() * radius, base_y + height, a1.sin() * radius),
            Vec3::new(a0.cos() * radius, base_y + height, a0.sin() * radius),
        ];
        for (slot, corner) in corners.into_iter().enumerate() {
            positions[slot] = matrix.transform_point3(corner).to_array();
        }
        let u0 = index as f32 / sections as f32;
        let u1 = (index + 1) as f32 / sections as f32;
        let strip = [
            [u0 + uv[0][0], uv[0][1]],
            [u1 + uv[0][0], uv[1][1]],
            [u0 + uv[2][0], uv[2][1]],
            [u1 + uv[2][0], uv[3][1]],
        ];
        push_quad(out, context, &texture, positions, strip, color);
    }
}

fn emit_grid(grid: &Grid, context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let matrix = context.placement;
    let cell = grid.cell_size * context.scale;
    if !(cell.is_finite()) || cell <= 0. {
        return;
    }
    let half = cell * 0.5;
    let mut positions = [[0f32; 3]; 4];
    let uv = flipbook_uv(context.common, context.clock);
    for row in 0..grid.rows {
        for col in 0..grid.columns {
            let index = (row * grid.columns + col) as usize;
            let Some(value) = grid.colors.get(index) else {
                continue;
            };
            let color = rgba(*value, context.color_scale);
            if color[3] < ALPHA_CUTOFF {
                continue;
            }
            let x = (col as f32 - (grid.columns as f32 - 1.) * 0.5) * cell;
            let z = (row as f32 - (grid.rows as f32 - 1.) * 0.5) * cell;
            let corners = [
                Vec3::new(x - half, grid.z_offset, z - half),
                Vec3::new(x + half, grid.z_offset, z - half),
                Vec3::new(x - half, grid.z_offset, z + half),
                Vec3::new(x + half, grid.z_offset, z + half),
            ];
            for (slot, corner) in corners.into_iter().enumerate() {
                positions[slot] = matrix.transform_point3(corner).to_array();
            }
            push_quad(out, context, &texture, positions, uv, color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_particles(
    emitter: &mut Emitter,
    particle: &Particle,
    affectors: &[Control],
    generation: Option<&KeyPoint>,
    context: &ElementContext,
    dt: f32,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) {
    stats.emitters += 1;
    let Emitter {
        particles,
        carry,
        rng,
        ramps,
    } = emitter;
    let quota = if particle.quota > 0 {
        (particle.quota as usize).min(MAX_PARTICLES)
    } else {
        MAX_PARTICLES
    };
    if particle.emission_rate > 0. && dt > 0. {
        *carry += particle.emission_rate * dt;
        let mut count = carry.trunc() as usize;
        *carry -= count as f32;
        let room = quota.saturating_sub(particles.len());
        if count > room {
            count = room;
        }
        let path = generation.and_then(|key| {
            key.controls.iter().find_map(|control| match &control.data {
                ControlData::Path { points, .. } if points.len() > 1 => Some(points.as_slice()),
                _ => None,
            })
        });
        for _ in 0..count {
            particles.push(spawn(particle, path, rng, context));
        }
    }
    if dt > 0. {
        // `age >= ttl` unlinks the node (0x71a247).
        particles.retain_mut(|state| {
            state.age += dt;
            state.previous = state.position;
            if particle.ttl > 0. && state.age >= particle.ttl {
                return false;
            }
            integrate(state, particle, affectors, ramps, dt);
            true
        });
    }
    stats.particles += particles.len();

    let tint = context.rgba();
    if tint[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let billboard = !particle.is_3d;
    let uv = flipbook_uv(context.common, context.clock);
    for state in particles.iter() {
        let color = [
            tint[0] * ((state.color >> 16) & 0xff) as f32 / 255.,
            tint[1] * ((state.color >> 8) & 0xff) as f32 / 255.,
            tint[2] * (state.color & 0xff) as f32 / 255.,
            tint[3] * ((state.color >> 24) & 0xff) as f32 / 255.,
        ];
        if color[3] < ALPHA_CUTOFF {
            continue;
        }
        let scale = if state.scale.is_finite() && state.scale > 0. {
            state.scale
        } else {
            1.
        };
        let (right, up) = if billboard {
            (context.view.right, context.view.up)
        } else {
            particle_basis(state.direction, context.view)
        };
        let positions = quad_corners(
            context,
            QuadBasis::Billboard {
                right,
                up,
                forward: context.view.forward,
            },
            state.position,
            particle.width,
            particle.height,
            particle.origin,
            particle.no_scale,
            state.rotation,
            scale,
        );
        let particle_uv = if particle.use_particle_uv {
            travel_uv(state, particle)
        } else {
            uv
        };
        push_quad(out, context, &texture, positions, particle_uv, color);
    }
}

/// Oriented-particle basis: the particle's own direction is its up axis and the
/// camera supplies the other two, so the quad still reads from any angle.
fn particle_basis(direction: Vec3, view: &View) -> (Vec3, Vec3) {
    let up = if direction.length_squared() > 1e-9 {
        direction.normalize()
    } else {
        view.up
    };
    let right = view.forward.cross(up);
    let right = if right.length_squared() > 1e-9 {
        right.normalize()
    } else {
        view.right
    };
    (right, up)
}

/// `IsUseParUV` stretches the texture along the particle's own travel instead of
/// using the flipbook cell.
fn travel_uv(state: &ParticleState, particle: &Particle) -> [[f32; 2]; 4] {
    let travel = if particle.ttl > 0. {
        (state.age / particle.ttl).clamp(0., 1.)
    } else {
        0.
    };
    [[0., travel], [1., travel], [0., 1.], [1., 1.]]
}

/// Create one particle. Every randomized value is the container's own range
/// (`ScaleMin..ScaleMax`, `RotMin..RotMax`, `ColorMin..ColorMax`), drawn through
/// the same emitter vtable slots the client calls from its emit routine
/// `0x715b50`: slot 2 the TTL, slot 3 the colour, slot 5 the scale, slot 6 the
/// rotation and slot 8 the placement.
fn spawn(
    particle: &Particle,
    path: Option<&[Vec3]>,
    rng: &mut Rng,
    context: &ElementContext,
) -> ParticleState {
    // `IsSurface` selects a per-class sampler, and the classes do not agree on
    // what a surface is.
    //
    // Id 123's `0x727f00` spends two draws on a direction `D` uniform over the
    // sphere, writes `D.x*basis0 + D.y*basis1 + D.z*basis2` to node+0x10 (the
    // three `0x401ca0` constructions at `0x7280ff`, `0x728132` and `0x728165`,
    // summed at `0x7281d5` and `0x728202`) and the negated `D` to node+0x44. Its
    // birth flow calls the TTL slot next (`0x727ed8`) and never the direction
    // slot, so the emission cone does not apply to a surface particle of that
    // class.
    //
    // Id 124's `0x729be0` runs the same construction about a different axis and
    // takes its third coefficient from somewhere else entirely. It rotates
    // `0x9c0358` about `0x9c0338` by the first draw and uses only that result's x
    // and y; the third is a second draw scaled by `2/32767` (`0x8e6efc`) and
    // reduced by `1.0` (`0x8d5c28`), a uniform in `[-1, 1]`. Since the bound basis
    // pairs `0x9c0338` with `AreaSize.z`, the sample is the lateral surface of the
    // cylinder the extents describe, not an ellipsoid. It writes no direction at
    // all, and its birth flow calls the direction slot after both samplers
    // (`0x729bc1`), so there the cone still applies.
    //
    // Ids 120 and 121 keep the volume path: id 121's `0x727800` spends ten draws
    // and calls none of the vector helpers, so it is a different algorithm
    // outright, and id 120's block at `0x7272aa` runs the same chain but adds a
    // call to `0x70a320` and uses `pi/2 - Angle`. Ids 122 and 125 never read the
    // flag.
    // The basis builder's scalar argument: the interpolated keypoint `Scale`
    // times the root `PlaySpeed`. The bound branch ignores it, so it only reaches
    // the geometry of an unbound emitter, and only for the three classes that
    // hold `0x729670` in vtable slot [9] — ids 120 and 122 hold `0x56fbb0` and
    // never build a basis, and id 125 holds `0x727220`, which writes a scalar at
    // `+0xc0` rather than the nine-float matrix both samplers read.
    let basis_scale = if !particle.is_bind && matches!(context.id, 121 | 123 | 124) {
        context.emission_scale
    } else {
        1.
    };
    let half = particle.area_size.unwrap_or(Vec3::ZERO) * (0.5 * basis_scale);
    let (offset, inward) = if particle.is_surface {
        match context.id {
            123 => {
                let sample = surface_direction(rng);
                (sample * half, Some(-sample))
            }
            124 => (cylinder_sample(rng) * half, None),
            _ => (volume_offset(particle, rng, basis_scale), None),
        }
    } else {
        (volume_offset(particle, rng, basis_scale), None)
    };
    let position = match path {
        Some(points) => sample_path(points, rng.unit() * path_length(points)).0 + offset,
        None => offset,
    };
    let direction = match inward {
        Some(direction) => direction,
        None => {
            let axis = emission_axis(particle, context);
            if particle.angle != 0. {
                cone_direction(axis, particle.angle, rng)
            } else {
                axis
            }
        }
    };
    let scale = rng.range(particle.scale_min, particle.scale_max);
    let rotation = rng.range(particle.rot_min, particle.rot_max);
    ParticleState {
        age: 0.,
        ttl: particle.ttl,
        rotation,
        color: random_color(particle, rng),
        position,
        previous: position,
        direction,
        displacement: Vec3::ZERO,
        acc_speed: 0.,
        speed: particle.speed,
        scale,
        color_remainder: [0.; 4],
        seed: rng.next_u32(),
    }
}

/// The emission axis: `ParIniDir` when the author wrote one, otherwise the
/// element's own up axis after the keypoint orientation.
///
/// The client does not read `ParIniDir` as a direction. Its parser at
/// `0x7242e2` hands the three floats to the Euler-to-quaternion routine
/// `0x7b6f10` — which halves each argument by the 0.5 at `0x8eff98` and takes
/// sines and cosines of them, the `sin(a/2)cos(b/2)...` expansion of three
/// successive rotations — then combines the result with the quaternion already
/// at emitter+0x74 and stores four floats at +0xa0. The corpus agrees: the only
/// non-zero `ParIniDir` values are exact degree conversions (`0.017453` = 1
/// degree, `1.570796` = 90), which a unit direction would never be. About a
/// hundred of the 13,665 particle elements author one, so the vector reading
/// below is wrong for those and right for the rest; reproducing the quaternion
/// needs the provenance of emitter+0x74, which is not in the parse path. Both
/// halves of that are recorded in `Findings/`.
fn emission_axis(particle: &Particle, context: &ElementContext) -> Vec3 {
    if particle.initial_direction.length_squared() > 1e-9 {
        let axis = context
            .placement
            .transform_vector3(particle.initial_direction);
        return axis.normalize_or(Vec3::Y);
    }
    let axis = context.placement.transform_vector3(Vec3::Y);
    if axis.length_squared() > 1e-9 {
        axis.normalize()
    } else {
        context.view.up
    }
}

/// Sample an emission direction inside the authored cone. The client's sampler
/// `0x723440` returns the emission basis unchanged while `Angle` (emitter+0x10)
/// compares equal to the 0.0 at `0x8d58a0`, and otherwise draws two independent
/// uniforms: `alpha = rand() * (1/32767) * Angle` for the polar angle and
/// `beta = rand() * (2*pi/32767)` for the azimuth, each fed to the axis-angle
/// quaternion helper `0x7b6fd0` and applied to the basis in turn.
///
/// So the polar angle is uniform in `[0, Angle]`, *not* uniform in solid angle
/// (which would need `cos(alpha)` uniform), and `Angle` is never clamped. Of the
/// 13,665 particle elements in the corpus 416 author a 30 degree cap, 248 author
/// `3.141593` and 2,130 author `6.283185` — a half turn sweeps the polar angle
/// from the axis to its opposite, so both of those are a whole sphere, the full
/// turn folding back onto the same directions — and 36 author a negative angle,
/// a cap mirrored through the emitter. Clamping to `[0, pi]` would leave the
/// sphere cases distributed by solid angle rather than by angle, and the
/// `angle > 0` gate that went with it collapsed the mirrored ones onto a single
/// ray along the axis.
fn cone_direction(axis: Vec3, angle: f32, rng: &mut Rng) -> Vec3 {
    let axis = if axis.length_squared() > 1e-9 {
        axis.normalize()
    } else {
        Vec3::Y
    };
    // Draw order matches `0x723440`: the polar draw comes first.
    let polar = rng.unit() * angle;
    let azimuth = rng.unit() * std::f32::consts::TAU;
    let (sin_polar, cos_polar) = polar.sin_cos();
    let local = Vec3::new(
        sin_polar * azimuth.cos(),
        sin_polar * azimuth.sin(),
        cos_polar,
    );
    // `from_rotation_arc` maps the cone's +Z onto the emission axis; the azimuth
    // draw then spins the sample uniformly around it, which is what the second
    // native quaternion rotation does.
    if (axis - Vec3::Z).length_squared() < 1e-9 {
        local
    } else {
        Quat::from_rotation_arc(Vec3::Z, axis) * local
    }
}

/// The client's surface direction: two draws, uniform over the sphere.
///
/// Id 123's surface sampler `0x727f00` never consults the emission cone. It
/// scales a `rand()` draw by `2π/32767` (`0x8e6f14`) and builds an axis-angle
/// quaternion about the global axis at `0x9c0348` through `0x7b6fd0`, which
/// writes `axis * sin(a/2)` to `+0..0xc` and `cos(a/2)` to `+0xc`. The three
/// calls that follow evaluate `u + 2w(v x u) + 2(v x (v x u))` — `0x4093d0` is
/// the cross product, `0x633700` scales a vector by a scalar and `0x4071f0` adds
/// two — which is the rotation of the global axis at `0x9c0358` by that
/// quaternion. A second draw rotates `0x9c0348` about the first result
/// (`0x728004`, whose axis argument is the first rotation's own output), which
/// sweeps the sphere uniformly.
///
/// The bound basis builder `0x729670` pairs `0x9c0358` with `AreaSize.x`,
/// `0x9c0348` with `AreaSize.y` and `0x9c0338` with `AreaSize.z`, which is what
/// identifies the two globals as the x and y axes.
fn surface_direction(rng: &mut Rng) -> Vec3 {
    let tau = std::f32::consts::TAU;
    let first = Quat::from_axis_angle(Vec3::Y, rng.unit() * tau) * Vec3::X;
    Quat::from_axis_angle(first, rng.unit() * tau) * Vec3::Y
}

/// Id 124's surface sample: a full turn about the basis's z axis with an
/// independent uniform height, so the coefficients are `(cos t, sin t, u)`.
///
/// `0x729be0` builds the turn the way id 123 builds its direction — a quaternion
/// from `0x7b6fd0`, then the inline rotation `u + 2w(v x u) + 2(v x (v x u))`
/// through `0x4093d0`, `0x633700` and `0x4071f0` — but about `0x9c0338`, and it
/// never reads the rotated vector's third component. That coefficient is a second
/// `rand()` draw scaled by `2/32767` (`0x8e6efc`) and reduced by `1.0`
/// (`0x8d5c28`), which is `Rng::signed`.
fn cylinder_sample(rng: &mut Rng) -> Vec3 {
    let tau = std::f32::consts::TAU;
    let turned = Quat::from_axis_angle(Vec3::Z, rng.unit() * tau) * Vec3::X;
    turned.with_z(rng.signed())
}

/// The volume offset every class shares when `IsSurface` does not apply.
///
/// The client's shape routine `0x728270` multiplies a uniform unit-ball sample by
/// the matrix at emitter+0xc0 and then adds the emitter position. `0x729670` fills
/// that matrix and branches on `IsBind` at emitter+0x85 (`0x72967e`).
///
/// Bound emitters get `diag(AreaSize * 0.5)` laid against the three global unit
/// axes `0x9c0358`, `0x9c0348` and `0x9c0338` — `0x729684` reads `AreaSize.x`,
/// multiplies it by the 0.5 at `0x8d5894` and never touches the scalar argument —
/// which is what this reproduces unchanged.
///
/// Unbound emitters take `0x729763`, which reads its scalar argument three times
/// (the `push`/`fstps (%esp)` idiom shifts the frame, so `0x74(%esp)`,
/// `0x7c(%esp)` and `0x80(%esp)` are all `[entry+4]`) into `V = scalar *
/// AreaSize`, then writes row `i` as `rotate(axis_i, q) * (V_i * 0.5)`: rows 0 and
/// 1 spell the rotation out as `axis + 2w(v x axis) + 2(v x (v x axis))` with `v`
/// the vector part of `q` and `2w` from `0x7297f4`, and row 2 reaches the same
/// result through `0x634420` and `0x401ce0`. Both of its callers pass `keypoint
/// Scale * root PlaySpeed` (`0x71a35b`, `0x71a501`), and `q` at emitter+0x74 is
/// the Hamilton product `Q * keypoint Direction` computed at
/// `0x71a406`–`0x71a493`, where `Q` is what `0x6fa810` returns: root+0x1b4 when
/// root+0x1f4 and element+0x131 are set and either element+0x132 or root+0x284
/// is, otherwise root+0x1a4.
///
/// The rotation half of that this already does, one stage later: `placement`
/// turns every particle position by the interpolated keypoint direction, and the
/// client's unbound particles are world space — its samplers add emitter+0x68,
/// the element's world position from `0x7872d0`, which a later element transform
/// would double count. The two compositions therefore agree wherever the parent
/// basis carries `Q`. The scalar was the missing half, and it is a real one:
/// 4,761 unbound elements of ids 121, 123 and 124 author an `AreaSize`, 1,605 of
/// their 5,342 keypoint scales are not 1.0 (the largest is 10, none is negative)
/// and 1,430 of the 5,459 roots author a `PlaySpeed` other than 1.
fn volume_offset(particle: &Particle, rng: &mut Rng, basis_scale: f32) -> Vec3 {
    match (particle.radius, particle.area_size) {
        (Some(radius), _) => random_unit_ball(rng) * radius.max(0.),
        (None, Some(area)) => random_unit_ball(rng) * area * (0.5 * basis_scale),
        (None, None) => Vec3::ZERO,
    }
}

/// A uniform sample of the unit ball. `0x728270` rejection-samples three
/// `rand() * (2/32767) - 1` draws and retries while `x*x + y*y + z*z > 1`; a
/// uniform direction scaled by the cube root of a uniform radius is the same
/// distribution, without burning an unbounded number of draws.
fn random_unit_ball(rng: &mut Rng) -> Vec3 {
    random_unit(rng) * rng.unit().cbrt()
}

fn random_unit(rng: &mut Rng) -> Vec3 {
    let z = rng.signed();
    let radius = (1. - z * z).max(0.).sqrt();
    let theta = rng.unit() * std::f32::consts::TAU;
    Vec3::new(radius * theta.cos(), radius * theta.sin(), z)
}

/// Spawn colour, drawn the way the client's colour slot `0x7237f0` draws it.
///
/// Equal endpoints short-circuit at `0x7237fc` and come back untouched.
/// Otherwise `IsUseHSVInterp` (emitter+0xb0, tested at `0x723804`) picks between
/// two samplers that are not variations on a theme: the RGB path draws four
/// independent random values and the HSV path draws one and shares it, so the
/// HSV path's channels move together. 168 retail elements ask for the HSV one.
///
/// One native detail is left out on purpose. The RGB path multiplies its alpha
/// by emitter+0x8c before truncating it (`0x723955`), and that factor is the
/// element's live alpha, which this player already applies once while building
/// the quad; applying it here as well would square it. The HSV path has no such
/// factor — its alpha is simply the fourth lerped channel. Recorded in
/// `Findings/`.
fn random_color(particle: &Particle, rng: &mut Rng) -> u32 {
    if particle.color_min == particle.color_max {
        return particle.color_min;
    }
    if particle.use_hsv_interpolation {
        random_color_hsv(particle.color_min, particle.color_max, rng)
    } else {
        random_color_rgb(particle.color_min, particle.color_max, rng)
    }
}

/// The RGB sampler: four independent `rand()` values, one per channel, each
/// combined as `min * (1 - u) + max * u` and passed through `_ftol` (thunk
/// `0x892e78`, `MSVCRT._ftol`), which truncates toward zero, so a sample lands
/// up to one unit below what rounding would give it. The client draws in the
/// order red, green, blue, alpha (`0x7238cc` to `0x723908`); with draws this
/// independent the order is not observable, so this walks the shifts ascending.
fn random_color_rgb(min: u32, max: u32, rng: &mut Rng) -> u32 {
    let mut packed = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let low = ((min >> shift) & 0xff) as f32;
        let high = ((max >> shift) & 0xff) as f32;
        let u = rng.unit();
        packed |= ((low * (1. - u) + high * u) as u32) << shift;
    }
    packed
}

/// The HSV sampler. `0x723812` draws **one** `rand()` and uses it for all four
/// channels, and it lerps the other way round from the RGB path —
/// `min * u + max * (1 - u)` — which for a uniform draw is the same
/// distribution, so only the coupling of the channels differs.
///
/// Hue is a circle and the client knows it: `0x723829` compares the two hues and
/// takes the branch at `0x72383b` when the minimum is the larger one, adding a
/// full turn to the maximum and subtracting it again from any result above one
/// (`0x72385e`). An author writing 0.9 down to 0.1 gets the short way round
/// through red rather than the long way back through the spectrum.
fn random_color_hsv(min: u32, max: u32, rng: &mut Rng) -> u32 {
    let low = rgb_to_hsva(min);
    let high = rgb_to_hsva(max);
    let u = f64::from(rng.unit());
    let inverse = 1. - u;
    // The client stores all four results as singles (`0x723876`, `0x723890`,
    // `0x7238a0`, `0x7238ae`) before handing them to the converter, so each is
    // rounded to f32 here at the same point.
    let channel =
        |index: usize| (f64::from(low[index]) * u + f64::from(high[index]) * inverse) as f32;
    let hue = if low[0] <= high[0] {
        f64::from(low[0]) * u + f64::from(high[0]) * inverse
    } else {
        let wrapped = f64::from(low[0]) * u + (f64::from(high[0]) + 1.) * inverse;
        if wrapped > 1. {
            wrapped - 1.
        } else {
            wrapped
        }
    };
    hsva_to_packed([hue as f32, channel(1), channel(2), channel(3)])
}

/// The client's RGB→HSV, `0x789ad0`. The element parser calls it once per
/// emitter — `0x724434` for `ColorMin` into emitter+0x38 and `0x724440` for
/// `ColorMax` into emitter+0x48 — and it does so unconditionally: the
/// `IsUseHSVInterp` byte only decides which sampler reads the copies.
///
/// Each byte is scaled by the f32 one-over-255 at `0x8f0928`, the alpha rides
/// along as that same float (`0x789be8` copies its bits), and the hue comes out
/// normalised: the three-way maximum test at `0x789c33`/`0x789c52`/`0x789c77`
/// offsets the sector by 0, 2 or 4, `0x789c98` scales to degrees, `0x789cab`
/// adds a full turn to a negative result and `0x789cb1` divides by 360.
fn rgb_to_hsva(packed: u32) -> [f32; 4] {
    const BYTE: f32 = 1. / 255.;
    let red = ((packed >> 16) & 0xff) as f32 * BYTE;
    let green = ((packed >> 8) & 0xff) as f32 * BYTE;
    let blue = (packed & 0xff) as f32 * BYTE;
    let alpha = (packed >> 24) as f32 * BYTE;
    let highest = red.max(green).max(blue);
    let lowest = red.min(green).min(blue);
    let delta = highest - lowest;
    // `0x789bf6` tests the value channel against zero before dividing, so black
    // stores a zero saturation instead of a quotient; `0x789c29` then stores a
    // literal zero hue for any achromatic colour.
    let saturation = if highest == 0. {
        0.
    } else {
        (f64::from(delta) / f64::from(highest)) as f32
    };
    let hue = if saturation == 0. {
        0.
    } else {
        // The x87 keeps its intermediates wider than f32 and only rounds when it
        // stores, so the hue chain is walked in f64 and rounded once at the end.
        let delta = f64::from(delta);
        let sector = if red == highest {
            (f64::from(green) - f64::from(blue)) / delta
        } else if green == highest {
            (f64::from(blue) - f64::from(red)) / delta + 2.
        } else {
            (f64::from(red) - f64::from(green)) / delta + 4.
        };
        let degrees = sector * 60.;
        let wrapped = if degrees < 0. {
            degrees + 360.
        } else {
            degrees
        };
        (wrapped * (1. / 360.)) as f32
    };
    [hue, saturation, highest, alpha]
}

/// The client's HSV→RGB, `0x7898f0`: the six-sector hexcone of the switch at
/// `0x7899d1` (jump table `0x789ab8`), with three details worth keeping.
///
/// The value channel is scaled by 255 and then biased by the 0.5 at `0x8eff98`
/// before `_ftol` truncates it, so value is rounded to nearest, while the alpha
/// at `0x789919` is only scaled and so is truncated. A zero saturation
/// short-circuits to grey at `0x789933`, and a hue of exactly 1.0 is folded to
/// 0.0 at `0x789956` so the top of the circle lands in sector 0 instead of off
/// the end of the switch.
fn hsva_to_packed(hsva: [f32; 4]) -> u32 {
    let [hue, saturation, value, alpha] = hsva;
    let v = f64::from(value) * 255. + 0.5;
    let a = (f64::from(alpha) * 255.) as u32;
    let (red, green, blue) = if saturation == 0. {
        let grey = v as u32;
        (grey, grey, grey)
    } else {
        let folded = if hue == 1. { 0. } else { hue };
        let scaled = f64::from(folded) * 6.;
        // `floor` through the IAT at `0x8d54b4`, then `_ftol` at `0x789975`.
        let index = scaled.floor();
        let fraction = scaled - index;
        let s = f64::from(saturation);
        let p = (1. - s) * v;
        let q = (1. - s * fraction) * v;
        let t = (1. - s * (1. - fraction)) * v;
        // `cmp $5,%eax; ja 0x789a8f` sends a seventh sector to a path that reads
        // the value slot as an integer. Hue is normalised into [0, 1] so the
        // branch is unreachable; the sixth case is what it would have to be.
        let channels = match index as i64 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        (channels.0 as u32, channels.1 as u32, channels.2 as u32)
    };
    (a << 24) | (red << 16) | (green << 8) | blue
}

/// One integration step: every affector through the client's temp-struct
/// mapping, then the accelerations exactly as `0x7165b0` writes them.
fn integrate(
    state: &mut ParticleState,
    particle: &Particle,
    affectors: &[Control],
    ramps: &[Option<ColorTrack>],
    dt: f32,
) {
    let age_ms = state.age * 1000.;
    for (slot, control) in affectors.iter().enumerate() {
        if age_ms < control.start {
            continue;
        }
        if control.end >= 0. && age_ms > control.end {
            continue;
        }
        let local = (age_ms - control.start) / 1000.;
        match &control.data {
            ControlData::Motion { dir, vel, acc } => {
                let dspl = displacement(*vel, *acc, local, dt);
                state.position += *dir * dspl;
                state.displacement += *dir * dspl;
            }
            ControlData::Speed { vel, acc } => {
                state.rotation += displacement(*vel, *acc, local, dt);
            }
            ControlData::Orbit {
                pos,
                axis,
                vel,
                acc,
            } => {
                let dspl = displacement(*vel, *acc, local, dt);
                let axis = if axis.length_squared() > 1e-12 {
                    axis.normalize()
                } else {
                    Vec3::Y
                };
                let rotation = Quat::from_axis_angle(axis, dspl);
                state.position = *pos + rotation * (state.position - *pos);
            }
            ControlData::Attract { pos, vel, acc } => {
                if *vel == -1. {
                    continue;
                }
                let dspl = displacement(*vel, *acc, local, dt);
                let target = *pos + state.displacement;
                if dspl == -1. {
                    state.position = target;
                    continue;
                }
                let delta = state.position - target;
                let length = delta.length();
                if length > 1e-6 {
                    let step = delta / length * dspl;
                    state.position += step;
                    state.displacement += step;
                }
            }
            ControlData::ColorDelta { delta } => {
                let mut packed = state.color;
                for (index, shift) in COLOR_SHIFTS.into_iter().enumerate() {
                    let shown = ((packed >> shift) & 0xff) as u8;
                    let updated =
                        color_delta(shown, delta[index], dt, &mut state.color_remainder[index]);
                    packed = (packed & !(0xff << shift)) | ((updated as u32) << shift);
                }
                state.color = packed;
            }
            ControlData::ScaleChange { rate, min, max } => {
                state.scale = (state.scale + rate * dt).clamp(*min, *max);
            }
            ControlData::Fractal {
                amplitude,
                wave_length,
                persistence,
                octaves,
                buffer_length,
                ..
            } => {
                let sample = fbm(
                    state.seed,
                    state.age,
                    *wave_length,
                    *persistence,
                    (*octaves).max(1) as usize,
                    *buffer_length,
                );
                state.position += Vec3::new(sample, sample * 0.5, sample * 0.25) * *amplitude;
            }
            ControlData::ColorRamp { .. } => {
                if let Some(Some(track)) = ramps.get(slot) {
                    if let Some(color) = track.sample(state.age - control.start / 1000.) {
                        state.color = color;
                    }
                }
            }
            ControlData::ScaleRamp { first, stops } => {
                state.scale = sample_ramp(*first, stops, state.age - control.start / 1000.);
            }
            ControlData::Path { points, .. } => {
                if points.len() > 1 {
                    let total = path_length(points);
                    if total > 0. {
                        let distance = if state.ttl > 0. {
                            (state.age / state.ttl).clamp(0., 1.) * total
                        } else {
                            total
                        };
                        state.position = sample_path(points, distance).0;
                    }
                }
            }
        }
    }
    // Acceleration pass (0x7165b0), gated in the client on the element's own
    // integration flag at +0x23c: trapezoidal integration of `ParAcc` along the
    // particle direction and of `Acc` along the authored `AccDir`.
    let half = dt * 0.5;
    if particle.particle_acc != 0. {
        let before = state.speed;
        state.speed += particle.particle_acc * dt;
        let step = (before + state.speed) * half;
        state.position += state.direction * step;
        state.displacement += state.direction * step;
    } else if state.speed != 0. {
        // With no acceleration the particle still travels at its spawn speed.
        let step = state.speed * dt;
        state.position += state.direction * step;
        state.displacement += state.direction * step;
    }
    if particle.acc != 0. {
        let before = state.acc_speed;
        state.acc_speed += particle.acc * dt;
        let step = (before + state.acc_speed) * half;
        let direction = if particle.acc_dir.length_squared() > 1e-9 {
            particle.acc_dir.normalize()
        } else {
            Vec3::ZERO
        };
        state.position += direction * step;
        state.displacement += direction * step;
    }
    if particle.is_drag && particle.drag_power != 0. {
        state.speed *= (-particle.drag_power.abs() * dt).exp();
    }
}

/// Elements 110 and 180: a strip through the element's own motion history. Each
/// interval the current placement contributes one cross-section — the two rails
/// are the authored `OrgPos1`/`OrgPos2` (ribbon) or the two `Pos` values (wave) —
/// and each cross-section ages out after `SegLife` milliseconds. Connecting the
/// survivors is the triangle strip the client builds.
fn emit_ribbon(
    ribbon: &mut RibbonState,
    element: &Element,
    context: &ElementContext,
    dt: f32,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) {
    stats.ribbons += 1;
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let (life, rails) = match &element.spec {
        Spec::Ribbon(spec) => (spec.segment_life, (spec.origin_1, spec.origin_2)),
        Spec::Wave(wave) => (
            wave.segment_life,
            (
                wave.positions.first().copied().unwrap_or(Vec3::ZERO),
                wave.positions.get(1).copied().unwrap_or(Vec3::ZERO),
            ),
        ),
        _ => return,
    };
    if !life.is_finite() || life <= 0. {
        return;
    }
    // Neither class authors an interval of its own; the client emits one
    // cross-section per frame, so this uses the 30 Hz authoring clock the rest
    // of the container's intervals are written against.
    let interval = 1000. / 30.;
    let dt_ms = dt * 1000.;
    ribbon.carry += dt_ms;
    let mut emitted = 0;
    while ribbon.carry >= interval && emitted < MAX_SEGMENTS {
        ribbon.carry -= interval;
        emitted += 1;
        ribbon.vertices.push(RibbonVertex {
            position: context.placement.transform_point3(rails.0),
            age: 0.,
        });
        ribbon.vertices.push(RibbonVertex {
            position: context.placement.transform_point3(rails.1),
            age: 0.,
        });
        if ribbon.vertices.len() > MAX_SEGMENTS * 2 {
            let excess = ribbon.vertices.len() - MAX_SEGMENTS * 2;
            ribbon.vertices.drain(0..excess);
        }
    }
    for vertex in &mut ribbon.vertices {
        vertex.age += dt_ms;
    }
    ribbon.vertices.retain(|vertex| vertex.age < life);
    if ribbon.vertices.len() < 4 {
        return;
    }
    let uv = flipbook_uv(context.common, context.clock);
    let mut positions = [[0f32; 3]; 4];
    // Vertices are pushed in pairs, so the strip runs along the pairs.
    for pair in 0..ribbon.vertices.len() / 2 - 1 {
        let a0 = &ribbon.vertices[pair * 2];
        let a1 = &ribbon.vertices[pair * 2 + 1];
        let b0 = &ribbon.vertices[pair * 2 + 2];
        let b1 = &ribbon.vertices[pair * 2 + 3];
        let fade_a = 1. - (a0.age / life).clamp(0., 1.);
        let fade_b = 1. - (b0.age / life).clamp(0., 1.);
        let alpha = color[3] * fade_a.max(fade_b);
        if alpha < ALPHA_CUTOFF {
            continue;
        }
        for (slot, point) in [a0.position, a1.position, b1.position, b0.position]
            .into_iter()
            .enumerate()
        {
            positions[slot] = point.to_array();
        }
        let u = (a0.age / life).clamp(0., 1.);
        let v = (b0.age / life).clamp(0., 1.);
        let strip = [[uv[0][0], u], [uv[1][0], u], [uv[2][0], v], [uv[3][0], v]];
        push_quad(
            out,
            context,
            &texture,
            positions,
            strip,
            [color[0], color[1], color[2], alpha],
        );
    }
}

/// Elements 150 and 152: the authored fractal strip. `Segs` cross-sections run
/// from `StartPos` to `EndPos`; the noise block (`BufLen`, `Amplitude`,
/// `WaveLen`, `Persistence`, `OctaveNum`) displaces them and is regenerated every
/// `Interval` milliseconds, which is the flicker these two classes are authored
/// for. The client writes `Width` and `Alpha` three times each — head, tail and
/// segment — so the strip interpolates head to tail along its length.
fn emit_fractal(
    fractal: &mut FractalState,
    element: &Element,
    context: &ElementContext,
    dt: f32,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) {
    stats.ribbons += 1;
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let (spec, trail) = match &element.spec {
        Spec::Fractal(spec) => (spec, None),
        Spec::Trail(spec) => (&spec.fractal, Some(spec)),
        _ => return,
    };
    let segments = spec.segments.max(2) as usize;
    let interval = spec.interval.max(1) as f32;
    fractal.timer += dt * 1000.;
    if fractal.timer >= interval || fractal.offsets.len() != segments + 1 {
        fractal.timer = 0.;
        fractal.offsets.clear();
        for index in 0..=segments {
            let t = index as f32 / segments as f32;
            let sample = fbm(
                fractal.rng.next_u32(),
                t * spec.wave_length.max(1e-3),
                spec.noise_wave_length.max(1e-3),
                spec.persistence,
                spec.octaves.max(1) as usize,
                spec.buffer_length,
            );
            fractal.offsets.push(
                Vec3::new(sample, sample * 0.5, sample * 0.25)
                    * spec.noise_amplitude
                    * context.scale,
            );
        }
    }
    let head_width = spec.widths.first().copied().unwrap_or(0.1);
    let tail_width = spec.widths.get(1).copied().unwrap_or(head_width);
    let head_alpha = spec.alphas.first().copied().unwrap_or(1.);
    let tail_alpha = spec.alphas.get(1).copied().unwrap_or(head_alpha);
    let matrix = context.placement;
    let start = matrix.transform_point3(spec.start_position);
    let end = matrix.transform_point3(spec.end_position);
    let axis = end - start;
    if axis.length_squared() < 1e-12 {
        return;
    }
    // The strip faces the camera: its width axis is the cross of the strip axis
    // and the view direction. `UseNormal`/`Normal` author a fixed width axis
    // instead, and `Filter` selects the smoothing the client applies to it.
    let normal = if spec.use_normal && spec.normal.length_squared() > 1e-9 {
        matrix.transform_vector3(spec.normal).normalize_or_zero()
    } else {
        let view = (context.view.position - start).normalize_or_zero();
        let cross = axis.cross(view);
        if cross.length_squared() > 1e-12 {
            cross.normalize()
        } else {
            context.view.right
        }
    };
    let uv = flipbook_uv(context.common, context.clock);
    let mut positions = [[0f32; 3]; 4];
    for index in 0..segments {
        let t0 = index as f32 / segments as f32;
        let t1 = (index + 1) as f32 / segments as f32;
        let a = start.lerp(end, t0) + fractal.offsets[index];
        let b = start.lerp(end, t1) + fractal.offsets[index + 1];
        let width0 = (head_width + (tail_width - head_width) * t0).max(0.) * context.scale * 0.5;
        let width1 = (head_width + (tail_width - head_width) * t1).max(0.) * context.scale * 0.5;
        let alpha0 = head_alpha + (tail_alpha - head_alpha) * t0;
        let mut alpha1 = head_alpha + (tail_alpha - head_alpha) * t1;
        // A trail's tail disappears over its authored fade; a fractal ribbon
        // keeps the alpha ramp exactly as written.
        if let Some(trail) = trail {
            if trail.is_tail_disappear {
                let fade = (1. - trail.tail_fade_out).clamp(0., 1.);
                alpha1 *= fade;
            }
        }
        let alpha = color[3] * ((alpha0 + alpha1) * 0.5).clamp(0., 1.);
        if alpha < ALPHA_CUTOFF {
            continue;
        }
        let corners = [
            a - normal * width0,
            a + normal * width0,
            b + normal * width1,
            b - normal * width1,
        ];
        for (slot, corner) in corners.into_iter().enumerate() {
            positions[slot] = corner.to_array();
        }
        let strip = [
            [uv[0][0] + t0, uv[0][1]],
            [uv[1][0] + t0, uv[1][1]],
            [uv[2][0] + t1, uv[2][1]],
            [uv[3][0] + t1, uv[3][1]],
        ];
        push_quad(
            out,
            context,
            &texture,
            positions,
            strip,
            [color[0], color[1], color[2], alpha],
        );
    }
}

/// Lightning: rebuild the bolt every `Interval` milliseconds, stepping from the
/// element origin with the authored deviation, width and alpha ramps, branching
/// up to `MaxBranches` times.
fn emit_bolts(
    bolts: &mut BoltState,
    bolt: &Lightning,
    context: &ElementContext,
    dt: f32,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
) {
    stats.ribbons += 1;
    let dt_ms = dt * 1000.;
    bolts.timer += dt_ms;
    let interval = bolt.interval.max(1) as f32;
    if bolts.timer >= interval {
        bolts.timer = 0.;
        bolts.segments.clear();
        let direction = context
            .placement
            .transform_vector3(Vec3::Y)
            .normalize_or_zero();
        let origin = context.placement.transform_point3(Vec3::ZERO);
        for _ in 0..bolt.bolts_per_interval.max(1) {
            grow_bolt(
                bolts,
                bolt,
                origin,
                direction,
                bolt.max_steps.max(1),
                bolt.max_branches,
                0,
            );
        }
    }
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF || bolts.segments.is_empty() {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let right = context.view.right;
    let mut positions = [[0f32; 3]; 4];
    let uv = flipbook_uv(context.common, context.clock);
    for (start, end, width, alpha) in &bolts.segments {
        let alpha = color[3] * alpha;
        if alpha < ALPHA_CUTOFF {
            continue;
        }
        let half = width * 0.5 * context.scale;
        let a0 = *start - right * half;
        let a1 = *start + right * half;
        let b0 = *end - right * half;
        let b1 = *end + right * half;
        for (slot, point) in [a0, a1, b1, b0].into_iter().enumerate() {
            positions[slot] = point.to_array();
        }
        push_quad(
            out,
            context,
            &texture,
            positions,
            uv,
            [color[0], color[1], color[2], alpha],
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn grow_bolt(
    bolts: &mut BoltState,
    bolt: &Lightning,
    mut position: Vec3,
    mut direction: Vec3,
    steps: u32,
    branches: u32,
    depth: u32,
) {
    if bolts.segments.len() >= MAX_SEGMENTS {
        return;
    }
    for step in 0..steps {
        let blend = if steps > 1 {
            step as f32 / (steps - 1) as f32
        } else {
            1.
        };
        let length = bolts.rng.range(bolt.step_min, bolt.step_max);
        // `Deviation` is an angular jitter in radians applied about two axes,
        // which is what makes retail strokes read as hand drawn.
        let yaw = Quat::from_axis_angle(Vec3::Y, bolts.rng.signed() * bolt.deviation);
        let pitch_axis = direction.cross(Vec3::Y);
        let pitch_axis = if pitch_axis.length_squared() > 1e-9 {
            pitch_axis.normalize()
        } else {
            Vec3::X
        };
        let pitch = Quat::from_axis_angle(pitch_axis, bolts.rng.signed() * bolt.deviation);
        direction = (pitch * (yaw * direction)).normalize_or_zero();
        let next = position + direction * length;
        let width = bolt.width_start + (bolt.width_end - bolt.width_start) * blend;
        let alpha = bolt.alpha_start + (bolt.alpha_end - bolt.alpha_start) * blend;
        bolts
            .segments
            .push((position, next, width.max(0.), alpha.clamp(0., 1.)));
        position = next;
        if bolts.segments.len() >= MAX_SEGMENTS {
            return;
        }
        if depth < bolt.circles && branches > 0 && bolts.rng.unit() < 0.35 {
            let branch =
                Quat::from_axis_angle(pitch_axis, bolts.rng.signed() * bolt.deviation * 2.)
                    * direction;
            grow_bolt(
                bolts,
                bolt,
                position,
                branch,
                (steps / 2).max(1),
                branches - 1,
                depth + 1,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gfx_decal, gfx_file};

    /// Version 37 root header, byte-identical to the `gfx_file` fixture so both
    /// readers can be pointed at the same text.
    fn header37(elements: u32) -> String {
        format!(
            "MOXTVersion: 37\r\nDedaultScale: 2\r\nPlaySpeed: 1\r\nDefaultAlpha: 0.5\r\n\
             Raytrace: 0\r\nFadeByDist: 0\r\nFadeStart: 0.000000\r\nFadeEnd: 0.000000\r\n\
             Vec: 0.100000, 0.200000, 0.300000\r\nVec: 1.000000, 2.000000, 3.000000\r\n\
             ShakeCam: 0\r\nGFXELEMENTCOUNT: {elements}\r\n"
        )
    }

    /// The element-100 record every retail sprite carries, verbatim shape from
    /// `gfx/无标题.gfx`: width 0.25, height 0.5, centred origin, one forever
    /// keypoint at (1, 2, 3) with scale 2 and full white.
    const SPRITE: &str = "GFXELEMENTID: 100\r\nName: glow\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
        RepeatCount: 1\r\nRepeatDelay: 0\r\nTexFile: 粒子\\glow.dds\r\nBindEle: \r\nZEnable: 0\r\n\
        MatchGrnd: 0\r\nTexRow: 1\r\nTexCol: 1\r\nTexInterval: 50\r\nPriority: 2\r\nIsDummy: 0\r\n\
        DummyEle: \r\nWarp: 0\r\nWidth: 0.25\r\nHeight: 0.5\r\nRotFromView: 0\r\nGrndNormOnly: 0\r\n\
        StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
        Position: 1, 2, 3\r\nColor: -1\r\nScale: 2\r\nDirection: 0, 0, 0, 1\r\nRad_2D: 0\r\n\
        CtrlMethodCount: 0\r\n";

    /// A camera looking down -Z from (0, 0, 10), so the billboard basis is the
    /// world XY plane and a sprite at the origin is 10 units away.
    fn view() -> View {
        View {
            position: Vec3::new(0., 0., 10.),
            right: Vec3::X,
            up: Vec3::Y,
            forward: Vec3::new(0., 0., -1.),
        }
    }

    fn build(source: &str) -> Player {
        let root = gfx_file::parse(source).unwrap_or_else(|error| panic!("{error}"));
        Player::new(root, 7, &mut |_| Err("this root has no children".into()))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn step(player: &mut Player, dt_ms: f32) {
        player
            .advance(dt_ms, &view(), Mat4::IDENTITY, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// The quad centre, which for a centred origin is the element position.
    fn quad_centre(quad: &EffectQuad) -> Vec3 {
        let mut total = Vec3::ZERO;
        for corner in &quad.positions {
            total += Vec3::from_array(*corner);
        }
        total / 4.
    }

    /// The horizontal extent of a centred sprite: `4 * Width * scale`.
    fn extent_x(quad: &EffectQuad) -> f32 {
        quad.positions[1][0] - quad.positions[0][0]
    }

    /// The packed alpha of the first quad as the client's 0..255 integer.
    fn alpha8(quad: &EffectQuad) -> u32 {
        (quad.color[3] * 255.).round() as u32
    }

    /// The whole point of the static reader is that it was written against the
    /// client's own decal calls, so wherever the two readers accept the same
    /// root they must produce the same frame. The corpus sweep repeats this over
    /// every retail root `gfx_decal` accepts; this fixture keeps the check inside
    /// `cargo test`.
    #[test]
    fn frame_zero_matches_the_verified_static_decal() {
        let source = header37(1) + SPRITE;
        let reference = gfx_decal::Root::parse(&source).unwrap();
        let expected = reference.quads(Mat4::IDENTITY, 1., 1.).unwrap();

        let mut player = build(&source);
        step(&mut player, 0.);
        let actual = player.quads();

        assert_eq!(expected.len(), 1);
        assert_eq!(actual.len(), expected.len());
        for (want, got) in expected.iter().zip(actual) {
            assert_eq!(want.texture, got.texture);
            assert_eq!(want.source_blend, got.source_blend);
            assert_eq!(want.destination_blend, got.destination_blend);
            assert_eq!(want.depth_test, got.depth_test);
            for slot in 0..4 {
                let want = &want.vertices[slot];
                for axis in 0..3 {
                    assert!(
                        (want.position[axis] - got.positions[slot][axis]).abs() <= 1e-5,
                        "corner {slot} axis {axis}: {} != {}",
                        want.position[axis],
                        got.positions[slot][axis]
                    );
                }
                for axis in 0..2 {
                    assert!(
                        (want.uv[axis] - got.uv[slot][axis]).abs() <= 1e-6,
                        "uv {slot} axis {axis}: {} != {}",
                        want.uv[axis],
                        got.uv[slot][axis]
                    );
                }
                for channel in 0..4 {
                    assert!(
                        (want.color[channel] - got.color[channel]).abs() <= 1e-5,
                        "color channel {channel}: {} != {}",
                        want.color[channel],
                        got.color[channel]
                    );
                }
            }
        }
    }

    /// `gfx_decal` doubles the authored dimensions and splits them at `OrgPt`,
    /// and its *top* extent is the complement of that split. 704 corpus `OrgPt`
    /// lines are off centre, so this is not a theoretical case.
    #[test]
    fn an_off_centre_origin_splits_the_doubled_dimensions() {
        let source = header37(1)
            + &SPRITE.replace(
                "GrndNormOnly: 0\r\n",
                "GrndNormOnly: 0\r\nNoScale: 0\r\nNoScale: 0\r\n\
                 OrgPt: 0.250000\r\nOrgPt: 0.750000\r\n",
            );
        let reference = gfx_decal::Root::parse(&source).unwrap();
        let expected = reference.quads(Mat4::IDENTITY, 1., 1.).unwrap();

        let mut player = build(&source);
        step(&mut player, 0.);
        let got = player.quads();
        assert_eq!(got.len(), 1);

        // w0 = 0.5*0.25 = 0.125, w1 = 0.5-0.125 = 0.375, h0 = 1*0.75 = 0.75,
        // h1 = 1-0.75 = 0.25, each times the keypoint scale 2, then the root
        // scale 2, around the position (1, 2, 3).
        let want = [[1.5, 5., 6.], [3.5, 5., 6.], [1.5, 1., 6.], [3.5, 1., 6.]];
        for slot in 0..4 {
            for axis in 0..3 {
                assert!(
                    (got[0].positions[slot][axis] - want[slot][axis]).abs() <= 1e-5,
                    "player corner {slot} axis {axis}: {} != {}",
                    got[0].positions[slot][axis],
                    want[slot][axis]
                );
                assert!(
                    (expected[0].vertices[slot].position[axis] - want[slot][axis]).abs() <= 1e-5,
                    "decal corner {slot} axis {axis}: {} != {}",
                    expected[0].vertices[slot].position[axis],
                    want[slot][axis]
                );
            }
        }
    }

    /// A sprite whose origin is off centre must still agree with the decal when
    /// the parent basis is rotated and scaled, which is the case the billboard
    /// path cannot be checked against.
    #[test]
    fn a_rotated_and_scaled_placement_carries_the_sprite() {
        let source = header37(1) + SPRITE;
        let reference = gfx_decal::Root::parse(&source).unwrap();
        let placement = Mat4::from_scale(Vec3::new(3., 3., 3.))
            * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2)
            * Mat4::from_translation(Vec3::new(10., 0., 0.));
        // The decal normalizes the parent basis, so a non-uniform parent is
        // applied as the uniform length of its axes.
        let expected = reference.quads(placement, 0.5, 2.).unwrap();

        let root = gfx_file::parse(&source).unwrap();
        let mut player = Player::new(root, 7, &mut |_| Err("no children".into())).unwrap();
        player.advance(0., &view(), placement, 0.5, 2.).unwrap();
        let got = player.quads();
        assert_eq!(got.len(), expected.len());
        for slot in 0..4 {
            for axis in 0..3 {
                assert!(
                    (expected[0].vertices[slot].position[axis] - got[0].positions[slot][axis])
                        .abs()
                        <= 1e-4,
                    "corner {slot} axis {axis}: {} != {}",
                    expected[0].vertices[slot].position[axis],
                    got[0].positions[slot][axis]
                );
            }
        }
        // Alpha folds root, placement and keypoint: 255 * 0.5 * 2 = 255.
        assert_eq!(alpha8(&got[0]), 255);
    }

    #[test]
    fn repeat_count_and_repeat_delay_gate_the_element_clock() {
        // One 100 ms keypoint, played twice with a 50 ms gap between the passes.
        let source = header37(1)
            + &SPRITE
                .replace("RepeatCount: 1\r\n", "RepeatCount: 2\r\n")
                .replace("RepeatDelay: 0\r\n", "RepeatDelay: 50\r\n")
                .replace("TimeSpan: -1\r\n", "TimeSpan: 100\r\n");
        let mut player = build(&source);

        step(&mut player, 60.);
        assert_eq!(player.quads().len(), 1, "first pass is visible");
        assert!(!player.finished());

        step(&mut player, 60.);
        assert!(
            player.quads().is_empty(),
            "120 ms falls in the repeat delay"
        );
        assert!(!player.finished());

        step(&mut player, 80.);
        assert_eq!(player.quads().len(), 1, "second pass is visible");

        step(&mut player, 100.);
        assert!(player.finished(), "both passes are over at 300 ms");
        assert!(player.quads().is_empty());

        // A negative repeat count never finishes.
        let looping = header37(1)
            + &SPRITE
                .replace("RepeatCount: 1\r\n", "RepeatCount: -1\r\n")
                .replace("TimeSpan: -1\r\n", "TimeSpan: 100\r\n");
        let mut player = build(&looping);
        for _ in 0..40 {
            step(&mut player, 100.);
        }
        assert!(!player.finished());
        assert_eq!(player.time_ms(), 4000.);
    }

    /// Controllers 100 and 101 integrate `dspl = (2*v + Acc*dt) * dt * 0.5`
    /// with `v = Vel + Acc*age`, read out of `0x6eaf60` and `0x6eb230`.
    #[test]
    fn controller_100_integrates_the_verified_trapezoid() {
        let source = header37(1)
            + &SPRITE.replace(
                "CtrlMethodCount: 0\r\n",
                "CtrlMethodCount: 1\r\nCtrlType: 100\r\nStartTime: 0.000000\r\n\
                 EndTime: -1.000000\r\nDir: 1.000000, 0.000000, 0.000000\r\n\
                 Vel: 1.000000\r\nAcc: 2.000000\r\n",
            );
        let mut player = build(&source);

        // The element clock advances before its controllers run, so the first
        // frame integrates at age 0.1 s: v = 1.2,
        // dspl = 0.5*(2*1.2 + 2*0.1)*0.1 = 0.13, and the root scale of 2 puts
        // the sprite centre at 2*(1 + 0.13).
        step(&mut player, 100.);
        let centre = quad_centre(&player.quads()[0]);
        assert!((centre.x - 2. * 1.13).abs() <= 1e-5, "{centre}");
        assert!((centre.y - 4.).abs() <= 1e-5, "{centre}");

        // age 0.2: v = 1.4, dspl = 0.5*(2*1.4 + 2*0.1)*0.1 = 0.15, total 0.28.
        step(&mut player, 100.);
        let centre = quad_centre(&player.quads()[0]);
        assert!((centre.x - 2. * 1.28).abs() <= 1e-5, "{centre}");
    }

    /// `EndTime` is compared against 0.0 at `0x8d58a0`, so a controller stops
    /// once the element clock passes a non-negative end and any negative end
    /// means forever.
    #[test]
    fn controller_end_time_stops_the_step() {
        let source = header37(1)
            + &SPRITE.replace(
                "CtrlMethodCount: 0\r\n",
                "CtrlMethodCount: 1\r\nCtrlType: 100\r\nStartTime: 0.000000\r\n\
                 EndTime: 150.000000\r\nDir: 1.000000, 0.000000, 0.000000\r\n\
                 Vel: 1.000000\r\nAcc: 0.000000\r\n",
            );
        let mut player = build(&source);
        step(&mut player, 100.);
        let after_first = quad_centre(&player.quads()[0]).x;
        assert!((after_first - 2. * 1.1).abs() <= 1e-5, "{after_first}");
        step(&mut player, 100.);
        let after_second = quad_centre(&player.quads()[0]).x;
        assert_eq!(
            after_second, after_first,
            "the controller is past its EndTime and must not move the sprite"
        );
    }

    /// Controller 105 keeps one shared sub-integer remainder (temp+0x24), so a
    /// rate below one alpha unit per frame still fades.
    #[test]
    fn controller_105_carries_its_remainder_and_clamps() {
        // An opaque root, so one alpha unit in the packed colour is one unit in
        // the output byte.
        let opaque = |delta: i32| {
            (header37(1)
                + &SPRITE.replace(
                    "CtrlMethodCount: 0\r\n",
                    &format!(
                        "CtrlMethodCount: 1\r\nCtrlType: 105\r\nStartTime: 0.000000\r\n\
                         EndTime: -1.000000\r\nColorDelta: 0, 0, 0, {delta}\r\n"
                    ),
                ))
                .replace("DefaultAlpha: 0.5\r\n", "DefaultAlpha: 1.000000\r\n")
        };

        let mut player = build(&opaque(-3));
        // 0.3 units per 100 ms frame: a player that truncated every step would
        // never fade at all, because no single frame reaches one unit. The
        // client keeps the fraction in temp+0x24 and the fade still lands.
        for frame in 0..3 {
            step(&mut player, 100.);
            assert_eq!(alpha8(&player.quads()[0]), 255, "frame {frame}");
        }
        step(&mut player, 100.);
        assert_eq!(
            alpha8(&player.quads()[0]),
            254,
            "four frames of 0.3 add up to one whole unit"
        );

        // A steep rate clamps the channel at zero and the cutoff drops the quad.
        let mut player = build(&opaque(-4000));
        step(&mut player, 100.);
        assert!(player.quads().is_empty(), "alpha reached zero");
    }

    #[test]
    fn controller_106_ramps_scale_and_clamps_it() {
        let source = header37(1)
            + &SPRITE.replace("Scale: 2\r\n", "Scale: 1\r\n").replace(
                "CtrlMethodCount: 0\r\n",
                "CtrlMethodCount: 1\r\nCtrlType: 106\r\nStartTime: 0.000000\r\n\
                     EndTime: -1.000000\r\nScaleChage: 1.000000, 0.010000, 1.500000\r\n",
            );
        let mut player = build(&source);
        step(&mut player, 100.);
        assert!(
            (extent_x(&player.quads()[0]) - 4. * 0.25 * 1.1).abs() <= 1e-5,
            "{}",
            extent_x(&player.quads()[0])
        );
        step(&mut player, 900.);
        assert!(
            (extent_x(&player.quads()[0]) - 4. * 0.25 * 1.5).abs() <= 1e-5,
            "the ramp is clamped at its maximum: {}",
            extent_x(&player.quads()[0])
        );
    }

    /// Emission rate, TTL unlinking and the quota all come from the particle
    /// system's own counters (`0x715b50`, `0x71a247`, `0x715c09`).
    #[test]
    fn particles_respect_emission_rate_ttl_and_quota() {
        let emitter = |quota: i32, ttl: f32| {
            header37(1)
                + &PARTICLE
                    .replace("Quota: -1\r\n", &format!("Quota: {quota}\r\n"))
                    .replace("TTL: 0.500000\r\n", &format!("TTL: {ttl:.6}\r\n"))
        };

        // 4 particles per second over a 250 ms frame emits exactly one per
        // frame, and a particle is unlinked once its age reaches the TTL
        // (0x71a247), so a one second life holds three of them alive.
        let mut player = build(&emitter(-1, 1.));
        for live in [1, 2, 3, 3, 3, 3, 3] {
            step(&mut player, 250.);
            assert_eq!(player.quads().len(), live, "at {} ms", player.time_ms());
        }
        assert_eq!(player.stats().particles, 3);
        assert_eq!(player.stats().emitters, 1);

        // A quota of one caps the same emitter.
        let mut capped = build(&emitter(1, 30.));
        for _ in 0..4 {
            step(&mut capped, 250.);
            assert_eq!(capped.quads().len(), 1, "quota caps the live count");
        }
        assert_eq!(capped.stats().particles, 1);
    }

    /// A minimal element 123, the retail record from
    /// `gfx/场景/符文阵效果2改.gfx` with its five affectors removed so the
    /// emission counters can be read on their own.
    const PARTICLE: &str = "GFXELEMENTID: 123\r\nName: emitter\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
        RepeatCount: -1\r\nRepeatDelay: 0\r\nTexFile: 粒子\\particle001.dds\r\nBindEle: \r\n\
        ZEnable: 1\r\nMatchGrnd: 0\r\nGroundHeight: 0\r\nTexRow: 1\r\nTexCol: 1\r\n\
        TexInterval: 50\r\nPriority: 0\r\nIsDummy: 0\r\nDummyEle: \r\nWarp: 0\r\nTileMode: 0\r\n\
        TexSpeed: 0.000000\r\nTexSpeed: 0.000000\r\nUReverse: 0\r\nVReverse: 0\r\nUVExchg: 0\r\n\
        RenderLayer: 0\r\nNoDownSample: 0\r\nResetLoopEnd: 0\r\nTexAnimMaxTime: 0\r\nQuota: -1\r\n\
        ParticleWidth: 0.200000\r\nParticleHeight: 0.200000\r\n3DParticle: 0\r\nFacing: 0\r\n\
        ScaleNoOff: 0\r\nNoScale: 0\r\nNoScale: 0\r\nOrgPt: 0.500000\r\nOrgPt: 0.500000\r\n\
        IsUseParUV: 0\r\nIsStartOnGrnd: 0\r\nEmissionRate: 4.000000\r\nAngle: 0.000000\r\n\
        Speed: 0.000000\r\nParAcc: 0.000000\r\nAccDir: 0.000000, 0.000000, -1.000000\r\n\
        Acc: 0.000000\r\nTTL: 0.500000\r\nColorMin: -1\r\nColorMax: -1\r\nScaleMin: 1.000000\r\n\
        ScaleMax: 1.000000\r\nRotMin: 0.000000\r\nRotMax: 0.000000\r\nIsSurface: 0\r\nIsBind: 0\r\n\
        IsDrag: 0\r\nDragPow: 1.000000\r\nParIniDir: 0.000000, 0.000000, 0.000000\r\n\
        IsUseHSVInterp: 0\r\nAreaSize: 0.000000, 0.000000, 0.000000\r\nIsAvgGen: 0\r\n\
        AlphaSeg: 0\r\nBetaSeg: 0\r\nAffectorCount: 0\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\n\
        InterpolateMode: 0\r\nTimeSpan: -1\r\nPosition: 0.000000, 0.000000, 0.000000\r\nColor: -1\r\n\
        Scale: 1.000000\r\nDirection: 0.000000, 0.000000, 0.000000, 1.000000\r\nRad_2D: 0.000000\r\n\
        CtrlMethodCount: 0\r\n";

    /// A spawn context with an identity placement, so a particle offset can be
    /// read straight off the returned state.
    fn spawn_context<'a>(view: &'a View, common: &'a Common) -> ElementContext<'a> {
        ElementContext {
            id: 123,
            placement: Mat4::IDENTITY,
            view,
            common,
            version: 37,
            color: u32::MAX,
            color_scale: 1.,
            scale: 1.,
            emission_scale: 1.,
            rotation: 0.,
            clock: 0.,
        }
    }

    /// `AreaSize` is an ellipsoid of semi-axes `AreaSize * 0.5`: `0x729670`
    /// scales each basis axis by half the authored extent and `0x728270`
    /// multiplies a unit-ball sample by the result before adding the emitter
    /// position. A box of the same extents would put particles at its corners,
    /// where the ellipsoid inequality fails by a factor of three.
    #[test]
    fn area_size_spawns_inside_its_ellipsoid() {
        let view = view();
        let common = Common::default();
        let context = spawn_context(&view, &common);
        let mut particle = Particle::default();
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(31);
        let mut reach = Vec3::ZERO;
        for _ in 0..4_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            let normalized = offset / Vec3::new(1., 0.5, 0.25);
            assert!(
                normalized.length_squared() <= 1. + 1e-4,
                "{offset} is outside the ellipsoid"
            );
            reach = reach.max(offset.abs());
        }
        // The volume is filled out to its semi-axes, not held well inside them.
        assert!(reach.x > 0.9 && reach.y > 0.45 && reach.z > 0.2, "{reach}");
    }

    /// The unbound basis carries a scalar the bound one does not: `0x71a35b` and
    /// `0x71a501` hand `0x729670` the product of the interpolated keypoint
    /// `Scale` (`[esp+0x48]`) and the root `PlaySpeed` (`[esp+0x24]`, written at
    /// `0x71a225` from root+0x1ec), and `0x729763` folds it into all three
    /// `AreaSize` components before the rotation.
    #[test]
    fn unbound_emission_carries_the_basis_scalar() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 123;
        // Keypoint `Scale` 2 under a root `PlaySpeed` of 3.
        context.emission_scale = 6.;
        let mut particle = Particle::default();
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(11);
        let mut reach = Vec3::ZERO;
        for _ in 0..4_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            let normalized = offset / Vec3::new(6., 3., 1.5);
            assert!(
                normalized.length_squared() <= 1. + 1e-4,
                "{offset} is outside the scaled ellipsoid"
            );
            reach = reach.max(offset.abs());
        }
        assert!(reach.x > 5. && reach.y > 2.5 && reach.z > 1.2, "{reach}");
    }

    /// The bound branch never reads that argument — `0x729684` takes `AreaSize.x`
    /// and multiplies by the 0.5 at `0x8d5894` — so a bound emitter's volume is
    /// the authored ellipsoid whatever the keypoint scale says.
    #[test]
    fn bound_emission_ignores_the_basis_scalar() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 123;
        context.emission_scale = 6.;
        let mut particle = Particle::default();
        particle.is_bind = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(11);
        let mut reach = Vec3::ZERO;
        for _ in 0..2_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            let normalized = offset / Vec3::new(1., 0.5, 0.25);
            assert!(
                normalized.length_squared() <= 1. + 1e-4,
                "{offset} is outside the authored ellipsoid"
            );
            reach = reach.max(offset.abs());
        }
        assert!(reach.x > 0.9 && reach.y > 0.45 && reach.z > 0.2, "{reach}");
    }

    /// Ids 120 and 122 hold `0x56fbb0` in vtable slot [9] instead of the basis
    /// builder, so their class never fills `+0xc0…+0xe0` at all and no scalar can
    /// reach it.
    #[test]
    fn classes_without_the_basis_builder_ignore_the_scalar() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 120;
        context.emission_scale = 6.;
        let mut particle = Particle::default();
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(11);
        for _ in 0..1_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            let normalized = offset / Vec3::new(1., 0.5, 0.25);
            assert!(
                normalized.length_squared() <= 1. + 1e-4,
                "{offset} is outside the authored ellipsoid"
            );
        }
    }

    /// Id 125's slot [9] is its own `0x727220`, which writes a single scalar at
    /// `+0xc0` rather than the nine-float matrix, and its sampler reads `Radius`.
    #[test]
    fn the_sphere_radius_ignores_the_basis_scalar() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 125;
        context.emission_scale = 6.;
        let mut particle = Particle::default();
        particle.radius = Some(2.);
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(11);
        for _ in 0..1_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            assert!(offset.length() <= 2. + 1e-4, "{offset} escaped the radius");
        }
    }

    /// Id 124's surface sampler reads the same nine floats the volume sampler
    /// does, so its cylinder is scaled by the same factor: the cross section
    /// moves out to `Scale * PlaySpeed * AreaSize * 0.5`.
    #[test]
    fn surface_emission_carries_the_basis_scalar() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 124;
        context.emission_scale = 4.;
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(11);
        for _ in 0..1_000 {
            let offset = spawn(&particle, None, &mut rng, &context).position;
            let cross = (offset.x / 4.).powi(2) + (offset.y / 2.).powi(2);
            assert!((cross - 1.).abs() < 1e-3, "{offset} is off the ellipse");
            assert!(
                offset.z.abs() <= 1. + 1e-4,
                "{offset} is outside the height"
            );
        }
    }

    /// The same check over retail data: every unbound element of ids 121, 123 and
    /// 124 that authors an `AreaSize`, played with its own first keypoint `Scale`
    /// and its root's `PlaySpeed`, stays inside the scaled ellipsoid and reaches
    /// it. Elements whose authored factor or extent collapses to zero are counted
    /// and skipped rather than divided by.
    #[test]
    fn corpus_unbound_emission_carries_the_basis_scalar() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        let (mut parsed, mut elements, mut scaled, mut degenerate) =
            (0usize, 0usize, 0usize, 0usize);
        let mut reach = 0f32;
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed_root) = crate::gfx_file::parse(&source) else {
                continue;
            };
            parsed += 1;
            for element in &parsed_root.elements {
                if !matches!(element.id, 121 | 123 | 124) {
                    continue;
                }
                let crate::gfx_file::Spec::Particle(particle) = &element.spec else {
                    continue;
                };
                if particle.is_bind {
                    continue;
                }
                let Some(area) = particle.area_size else {
                    continue;
                };
                elements += 1;
                context.id = element.id;
                context.emission_scale =
                    element.keypoints.first().map(|key| key.scale).unwrap_or(1.)
                        * parsed_root.play_speed;
                let half = area * (0.5 * context.emission_scale);
                if half.x == 0. || half.y == 0. || half.z == 0. {
                    degenerate += 1;
                    continue;
                }
                if (context.emission_scale - 1.).abs() > 1e-6 {
                    scaled += 1;
                }
                let mut rng = Rng::new(elements as u32 * 7919 + 13);
                for _ in 0..8 {
                    let offset = spawn(particle, None, &mut rng, &context).position;
                    assert!(offset.is_finite(), "{} spawned {offset}", file.display());
                    if particle.is_surface && element.id == 124 {
                        // A cylinder sample reaches sqrt(2) half-extents, so the
                        // ellipsoid inequality does not describe it: its cross
                        // section sits on the ellipse and its height stays inside.
                        if half.x != 0. && half.y != 0. {
                            let cross = (offset.x / half.x).powi(2) + (offset.y / half.y).powi(2);
                            assert!(
                                (cross - 1.).abs() < 1e-3,
                                "{}: {offset} is off the ellipse {half}",
                                file.display()
                            );
                        }
                        assert!(
                            offset.z.abs() <= half.z.abs() + 1e-6,
                            "{}: {offset} is outside the height {half}",
                            file.display()
                        );
                    } else {
                        let normalized = (offset / half).length_squared();
                        assert!(
                            normalized <= 1. + 1e-3,
                            "{}: {offset} is outside {half}",
                            file.display()
                        );
                        reach = reach.max(normalized);
                    }
                }
            }
        }
        println!(
            "corpus basis scalar: {parsed} parsed, {elements} unbound elements with an \
             AreaSize, {scaled} under a factor other than 1, {degenerate} degenerate, \
             closest sample to the boundary {reach:.6}"
        );
        assert!(parsed > 5_000, "only {parsed} corpus files parsed");
        assert!(elements > 4_000, "only {elements} unbound elements");
        assert!(scaled > 1_000, "only {scaled} elements are scaled");
        assert!(reach > 0.9, "nothing reached the boundary: {reach}");
    }

    /// Id 123's surface sampler `0x727f00` puts the particle *on* the ellipsoid
    /// rather than inside it: the position is `D.x*basis0 + D.y*basis1 + D.z*basis2`
    /// for a unit direction `D`, and the basis is the same `AreaSize * 0.5` the
    /// volume path uses. Every sample therefore satisfies the ellipsoid equation
    /// with equality, where the volume path satisfies it strictly.
    #[test]
    fn surface_emission_lands_on_the_ellipsoid() {
        let view = view();
        let common = Common::default();
        let context = spawn_context(&view, &common);
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(41);
        let mut reach = Vec3::ZERO;
        for _ in 0..2_000 {
            let position = spawn(&particle, None, &mut rng, &context).position;
            let normalized = position / Vec3::new(1., 0.5, 0.25);
            assert!(
                (normalized.length_squared() - 1.).abs() < 1e-4,
                "{position} is not on the ellipsoid"
            );
            reach = reach.max(position.abs());
        }
        // The samples cover the whole surface, so all three semi-axes are reached.
        assert!(
            reach.x > 0.99 && reach.y > 0.49 && reach.z > 0.24,
            "{reach}"
        );
    }

    /// The surface sampler writes the negated sample to node+0x44 (`0x7281ad`
    /// through `0x7281b3`, storing the vector built by the `fchs` chain at
    /// `0x72816a`) and, unlike ids 121 and 124, the birth flow does not call the
    /// direction slot afterwards. A cone angle authored on the element is
    /// therefore ignored: the direction is exactly `-D`, anti-parallel to the
    /// position measured in semi-axis units.
    #[test]
    fn surface_direction_is_the_negated_sample() {
        let view = view();
        let common = Common::default();
        let context = spawn_context(&view, &common);
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        // A wide cone, which would bend every direction if it were consulted.
        particle.angle = std::f32::consts::FRAC_PI_2;
        let mut rng = Rng::new(43);
        for _ in 0..500 {
            let state = spawn(&particle, None, &mut rng, &context);
            let radial = state.position / Vec3::new(1., 0.5, 0.25);
            assert!(
                (state.direction + radial).length() < 1e-4,
                "direction {} should negate the sample behind position {}",
                state.direction,
                state.position
            );
        }
    }

    /// The two rotations sweep the sphere uniformly: the first rotates `0x9c0358`
    /// about `0x9c0348`, the second rotates `0x9c0348` about the first result.
    /// A single rotation about a fixed axis would confine the samples to one
    /// circle, so a centred mean and both signs on every axis are what
    /// distinguish the two-draw construction.
    #[test]
    fn surface_directions_cover_the_sphere() {
        let mut rng = Rng::new(47);
        let samples = 4_000;
        let mut mean = Vec3::ZERO;
        let mut signs = [0u8; 3];
        for _ in 0..samples {
            let d = surface_direction(&mut rng);
            assert!((d.length() - 1.).abs() < 1e-4, "{d} is not unit length");
            mean += d;
            for (slot, component) in signs.iter_mut().zip([d.x, d.y, d.z]) {
                *slot |= if component > 0. { 1 } else { 2 };
            }
        }
        let mean = mean / samples as f32;
        assert!(
            mean.length() < 0.05,
            "the mean direction {mean} is off centre"
        );
        assert_eq!(
            signs,
            [3, 3, 3],
            "every axis should be covered in both directions"
        );
    }

    /// The surface path spends exactly the client's two draws on the direction
    /// (`0x727f0e` and `0x727fe1`), where the volume path spends at least the
    /// three of `0x728270`'s rejection loop. Getting the count wrong would shift
    /// every sample that follows it.
    #[test]
    fn surface_direction_spends_two_draws() {
        let mut rng = Rng::new(53);
        surface_direction(&mut rng);
        let mut reference = Rng::new(53);
        reference.next_u32();
        reference.next_u32();
        assert_eq!(rng.next_u32(), reference.next_u32());
    }

    /// Only id 123's sampler is decoded, so only id 123 gets the surface path.
    /// Id 121's `0x727800` spends ten draws and calls none of the vector helpers,
    /// and ids 122 and 125 never read the flag, so an element of another class
    /// still fills the volume even with `IsSurface` set.
    #[test]
    fn other_classes_keep_the_volume_path() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 121;
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(59);
        let mut closest = f32::MAX;
        for _ in 0..500 {
            let position = spawn(&particle, None, &mut rng, &context).position;
            let normalized = position / Vec3::new(1., 0.5, 0.25);
            closest = closest.min(normalized.length_squared());
        }
        // A surface sampler would pin every sample to exactly 1.
        assert!(
            closest < 0.5,
            "id 121 should still fill the volume; its closest sample was {closest}"
        );
    }

    /// Id 124's surface sample is the lateral surface of a cylinder, not an
    /// ellipsoid: `0x729be0` takes its first two coefficients from a full turn
    /// about `0x9c0338` and its third from an independent uniform in `[-1, 1]`.
    /// The cross-section therefore sits exactly on the ellipse while the height
    /// covers the whole extent, which no sphere sample does.
    #[test]
    fn id_124_surface_emission_is_a_cylinder() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 124;
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut rng = Rng::new(61);
        let mut height = 0f32;
        for _ in 0..2_000 {
            let position = spawn(&particle, None, &mut rng, &context).position;
            let cross = (position.x / 1.).powi(2) + (position.y / 0.5).powi(2);
            assert!(
                (cross - 1.).abs() < 1e-4,
                "{position} is off the cylinder's cross-section"
            );
            assert!(
                position.z.abs() <= 0.25 + 1e-4,
                "{position} is outside the cylinder's height"
            );
            height = height.max(position.z.abs());
        }
        // The height is uniform, so it reaches both ends; a sphere sample would
        // trade height for cross-section radius and never pin the radius at 1.
        assert!(height > 0.24, "the height only reached {height}");
    }

    /// Id 124's sampler writes no direction, and its birth flow calls the
    /// direction slot after both samplers (`0x729bc1`), so the emission cone still
    /// applies there — unlike id 123, whose flow goes straight to the TTL slot.
    /// With no cone angle the direction is the emission axis and costs no draws, so
    /// the same element with the flag cleared must produce exactly the same one.
    #[test]
    fn id_124_surface_emission_keeps_the_cone_direction() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 124;
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(2., 1., 0.5));
        let mut plain = Particle::default();
        plain.area_size = particle.area_size;
        let mut rng = Rng::new(67);
        let surface = spawn(&particle, None, &mut rng, &context);
        let volume = spawn(&plain, None, &mut rng, &context);
        assert_eq!(surface.direction, volume.direction);
        // And it is not id 123's negated sample.
        let radial = surface.position / Vec3::new(1., 0.5, 0.25);
        assert!(
            (surface.direction + radial).length() > 1e-3,
            "id 124 should not negate its sample; direction {} for position {}",
            surface.direction,
            surface.position
        );
    }

    /// The cylinder sample spends the client's two draws, one for the turn and one
    /// for the height (`0x729bed` and `0x729cba`).
    #[test]
    fn cylinder_sample_spends_two_draws() {
        let mut rng = Rng::new(71);
        cylinder_sample(&mut rng);
        let mut reference = Rng::new(71);
        reference.next_u32();
        reference.next_u32();
        assert_eq!(rng.next_u32(), reference.next_u32());
    }

    /// The archive's id-124 surface elements, measured against the cylinder their
    /// own authored `AreaSize` describes. The cross-section check is skipped where
    /// an extent is zero, since the sample then collapses onto an axis.
    #[test]
    fn corpus_cylinder_emission_holds_on_authored_area_sizes() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 124;
        let (mut parsed, mut elements, mut zeroed) = (0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed_root) = crate::gfx_file::parse(&source) else {
                continue;
            };
            parsed += 1;
            for element in &parsed_root.elements {
                if element.id != 124 {
                    continue;
                }
                let crate::gfx_file::Spec::Particle(particle) = &element.spec else {
                    continue;
                };
                if !particle.is_surface {
                    continue;
                }
                elements += 1;
                let half = particle.area_size.unwrap_or(Vec3::ZERO) * 0.5;
                if half.x == 0. || half.y == 0. || half.z == 0. {
                    zeroed += 1;
                }
                let mut rng = Rng::new(elements as u32 * 7919 + 13);
                for _ in 0..8 {
                    let position = spawn(particle, None, &mut rng, &context).position;
                    assert!(
                        position.is_finite(),
                        "{} spawned {position}",
                        file.display()
                    );
                    if half.x != 0. && half.y != 0. {
                        let cross = (position.x / half.x).powi(2) + (position.y / half.y).powi(2);
                        assert!(
                            (cross - 1.).abs() < 1e-3,
                            "{} spawned {position} off its cylinder",
                            file.display()
                        );
                    }
                    let slack = 1e-3 * half.z.abs().max(1.);
                    assert!(
                        position.z.abs() <= half.z.abs() + slack,
                        "{} spawned {position} outside its height",
                        file.display()
                    );
                }
            }
        }
        println!(
            "corpus cylinder: {parsed} files parsed, {elements} id-124 surface elements, \
             {zeroed} with a zero extent"
        );
        assert!(
            parsed > 5_000,
            "the census parsed 5,457 files, got {parsed}"
        );
        assert!(
            elements > 800,
            "the census found 1,036 id-124 surface elements, got {elements}"
        );
    }

    /// The retail archive, when `tools/pck_lib.py` has extracted it beside the
    /// checkout. CI has no archive, so the corpus tests skip instead of failing.
    /// Member names have to be decoded GBK: anything else silently breaks child
    /// reference resolution, which surfaces as warnings and never as an error, so
    /// the resolved count is the thing worth reading.
    fn corpus_root() -> Option<std::path::PathBuf> {
        let root = std::path::PathBuf::from("google_drive/x");
        root.is_dir().then_some(root)
    }

    /// Every file under a directory, sorted, so a sweep is reproducible. The
    /// archive holds one member whose name is literally `.gfx`, which a `*.gfx`
    /// glob would skip.
    fn corpus_files(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(root)
            .map(|dir| {
                dir.filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                corpus_files(&entry, out);
            } else {
                out.push(entry);
            }
        }
    }

    /// Every corpus element that takes id 123's surface path has to land on the
    /// ellipsoid its own authored `AreaSize` spans, including the degenerate
    /// values a synthetic fixture would not think of. The zero-safe form of the
    /// property is `position == -direction * (AreaSize * 0.5)` component-wise,
    /// which still holds when an extent is zero; the ellipsoid equation itself is
    /// only checked where all three extents are non-zero.
    #[test]
    fn corpus_surface_emission_holds_on_authored_area_sizes() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let common = Common::default();
        let context = spawn_context(&view, &common);
        let (mut parsed, mut elements) = (0usize, 0usize);
        let (mut zeroed, mut negative, mut missing) = (0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed_root) = crate::gfx_file::parse(&source) else {
                continue;
            };
            parsed += 1;
            for element in &parsed_root.elements {
                if element.id != 123 {
                    continue;
                }
                let crate::gfx_file::Spec::Particle(particle) = &element.spec else {
                    continue;
                };
                if !particle.is_surface {
                    continue;
                }
                elements += 1;
                let half = match particle.area_size {
                    Some(area) => {
                        if area.x == 0. || area.y == 0. || area.z == 0. {
                            zeroed += 1;
                        }
                        if area.x < 0. || area.y < 0. || area.z < 0. {
                            negative += 1;
                        }
                        area * 0.5
                    }
                    None => {
                        missing += 1;
                        Vec3::ZERO
                    }
                };
                let mut rng = Rng::new(elements as u32 * 7919 + 11);
                for _ in 0..8 {
                    let state = spawn(particle, None, &mut rng, &context);
                    let tolerance = 1e-4 * half.length().max(1.);
                    assert!(
                        state.position.is_finite() && state.direction.is_finite(),
                        "{} spawned position {} direction {}",
                        file.display(),
                        state.position,
                        state.direction
                    );
                    assert!(
                        (state.direction.length() - 1.).abs() < 1e-4,
                        "{} spawned a non-unit direction {}",
                        file.display(),
                        state.direction
                    );
                    let expected = -state.direction * half;
                    assert!(
                        (state.position - expected).length() < tolerance,
                        "{} spawned {} where the surface sample implies {}",
                        file.display(),
                        state.position,
                        expected
                    );
                    if half.x != 0. && half.y != 0. && half.z != 0. {
                        let normalized = state.position / half;
                        assert!(
                            (normalized.length_squared() - 1.).abs() < 1e-3,
                            "{} spawned {} off its ellipsoid",
                            file.display(),
                            state.position
                        );
                    }
                }
            }
        }
        println!(
            "corpus surface: {parsed} files parsed, {elements} id-123 surface elements, \
             {zeroed} with a zero extent, {negative} negative, {missing} without AreaSize"
        );
        assert!(
            parsed > 5_000,
            "the census parsed 5,457 files, got {parsed}"
        );
        assert!(
            elements > 1_000,
            "the census found 1,520 id-123 surface elements, got {elements}"
        );
    }

    /// The surface path moves particles; it does not create or destroy them, so a
    /// play-through of the whole archive should stay finite and still run to
    /// completion. This is the check that real authored data, with values no
    /// fixture anticipated, produces neither a panic nor NaN geometry. 120 frames
    /// of 16 ms, one player per file, children resolved from the same archive.
    #[test]
    fn corpus_plays_through_with_the_surface_path() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let (mut parsed, mut played, mut finished) = (0usize, 0usize, 0usize);
        let (mut quads, mut nonfinite, mut children) = (0usize, 0usize, 0usize);
        let mut warnings: std::collections::BTreeSet<String> = Default::default();
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed_root) = crate::gfx_file::parse(&source) else {
                continue;
            };
            parsed += 1;
            let archive = root.clone();
            let mut player = match Player::new(parsed_root, 7, &mut |path: &str| {
                let bytes =
                    std::fs::read(archive.join(path)).map_err(|e| format!("{path}: {e}"))?;
                let (source, _) = crate::gfx_file::decode(&bytes);
                crate::gfx_file::parse(&source)
            }) {
                Ok(player) => player,
                Err(_) => continue,
            };
            played += 1;
            for _ in 0..120 {
                if player.advance(16., &view, Mat4::IDENTITY, 1., 1.).is_err() {
                    break;
                }
                for quad in player.quads() {
                    quads += 1;
                    if quad
                        .positions
                        .iter()
                        .flatten()
                        .any(|component| !component.is_finite())
                    {
                        nonfinite += 1;
                    }
                }
            }
            children += player.stats().children;
            if player.finished() {
                finished += 1;
            }
            warnings.extend(player.warnings().iter().cloned());
        }
        println!(
            "corpus sweep: {parsed} parsed, {played} played, {quads} quads, {finished} finished, \
             {children} child elements, {} distinct warnings, {nonfinite} non-finite quads",
            warnings.len()
        );
        assert_eq!(
            nonfinite, 0,
            "the surface path produced non-finite geometry"
        );
        assert!(
            parsed > 5_000,
            "the census parsed 5,457 files, got {parsed}"
        );
        assert!(
            played > 5_000,
            "the census played 5,457 files, got {played}"
        );
    }

    /// The ball sample is uniform in volume, which is what the client's
    /// rejection loop produces (`0x728270`: three draws of
    /// `rand() * (2/32767) - 1`, retried while `x*x + y*y + z*z > 1`). Its mean
    /// radius is 3/4; a sample confined to the surface would average 1.
    #[test]
    fn unit_ball_samples_fill_the_volume() {
        let mut rng = Rng::new(37);
        let samples = 4_000;
        let mut total = 0f32;
        for _ in 0..samples {
            let point = random_unit_ball(&mut rng);
            assert!(point.length() <= 1. + 1e-5, "{point} is outside the ball");
            total += point.length();
        }
        let mean = total / samples as f32;
        assert!(
            (0.72..0.78).contains(&mean),
            "the mean radius should be 0.75, got {mean}"
        );
    }

    /// The client's cone sampler makes the polar angle uniform in `[0, Angle]`
    /// (`0x723440` draws `alpha = rand() * (1/32767) * Angle` and feeds it to the
    /// axis-angle helper `0x7b6fd0`), not uniform in solid angle. Half of a 90
    /// degree cone's samples therefore sit inside 45 degrees of the axis, where a
    /// solid-angle sample would put only `1 - cos(45) = 0.293` of them.
    #[test]
    fn cone_polar_angle_is_uniform_in_the_angle() {
        let axis = Vec3::Y;
        let rim = std::f32::consts::FRAC_PI_4.cos();
        let mut rng = Rng::new(11);
        let samples = 4_000;
        let mut inside = 0usize;
        for _ in 0..samples {
            let direction = cone_direction(axis, std::f32::consts::FRAC_PI_2, &mut rng);
            assert!(
                (direction.length() - 1.).abs() < 1e-4,
                "{direction} is not a unit vector"
            );
            if direction.dot(axis) > rim {
                inside += 1;
            }
        }
        let fraction = inside as f32 / samples as f32;
        assert!(
            (0.45..0.55).contains(&fraction),
            "half of the cone should sit inside 45 degrees, got {fraction}"
        );
    }

    /// `Angle` is measured from the emission axis and is never clamped, which
    /// the corpus needs: 416 elements author 30 degrees, 248 author a half turn
    /// and 2,130 a full turn — both of which sweep the whole sphere — and 36
    /// author a negative value. The sphere cases have to reach behind the axis
    /// and the negative ones have to open a mirrored cone instead of collapsing
    /// onto the axis, which the `angle > 0` gate they replaced did.
    #[test]
    fn cone_angle_is_never_clamped() {
        let axis = Vec3::new(0., 0., 1.);
        let mut rng = Rng::new(13);

        let rim = (std::f32::consts::FRAC_PI_6 + 1e-4).cos();
        for _ in 0..1_000 {
            let direction = cone_direction(axis, std::f32::consts::FRAC_PI_6, &mut rng);
            assert!(
                direction.dot(axis) > rim,
                "{direction} escaped a 30 degree cone"
            );
        }

        let mut behind = 0usize;
        for _ in 0..1_000 {
            let direction = cone_direction(axis, std::f32::consts::TAU, &mut rng);
            if direction.dot(axis) < -0.9 {
                behind += 1;
            }
        }
        assert!(
            behind > 10,
            "a full turn covers the whole sphere, got {behind}"
        );

        let mut forward = 0usize;
        let mut spread = 0f32;
        for _ in 0..1_000 {
            let direction = cone_direction(axis, -std::f32::consts::FRAC_PI_2, &mut rng);
            forward += (direction.dot(axis) > 0.) as usize;
            spread = spread.max(1. - direction.dot(axis));
        }
        assert_eq!(forward, 1_000, "a mirrored cone stays in front of its axis");
        assert!(spread > 0.9, "and is not a single ray: spread {spread}");
    }

    /// The colour slot `0x7237f0` lerps each channel as `min * (1 - u) + max * u`
    /// and truncates it through the float-to-int helper `0x892e78`, so a sample
    /// never lands above its exact value. Rounding would, on most draws.
    #[test]
    fn color_channels_truncate_instead_of_rounding() {
        let (min, max) = (0x0011_2233u32, 0xffee_ddccu32);
        // Seeded alike, so the reference rng replays `random_color`'s own draws.
        let mut actual = Rng::new(23);
        let mut reference = Rng::new(23);
        let mut always_truncates = true;
        let mut differs_from_rounding = false;
        for _ in 0..500 {
            let got = random_color_rgb(min, max, &mut actual);
            let mut want_truncated = 0u32;
            let mut want_rounded = 0u32;
            for shift in [0u32, 8, 16, 24] {
                let low = ((min >> shift) & 0xff) as f32;
                let high = ((max >> shift) & 0xff) as f32;
                let u = reference.unit();
                let exact = low * (1. - u) + high * u;
                want_truncated |= (exact as u32) << shift;
                want_rounded |= (exact.round().clamp(0., 255.) as u32) << shift;
            }
            always_truncates &= got == want_truncated;
            differs_from_rounding |= got != want_rounded;
        }
        assert!(always_truncates, "0x892e78 truncates toward zero");
        assert!(differs_from_rounding, "and that is observable");
    }

    /// `0x789ad0`: the byte layout the converter reads, the zero-saturation
    /// special cases at `0x789bf6` and `0x789c29`, and the alpha that rides
    /// along as a float instead of being converted back.
    #[test]
    fn rgb_to_hsva_matches_the_client_converter() {
        assert_eq!(rgb_to_hsva(0xffff_0000), [0., 1., 1., 1.], "pure red");
        for (packed, hue) in [(0xff00_ff00u32, 1. / 3.), (0xff00_00ff, 2. / 3.)] {
            let got = rgb_to_hsva(packed);
            assert!((got[0] - hue).abs() < 1e-6, "0x{packed:08x} hue {}", got[0]);
            assert_eq!((got[1], got[2]), (1., 1.));
        }
        let grey = rgb_to_hsva(0xff80_8080);
        assert_eq!(
            grey[0], 0.,
            "an achromatic colour stores a literal zero hue"
        );
        assert_eq!(grey[1], 0., "and a zero saturation");
        let black = rgb_to_hsva(0xff00_0000);
        assert_eq!(black[2], 0.);
        assert_eq!(black[1], 0., "the value channel is tested before dividing");
        assert!(
            (rgb_to_hsva(0x8000_0000)[3] - 128. / 255.).abs() < 1e-6,
            "alpha is scaled like the colour bytes and kept as a float"
        );
        // Magenta is the branch whose sector offset needs the negative-hue wrap
        // at `0x789cab`: (red - green) / delta + 4 = 4, and blue being the
        // maximum is what selects it.
        assert!(
            (rgb_to_hsva(0xffff_00ff)[0] - 5. / 6.).abs() < 1e-6,
            "magenta"
        );
    }

    /// The parser stores what the sampler reads back (`0x724434` into emitter+0x38,
    /// `0x723826` out of it), so a colour surviving the round trip is the
    /// available check that the two routines agree on the layout, the six
    /// sectors and the value channel's rounding bias.
    #[test]
    fn hsv_conversion_round_trips_the_palette() {
        let mut checked = 0;
        for alpha in [0u32, 0x40, 0x80, 0xff] {
            for red in (0..=255).step_by(17) {
                for green in (0..=255).step_by(17) {
                    for blue in (0..=255).step_by(17) {
                        let packed = (alpha << 24) | (red << 16) | (green << 8) | blue;
                        let got = hsva_to_packed(rgb_to_hsva(packed));
                        assert_eq!(got, packed, "0x{packed:08x} came back 0x{got:08x}");
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 4 * 16 * 16 * 16);
    }

    /// `0x789906` scales the value channel by 255 and `0x78990c` adds the 0.5 at
    /// `0x8eff98` before `_ftol` truncates, so value rounds; the alpha at
    /// `0x789919` is only scaled, so it truncates. Both are visible on a half.
    #[test]
    fn the_value_channel_rounds_and_the_alpha_truncates() {
        let packed = hsva_to_packed([0., 1., 0.5, 0.5]);
        assert_eq!(
            (packed >> 16) & 0xff,
            128,
            "0.5 * 255 + 0.5 truncates to 128"
        );
        assert_eq!(packed >> 24, 127, "0.5 * 255 truncates to 127");
    }

    /// `0x789933` skips the sector maths entirely for a zero saturation, so the
    /// hue is not even looked at.
    #[test]
    fn zero_saturation_is_grey_whatever_the_hue() {
        let packed = hsva_to_packed([0.73, 0., 0.4, 1.]);
        assert_eq!(packed & 0x00ff_ffff, 0x0066_6666, "trunc(0.4 * 255 + 0.5)");
        assert_eq!(packed >> 24, 255);
    }

    /// `0x789947` compares the hue with 1.0 and `0x789956` folds an exact 1.0 to
    /// 0.0, so the top of the circle lands in sector 0 instead of running off the
    /// end of the switch at `0x7899d1`. The witness has to be fully saturated:
    /// at a saturation of 0.5 the byte quantisation swallows the difference
    /// between sector 5's tail and sector 0's head.
    #[test]
    fn a_hue_of_exactly_one_folds_into_sector_zero() {
        let wrapped = hsva_to_packed([1., 1., 1., 1.]);
        assert_eq!(wrapped, hsva_to_packed([0., 1., 1., 1.]));
        assert_eq!(
            wrapped & 0x00ff_ffff,
            0x00ff_0000,
            "sector 0 at f = 0 is red"
        );
        assert_ne!(wrapped, hsva_to_packed([0.99, 1., 1., 1.]));
    }

    /// `0x723812` draws once for all four channels where the RGB path draws four
    /// times (`0x7238cc`, `0x7238e0`, `0x7238f4`, `0x723908`). The generator's
    /// state afterwards says which happened.
    #[test]
    fn the_hsv_path_shares_one_draw_between_its_channels() {
        let (min, max) = (0xff10_2030u32, 0x8040_5060u32);

        let mut hsv = Rng::new(5);
        random_color_hsv(min, max, &mut hsv);
        let mut one_draw = Rng::new(5);
        one_draw.next_u32();
        assert_eq!(hsv.0, one_draw.0, "one draw feeds every channel");

        let mut rgb = Rng::new(5);
        random_color_rgb(min, max, &mut rgb);
        let mut four_draws = Rng::new(5);
        for _ in 0..4 {
            four_draws.next_u32();
        }
        assert_eq!(rgb.0, four_draws.0, "and the RGB path draws per channel");
    }

    /// Between red and cyan the HSV path can only produce fully saturated
    /// colours, because saturation is `1 * u + 1 * (1 - u)` whatever the draw.
    /// The RGB path interpolates each channel on its own and spends most of its
    /// samples in the grey middle. That is what the 168 elements asking for
    /// `IsUseHSVInterp` are after, and it is the difference this port keeps.
    #[test]
    fn the_hsv_path_holds_saturation_across_a_hue_sweep() {
        let (min, max) = (0xffff_0000u32, 0xff00_ffffu32);
        let mut rng = Rng::new(3);
        let mut hsv_saturated = 0;
        let mut rgb_saturated = 0;
        for _ in 0..400 {
            if rgb_to_hsva(random_color_hsv(min, max, &mut rng))[1] > 0.99 {
                hsv_saturated += 1;
            }
            if rgb_to_hsva(random_color_rgb(min, max, &mut rng))[1] > 0.99 {
                rgb_saturated += 1;
            }
        }
        assert_eq!(hsv_saturated, 400, "a shared draw cannot desaturate");
        assert!(
            rgb_saturated < 200,
            "independent draws mostly do: {rgb_saturated}"
        );
    }

    /// `0x723829` compares the two hues and `0x72383b` takes the wrapped branch
    /// when the minimum is the larger one: a full turn is added to the maximum
    /// and taken off any result above one, so 0.9 down to 0.1 goes the short way
    /// through red instead of back through the whole spectrum.
    #[test]
    fn hue_wraps_the_short_way_when_the_minimum_is_above_the_maximum() {
        let min = hsva_to_packed([0.9, 1., 1., 1.]);
        let max = hsva_to_packed([0.1, 1., 1., 1.]);
        let mut rng = Rng::new(11);
        let mut above = 0;
        let mut below = 0;
        for _ in 0..500 {
            let hue = rgb_to_hsva(random_color_hsv(min, max, &mut rng))[0];
            assert!(
                hue >= 0.85 || hue <= 0.15,
                "hue {hue} left the short arc between 0.9 and 0.1"
            );
            above += (hue >= 0.85) as i32;
            below += (hue <= 0.15) as i32;
        }
        assert!(above > 0 && below > 0, "both sides of the wrap are used");
    }

    #[test]
    fn flipbook_cells_walk_the_authored_grid() {
        let source = header37(1)
            + &SPRITE
                .replace("TexRow: 1\r\n", "TexRow: 2\r\n")
                .replace("TexCol: 1\r\n", "TexCol: 3\r\n")
                .replace("TexInterval: 50\r\n", "TexInterval: 100\r\n");
        let mut player = build(&source);

        let cell = |player: &mut Player, dt: f32| -> [f32; 2] {
            step(player, dt);
            player.quads()[0].uv[0]
        };
        // index = floor(clock / 100) % 6, row = index / 3, col = index % 3, and
        // v grows downwards. The clocks below are cumulative.
        assert_eq!(cell(&mut player, 0.), [0., 0.], "clock 0, cell 0");
        assert_eq!(cell(&mut player, 150.), [1. / 3., 0.], "clock 150, cell 1");
        assert_eq!(cell(&mut player, 150.), [0., 0.5], "clock 300, cell 3");
        assert_eq!(cell(&mut player, 150.), [1. / 3., 0.5], "clock 450, cell 4");
        assert_eq!(
            cell(&mut player, 300.),
            [1. / 3., 0.],
            "clock 750 wraps to cell 1"
        );

        // `TexAnimMaxTime` shortens the walk: 300 ms at a 100 ms interval is
        // three cells, so the fourth is never reached.
        // The minimal sprite record carries no `TexAnimMaxTime`, so add it where
        // the retail element-100 layout puts it: after `Warp`, before `Width`.
        let limited = source.replace("Warp: 0\r\n", "Warp: 0\r\nTexAnimMaxTime: 300\r\n");
        let mut player = build(&limited);
        // limit = clamp(300 / 100, 1, 6) = 3 cells, so the walk is 0, 1, 2, 0.
        for (dt, clock, want) in [
            (0., 0., [0., 0.]),
            (150., 150., [1. / 3., 0.]),
            (100., 250., [2. / 3., 0.]),
            (100., 350., [0., 0.]),
        ] {
            step(&mut player, dt);
            assert_eq!(player.quads()[0].uv[0], want, "clock {clock}");
        }
    }

    /// `RotFromView: 1` takes the basis from the camera, `0` keeps the authored
    /// keypoint direction: 598 retail sprites against 8,271.
    #[test]
    fn rot_from_view_selects_the_camera_basis() {
        let camera = View {
            position: Vec3::new(0., 0., 10.),
            right: Vec3::Y,
            up: Vec3::Z,
            forward: Vec3::X,
        };
        for (rotate_from_view, plane_axis) in [(0, 2usize), (1, 0usize)] {
            let source = header37(1)
                + &SPRITE.replace(
                    "RotFromView: 0\r\n",
                    &format!("RotFromView: {rotate_from_view}\r\n"),
                );
            let root = gfx_file::parse(&source).unwrap();
            let mut player = Player::new(root, 7, &mut |_| Err("no children".into())).unwrap();
            player.advance(0., &camera, Mat4::IDENTITY, 1., 1.).unwrap();
            let quad = &player.quads()[0];
            // An element-space sprite spans the element's X and Y axes; a
            // camera-facing one spans the camera's right (Y) and up (Z) instead.
            let spread: Vec<f32> = (0..3)
                .map(|axis| {
                    let mut low = f32::INFINITY;
                    let mut high = f32::NEG_INFINITY;
                    for corner in &quad.positions {
                        low = low.min(corner[axis]);
                        high = high.max(corner[axis]);
                    }
                    high - low
                })
                .collect();
            let flat = plane_axis;
            assert!(
                spread[flat] < 1e-5,
                "axis {flat} should be flat: {spread:?}"
            );
            for axis in 0..3 {
                if axis != flat {
                    assert!(
                        spread[axis] > 0.5,
                        "axis {axis} should carry the quad: {spread:?}"
                    );
                }
            }
        }
    }

    /// `FadeByDist` scales the alpha between `FadeStart` and `FadeEnd`, and the
    /// client's cutoff drops the quad below 5/255.
    #[test]
    fn distance_fade_scales_alpha_and_the_cutoff_drops_the_quad() {
        let source = header37(1) + &SPRITE.replace("MOXTVersion", "MOXTVersion");
        let faded = source
            .replace("FadeByDist: 0\r\n", "FadeByDist: 1\r\n")
            .replace("FadeStart: 0.000000\r\n", "FadeStart: 5.000000\r\n")
            .replace("FadeEnd: 0.000000\r\n", "FadeEnd: 15.000000\r\n");
        let root = gfx_file::parse(&faded).unwrap();
        let mut player = Player::new(root, 7, &mut |_| Err("no children".into())).unwrap();

        // The sprite sits at (1, 2, 3) under a root scale of 2; the camera is 10
        // units away on Z, so the fade is halfway through its span.
        let near = View {
            position: Vec3::new(0., 0., 3. + 10.),
            ..view()
        };
        player.advance(0., &near, Mat4::IDENTITY, 1., 1.).unwrap();
        let distance = (near.position - Vec3::new(0., 0., 0.)).length();
        let fade = (1. - (distance - 5.) / 10.).clamp(0., 1.);
        let want = (255. * 0.5 * fade).trunc();
        assert_eq!(alpha8(&player.quads()[0]), want as u32);

        let far = View {
            position: Vec3::new(0., 0., 40.),
            ..view()
        };
        player.advance(0., &far, Mat4::IDENTITY, 1., 1.).unwrap();
        assert!(player.quads().is_empty(), "past FadeEnd nothing is drawn");
    }

    /// 82 of the corpus's 1,530 `GfxPath` references point at files that do not
    /// ship, so an unresolvable child is a named warning and the rest of the
    /// root still plays.
    #[test]
    fn a_missing_child_warns_and_its_siblings_still_play() {
        let source = header37(2) + SPRITE + CHILD;
        let root = gfx_file::parse(&source).unwrap();
        let mut asked: Vec<String> = Vec::new();
        let player = {
            let asked = &mut asked;
            Player::new(root, 7, &mut |path| {
                asked.push(path.to_string());
                Err("absent from the archive".into())
            })
            .unwrap()
        };
        assert_eq!(asked, vec!["gfx/npc/女npc_蝴蝶子效果.gfx".to_string()]);
        assert_eq!(player.warnings().len(), 1);
        assert!(
            player.warnings()[0].contains("女npc"),
            "{}",
            player.warnings()[0]
        );
        assert_eq!(player.stats().children, 0);

        let mut player = player;
        step(&mut player, 16.);
        assert_eq!(player.quads().len(), 1, "the sprite next to it still draws");
    }

    /// A nested root that resolves is played as a child, with the parent's
    /// placement, scale and alpha folded in.
    #[test]
    fn a_resolved_child_plays_inside_its_parent() {
        let source = header37(1) + CHILD;
        let child_source = header37(1) + SPRITE;
        let root = gfx_file::parse(&source).unwrap();
        let mut player = Player::new(root, 7, &mut |path| {
            assert!(path.ends_with(".gfx"), "{path}");
            gfx_file::parse(&child_source)
        })
        .unwrap();
        assert!(player.warnings().is_empty());
        step(&mut player, 16.);
        assert_eq!(player.quads().len(), 1, "the child's sprite draws");
        assert_eq!(player.stats().children, 1);
    }

    /// A child that names its own parent must not recurse: the depth cap and the
    /// path stack turn it into a warning.
    #[test]
    fn a_self_referencing_child_warns_without_hanging() {
        let source = header37(1) + CHILD;
        let root = gfx_file::parse(&source).unwrap();
        let mut player = Player::new(root, 7, &mut |_| gfx_file::parse(&source)).unwrap();
        assert!(
            !player.warnings().is_empty(),
            "the cycle is reported, not followed"
        );
        assert!(
            player
                .warnings()
                .iter()
                .any(|warning| warning.contains("references itself")),
            "{:?}",
            player.warnings()
        );
        step(&mut player, 16.);
        assert!(player.quads().is_empty());
    }

    /// The child element from `gfx_file`'s own fixture, with the forever
    /// keypoint every one of the 1,530 retail element-200 records carries: the
    /// keypoint is what places the nested root.
    const CHILD: &str = "GFXELEMENTID: 200\r\nName: ch\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
        GfxPath: NPC\\女npc_蝴蝶子效果.gfx\r\nOutColor: 1\r\nLoopFlag: 0\r\nPlaySpeed: 1.000000\r\n\
        StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
        Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\nRad_2D: 0\r\n\
        CtrlMethodCount: 0\r\n";
}
