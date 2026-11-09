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
//! `IsSurface` does not mean one thing. Ids 123 (`0x727f00`, an ellipsoid) and
//! 124 (`0x729be0`, a cylinder) sample the emitter's own `AreaSize` volume, while
//! id 120 (`0x7272aa`) samples no volume at all: it spreads the direction over
//! the surface of a cone and leaves the particle one unit from the emitter. All
//! three are reproduced, and so is id 121's `0x727800`, which picks one of the
//! box's three face pairs with probability proportional to its area and samples
//! that face. Ids 122 and 125 never read the flag.

use crate::{
    ecmodel,
    gfx_color_track::ColorTrack,
    gfx_file::{
        Child, Common, Control, ControlData, Curve, Cylinder, Element, Grid, KeyPoint, Lightning,
        Particle, Root, Spec, Sprite, Trail,
    },
};
use glam::{Mat4, Quat, Vec3, Vec4};

/// Nesting limit for element-200 children. The client resolves child paths at
/// load time and has no cycle guard, so a self-referencing root would recurse
/// until the stack goes.
pub const MAX_CHILD_DEPTH: usize = 6;

/// Per-emitter particle ceiling. `Quota` is authoritative when smaller; this
/// only bounds a malformed value.
/// Texture name carried by a model mesh that authors no map.
///
/// It is not a path: the effect pass cannot read it, so it falls back to its
/// flat white stand-in and the geometry still draws with the element's colour.
pub const UNTEXTURED_MESH: &str = "<untextured>";

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
    /// The element's authored `Priority` (1..5 on every corpus element). It
    /// orders blended geometry *within* a layer; the client authors it on every
    /// element and nothing read it, so a low-priority dust sheet and a
    /// high-priority flash landed in whatever order the texture bucket
    /// happened to produce.
    pub priority: i32,
    /// World-space corners in the client's quad order: top-left, top-right,
    /// bottom-left, bottom-right.
    pub positions: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// Linear RGB and alpha, already folded with the root and placement alpha.
    pub color: [f32; 4],
    /// Set only for model elements: the mesh is a triangle list rather than the
    /// four corners above, and `positions`/`uv` are left zero.
    pub mesh: Option<EffectMesh>,
}

/// One drawable piece of a model element, posed into world space.
///
/// Element 160 names `.SMD`/`.SKI` models from the effect archive. Those are
/// rigid — 0 or 1 bones per shipped skin — so the bind pose is the only pose
/// they need, and the result is a triangle list with per-vertex UV. It does not
/// fit `EffectQuad`'s four corners, so it travels beside them and the GPU pass
/// draws it with a triangle-list pipeline instead of the quad one.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectMesh {
    /// Archive path of the diffuse texture, same normalization as a quad's.
    pub texture: String,
    /// World-space triangle soup: three consecutive vertices per triangle.
    pub vertices: Vec<EffectVertex>,
}

/// A posed model vertex: position in world space and its authored UV.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
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
    /// matrix, which is all a renderer normally has on hand.
    ///
    /// This used to read the eye off the inverse's fourth column and the axes off
    /// its first three. Measured against this viewer's own projection that is
    /// wrong twice over, and both mistakes are invisible until an effect has to
    /// face the camera. `Mat4::transform_point3` assumes w = 1 and drops the
    /// perspective divide, so feeding it the NDC origin returns the **near
    /// plane** point: 3,911 units from the eye for a camera looking across the
    /// world, and worse the further it sits from the origin. The inverse's
    /// columns are not the camera basis either; the clip-space mirror this
    /// viewer applies (`from_scale(-1, 1, 1)`, the handedness flip) lives in the
    /// projection, so the columns come back tilted. Measured, the recovered
    /// forward was 0.91 away from the true view direction and the right and up
    /// axes were each off by tens of degrees.
    ///
    /// The basis is taken from points instead, so a mirrored, orthographic or
    /// skewed projection is handled by the same arithmetic. Two points on the
    /// centre ray give the view direction, two more give the screen axes, and
    /// the eye is where that ray crosses the plane `w_clip = 0` (row 3 of the
    /// matrix), which is the one point the inverse's own w cannot represent.
    /// Every value is checked for a finite, non-degenerate result and the
    /// default basis is returned rather than a NaN camera.
    pub fn from_inverse_view_projection(inverse: Mat4) -> Self {
        if !inverse.is_finite() {
            return Self::default();
        }
        let Some(near) = Self::unproject(inverse, 0., 0., 0.) else {
            return Self::default();
        };
        let Some(far) = Self::unproject(inverse, 0., 0., 1.) else {
            return Self::default();
        };
        // The centre ray, from the near plane towards the far plane. In this
        // right-handed projection that direction points back at the eye, so the
        // view direction is the other way round.
        let ray = near - far;
        if ray.length_squared() < 1e-12 {
            return Self::default();
        }
        let forward = ray.normalize();
        let Some(right) = Self::unproject(inverse, 1., 0., 1.) else {
            return Self::default();
        };
        let Some(up) = Self::unproject(inverse, 0., 1., 1.) else {
            return Self::default();
        };
        let right = (right - far).normalize_or_zero();
        let up = (up - far).normalize_or_zero();
        if right == Vec3::ZERO || up == Vec3::ZERO || right.dot(up).abs() > 0.01 {
            return Self::default();
        }
        // The eye is on the centre ray where the homogeneous w runs out:
        // `inverse` maps the plane w_clip = 0 to infinity, and that plane is row
        // 3 of the matrix this one inverted.
        let a = inverse.inverse().row(3);
        let plane = a.truncate();
        let denominator = plane.dot(forward);
        if !denominator.is_finite() || denominator.abs() < 1e-9 {
            return Self::default();
        }
        let position = near + forward * (-(plane.dot(near) + a.w) / denominator);
        if !position.is_finite() {
            return Self::default();
        }
        Self {
            position,
            right,
            up,
            forward,
        }
    }

    /// [`Self::from_inverse_view_projection`], but saying why it gave up.
    ///
    /// The fallible version returns `Self::default()`, which puts the eye at the
    /// world origin. That is safe for a caller that only needs a basis, and
    /// dangerous for a caller that reports how many effects are playing: a
    /// degenerate frame made every billboard face (0,0,0) and the caller had no
    /// way to tell that from "the map has no effects near you". The world pass
    /// uses this one so the reason reaches its status line.
    pub fn try_from_inverse_view_projection(inverse: Mat4) -> Result<Self, String> {
        if !inverse.is_finite() {
            return Err("effect camera matrix is not finite".into());
        }
        let near = Self::unproject(inverse, 0., 0., 0.)
            .ok_or("effect camera: near-plane ray has no finite w")?;
        let far = Self::unproject(inverse, 0., 0., 1.)
            .ok_or("effect camera: far-plane ray has no finite w")?;
        let ray = near - far;
        if ray.length_squared() < 1e-12 {
            return Err("effect camera: near and far rays coincide".into());
        }
        let forward = ray.normalize();
        let right = Self::unproject(inverse, 1., 0., 1.)
            .ok_or("effect camera: right ray has no finite w")?;
        let up =
            Self::unproject(inverse, 0., 1., 1.).ok_or("effect camera: up ray has no finite w")?;
        let right = (right - far).normalize_or_zero();
        let up = (up - far).normalize_or_zero();
        if right == Vec3::ZERO || up == Vec3::ZERO || right.dot(up).abs() > 0.01 {
            return Err("effect camera: axes are degenerate or not perpendicular".into());
        }
        let a = inverse.inverse().row(3);
        let plane = a.truncate();
        let denominator = plane.dot(forward);
        if !denominator.is_finite() || denominator.abs() < 1e-9 {
            return Err("effect camera: eye ray runs parallel to the w=0 plane".into());
        }
        let position = near + forward * (-(plane.dot(near) + a.w) / denominator);
        if !position.is_finite() {
            return Err("effect camera: eye position is not finite".into());
        }
        Ok(Self {
            position,
            right,
            up,
            forward,
        })
    }

    /// Inverse-project one NDC point and divide through by w, which is the step
    /// `Mat4::transform_point3` leaves out.
    fn unproject(inverse: Mat4, x: f32, y: f32, z: f32) -> Option<Vec3> {
        let homogeneous = inverse * Vec4::new(x, y, z, 1.);
        let w = homogeneous.w;
        (w.is_finite() && w.abs() > 1e-9).then(|| homogeneous.truncate() / w)
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
    /// Elements that declare `DummyEle`: helpers the client never submits.
    pub dummies: usize,
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
        self.dummies += other.dummies;
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
    /// Where this particle was born, before any controller moved it. Controller
    /// 107's noise is a *displacement of the birth point*, not a per-frame step:
    /// adding it every frame compounds, and `夜空飘尘`'s `Amplitude: 400`
    /// walked its 0.4-unit particle out to +/-7900 units in forty frames.
    origin: Vec3,
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
    /// Element 152 only: the vertex ring the trail keeps. Element 150 leaves it
    /// empty and draws its whole authored strip every frame.
    trail: TrailRing,
}

/// The trail's vertex ring, element+0x31c..+0x328. The constructor at `0x713820`
/// allocates `$0xc00` bytes — 256 records of twelve — initialises every one to
/// `{-1, 0, 0}` at `0x71384b`, so an unused slot is indexed -1, and starts the
/// head at 255 (`0x713867`). Records are laid down in index order and the live
/// window shrinks from the old end, so `records` here is oldest first.
#[derive(Clone, Debug, Default)]
struct TrailRing {
    /// element+0x314: milliseconds accumulated since the last vertex was laid.
    accumulator: f32,
    /// element+0x318: how many vertices have been laid down in total.
    laid: usize,
    /// element+0x328 worth of live records, oldest first.
    records: Vec<TrailVertex>,
}

/// The ring's capacity, element+0x31c: `movl $0x100,0x31c(%esi)` at `0x71382d`.
/// `0x713f23` refuses to count past it, so a trail authored with more segments
/// than that keeps only the newest 256.
const TRAIL_RING_CAPACITY: usize = 256;

/// One ring record: the twelve bytes `0x7140e8` writes are `{index, life, age}`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TrailVertex {
    /// The vertex's position in the authored strip, so a truncated ring still
    /// knows which part of the strip it covers.
    index: u32,
    /// element+0x30c at birth: the authored `vertslife`, in milliseconds.
    life: f32,
    /// Milliseconds since this vertex was laid down, clamped at `life`.
    age: f32,
}

impl TrailRing {
    /// `0x713ed4`: fill the ring with the whole authored strip at once, every
    /// record born this frame with age 0. The native only does this when the
    /// count does not already match, so a full ring is left alone.
    fn fill(&mut self, segments: usize, life: f32) {
        if self.records.len() == segments {
            return;
        }
        self.records.clear();
        self.accumulator = 0.;
        self.laid = segments;
        for index in 0..segments {
            self.push(TrailVertex {
                index: index as u32,
                life,
                age: 0.,
            });
        }
    }

    /// `0x7140a5`: push a record at the head. The count stops growing at the
    /// capacity (`0x713fd1`) while the head keeps advancing, so the oldest
    /// record is the one that gives way.
    fn push(&mut self, vertex: TrailVertex) {
        if self.records.len() >= TRAIL_RING_CAPACITY {
            self.records.remove(0);
        }
        self.records.push(vertex);
    }

    /// `0x713f60`: age every record by the frame delta. A record already at its
    /// life is left alone (`0x713fa5`) and the sum is clamped back to the life
    /// (`0x713fb0`), so an expired vertex stays expired instead of overflowing.
    fn age(&mut self, dt_ms: f32) {
        for record in &mut self.records {
            if record.age == record.life {
                continue;
            }
            record.age += dt_ms;
            if record.age > record.life {
                record.age = record.life;
            }
        }
    }

    /// `0x713fbe`, reached only when `istaildisappear` is set: scan from the
    /// newest record backwards and, at the first one whose age has reached its
    /// life, shrink the count by `position + 1` (`0x71400f` does
    /// `count += -1 - position`). That drops the expired record and every older
    /// one; the head does not move, so the live window simply gets shorter.
    fn drop_expired(&mut self) {
        for position in (0..self.records.len()).rev() {
            if self.records[position].age == self.records[position].life {
                // `position + 1` records go: the expired one and everything older
                // than it. Ages grow towards the old end, so the scan from the
                // newest stops at the newest expired record, and in an
                // oldest-first list "older" is the front.
                self.records.drain(..=position);
                return;
            }
        }
    }
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
    /// The sibling this element's `BindEle` resolved to, as a slot in
    /// `Player::elements`. The client resolves every name once at container
    /// level (`0x6e42f0`) through a hash map of element names held at
    /// container+0x1c and stores the result on the element at +0xa0 with the
    /// setter `0x74c2a0`; a name that resolves to nothing leaves the slot null
    /// with no diagnostic. `None` therefore covers both "unbound" and "bound to
    /// a name this root does not contain", which is what the client does too.
    bind_target: Option<usize>,
    /// This element's world placement at its most recent update. A dependent
    /// reads it the way `0x710121` reads the target's matrix at +0x58: one
    /// level, never recursively, so the mutual binds the corpus really contains
    /// (101 to 110 and 110 to 101) cannot diverge. `None` until the target has
    /// been placed once, so a dependent listed before its target falls back to
    /// the root basis for that frame instead of snapping to the origin.
    placement: Option<Mat4>,
    /// True once this element's own clock has run out. `0x6fa69a` makes a bound
    /// element's tick return without updating when the target's handle at +0x50
    /// is negative, so a dependent stops with its target rather than playing on
    /// alone.
    ended: bool,
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
    /// The rotation `placement` adds on top of the parent basis: the
    /// interpolated keypoint `Direction` with ctrl 100's orbit spun about Z.
    /// The client's emission direction is expressed in the emitter's own frame,
    /// so undoing this is what turns one of its directions into a local one.
    spin: Quat,
    /// The interpolated keypoint `Direction` without the orbit. An unbound
    /// emitter's orientation quaternion at +0x74 is `Q * keypoint Direction`
    /// (`0x71a4dc`), and `Q` is what the parent basis already carries, so this is
    /// the half of that product the local frame has not accounted for.
    keypoint_direction: Quat,
    view: &'a View,
    common: &'a Common,
    version: u32,
    /// Sampled keypoint colour after the controllers.
    color: u32,
    /// The root's authored `Vec:` pair, which element 190 uses as the two
    /// endpoints of its span instead of the element's own position.
    span: [Vec3; 2],
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
    /// The owner's placement scale, as handed to [`Player::advance_with_terrain`].
    /// A 100/101 sprite is a screen-space card whose size comes from
    /// `m_pGfx->GetActualScale()` in `A3DDecalEx::Fill_Verts_2D`, i.e. the GFX's
    /// own `DedaultScale`; the owner's map scale reaches the card only through
    /// `GetParentTM() * kp.m_vPos`, which moves its centre. Dividing it back out
    /// of the extents is what keeps `场景/树林光束.gfx` at its authored 14 units
    /// under the 8.776 this map authors, while `能量喷泉`'s particles, whose
    /// geometry is built in the parent frame, still follow the map scale.
    owner_scale: f32,
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

/// One ground query, the shape the engine's raycast returns. The owner runs it
/// once per frame at `0x6e53f0`: `0x41a430(&pos, owner+0x194)` returns a float
/// height that is stored at owner+0x1a0 (and copied to owner+0x170 at
/// `0x6e55a4`) and writes the surface normal to owner+0x194, which is what
/// `0x6e5b90` then turns into an orientation. `walker::physics::ground` already
/// produces exactly this pair, so the viewer takes it as an argument rather
/// than owning terrain it does not have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundHit {
    /// Ground height under the query position, in world units.
    pub height: f32,
    /// Surface normal there. The client normalises it before use.
    pub normal: Vec3,
}

/// How close the root has to be to the ground for `MatchGrnd` on its own to
/// take effect. `0x6e54a1` subtracts the raycast height from the owner's y,
/// `0x6e54ac` compares the difference against the float at `0x8eb828` and
/// `0x6e54bb`/`0x6e54c4` store the result as the byte at owner+0x284, which
/// `0x6fa7ee` and `0x6fa831` then read.
const GROUND_NEAR_DISTANCE: f32 = 0.2;

/// The surface height under a world X/Z, or `None` where the ray misses.
///
/// `A3DDecalEx::Fill_Verts_3D` asks this once per vertex of a ground sheet
/// (`AfxGetGrndNorm` with a NULL normal, `A3DDecalEx.cpp:517`), which is why a
/// ground decal can follow a crater floor instead of hovering over it. The
/// viewer is handed the query rather than owning terrain it does not have.
pub type GroundHeightQuery<'a> = &'a dyn Fn(f32, f32) -> Option<f32>;

/// The owner's terrain, threaded down to the sprite path so a `MatchGrnd` decal
/// can be draped over the surface the way `A3DDecalEx::Fill_Verts_3D` drapes it.
#[derive(Clone, Copy)]
pub struct GroundSampler<'a> {
    /// Height of the surface under any world X/Z.
    pub height: GroundHeightQuery<'a>,
    /// The client's `IsCloseToGrnd()` byte (owner+0x284). The ground branch
    /// requires it in addition to the element's own `MatchGrnd`, and it is also
    /// what `GetParentTM` reads, so the draped sheet and the element's frame
    /// agree about whether the owner is lying on the ground.
    pub near_ground: bool,
}

/// How wide one cell of a draped ground sheet is before the sheet is cut up
/// (`_grnd_grid`, `A3DDecalEx.cpp:29`).
const GROUND_GRID_CELL: f32 = 2.;
/// The most cells a draped sheet is cut into along either axis
/// (`_max_row_or_col`, `A3DDecalEx.cpp:30`), so one sheet is at most sixteen
/// quads — the 906 corpus elements that set `MatchGrnd` on a sprite split
/// 536 at one cell and 370 at two to sixteen.
const GROUND_MAX_ROW_OR_COL: usize = 4;
/// The height the ground branch adds over the surface — `vert.y =
/// AfxGetGrndNorm(...) + .2f` (`A3DDecalEx.cpp:517`). It is the same literal as
/// `GROUND_NEAR_DISTANCE` because both read the float at `0x8eb828`.
const GROUND_DRAPE_LIFT: f32 = 0.2;

/// The steepest surface the client will align an effect to. `0x6e5bb9` takes
/// the absolute dot of the normal with world up and `0x6e5bc1` rejects it below
/// the float at `0x9bff88`, which `0x6e5b80` computes once at startup as `fcos`
/// of the double at `0x8d5990` — exactly 5.0 degrees. A wall or a steep slope
/// therefore leaves the effect on its own orientation rather than tilting it.
const GROUND_MAX_TILT_COSINE: f32 = 0.996_194_7;

/// The rotation that takes world up onto `normal`, instruction for instruction
/// from `0x6e5b90`: the axis is `cross(up, normal)` divided by its own length
/// (`fdivrs` against 1.0 at `0x8d5c28`, `0x6e5c5e`), the angle is
/// `acos(dot(up, normal))` through `_CIacos` at `0x892f06`, and `0x7b6fd0`
/// turns the pair into a quaternion. The up vector is not an assumption —
/// `0x6ea170` initialises the global at `0x9c0348` to (0.0, 1.0, 0.0).
///
/// `None` is the client's own rejection for a surface tilted more than five
/// degrees, plus the single case it does not guard: when the normal points
/// straight down the cross product vanishes and `0x6e5c5e` divides by zero, so
/// there is no native result to reproduce and the element keeps the orientation
/// it already had, which is what `0x6e54d8` leaves in owner+0x1b4 when
/// `0x6e5b90` returns false.
fn ground_alignment(normal: Vec3) -> Option<Quat> {
    let dot = Vec3::Y.dot(normal);
    if !dot.is_finite() || dot.abs() < GROUND_MAX_TILT_COSINE {
        return None;
    }
    let axis = Vec3::Y.cross(normal);
    let length = axis.length();
    if !(length > 1e-6) {
        return (dot > 0.).then_some(Quat::IDENTITY);
    }
    Some(Quat::from_axis_angle(
        axis / length,
        dot.clamp(-1., 1.).acos(),
    ))
}

/// The owner-level frames `0x6e53f0` rebuilds every frame so that `0x6fa7c0`
/// can hand one of them to an element.
#[derive(Clone, Copy, Debug)]
struct GroundFrames {
    /// owner+0x284: the root is within [`GROUND_NEAR_DISTANCE`] of the ground.
    near_ground: bool,
    /// owner+0xfc: the owner's orientation composed with the ground alignment —
    /// owner+0x1b4, built by the Hamilton product at `0x6e57e0` — placed at
    /// ground height. `0x6e5540`..`0x6e556e` forms that translation from pos.x,
    /// the raycast height at owner+0x1a0 and pos.z.
    aligned: Mat4,
    /// owner+0x13c: the owner's own orientation at ground height, the copy of
    /// the base matrix that `0x6e5592` re-transforms with the ground height.
    height_only: Mat4,
    /// The frame element+0x150 `GrndNormOnly` asks for: the ground *normal*
    /// without the snapped height, so the effect lies on the slope's plane but
    /// stays at its own altitude rather than sliding down to the surface. It is
    /// the aligned orientation of `aligned` carried at the owner's own position
    /// instead of the raycast height — the third of the three combinations the
    /// two vectors at owner+0x1b4 and owner+0x1a0 make available, and the one
    /// neither of the other two frames expresses.
    normal_only: Mat4,
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
    /// The camera shake's state and the offset it produced on the last frame.
    shake: ShakeState,
    shake_offset: Vec3,
    /// Seed of the shake's noise lattice. The client's noise object builds its
    /// gradient table once, at construction (`0x7662b0` from the controller's
    /// constructor `0x6f10f0`), and the driver only samples it, so the lattice is
    /// fixed for the life of the effect rather than redrawn per frame the way
    /// element 150's is.
    shake_seed: u32,
    textures: Vec<String>,
    /// Elements that could not be resolved, most often a nested `GfxPath` the
    /// archive does not carry. 82 of the corpus's 1,530 child references (31
    /// distinct paths, for example `人物\技能\巫师\技能\…` with a doubled
    /// directory and `4品法宝_冷凝术_ 子效果.gfx` with a stray space) point at
    /// files that do not ship, so a missing child is a named warning and the rest
    /// of the root still plays rather than the whole effect being dropped.
    warnings: Vec<String>,
    /// Effect-archive models named by element 160, already posed and turned into
    /// triangle lists. Empty when the host supplied no model reader.
    models: Vec<Vec<EffectMesh>>,
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

    /// The posed meshes of this root's model elements, in element order.
    pub fn models(&self) -> &[Vec<EffectMesh>] {
        &self.models
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
                    trail: TrailRing::default(),
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
                            // The parent plays without it. 82 of the corpus's
                            // 1,530 `GfxPath` references point at files that were
                            // never packed — `gfx/人物/技能/巫师/技能抑制_闪电.gfx`
                            // asks for a `牧师` effect the archive has no directory
                            // for — and a root is not broken because the archive
                            // lacks a child, so this stays a warning and the
                            // element keeps its clock. The tail says what the
                            // viewer does next, since the raw reader error on its
                            // own reads like a fault in this program.
                            warnings.push(format!(
                                "element {index} ({}): {error}; the root plays without this child",
                                child.path
                            ));
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
                bind_target: None,
                placement: None,
                ended: false,
            });
        }
        // Resolve `BindEle` after every element exists, since a name may point
        // forward. Element names are unique within a root in all 5,459 corpus
        // files, so the first match is the only match and a linear scan
        // reproduces the client's bucket-chain lookup without a hash map.
        for slot in 0..elements.len() {
            let name = &root.elements[slot].common.bind_element;
            if name.is_empty() {
                continue;
            }
            elements[slot].bind_target = root
                .elements
                .iter()
                .position(|candidate| &candidate.common.name == name);
        }
        Ok(Self {
            root,
            time: 0.,
            finished: false,
            elements,
            quads: Vec::new(),
            stats: Stats::default(),
            rng,
            shake: ShakeState::default(),
            shake_offset: Vec3::ZERO,
            // "SHAK": a lattice of its own, so the shake's noise is not the same
            // sequence the elements draw from.
            shake_seed: seed.wrapping_add(0x5348_414b),
            warnings,
            textures,
            models: Vec::new(),
        })
    }

    /// Resolve every element-160 model in this root and its already-loaded
    /// children into posed triangle lists, once, at load time.
    ///
    /// The effect archive keeps models in the same package as the roots, and a
    /// model element names its `.SMD` relative to the GFX file's own folder
    /// (`gfx/models/` prefixed, exactly as the client composes it). The `.SMD`
    /// then names its `.BON` skeleton and `.SKI` skins relative to *its own*
    /// directory, which is why the reader is called with those names joined to
    /// the SMD's parent rather than the archive root.
    ///
    /// These models are static: measured across the 141 shipped `.SKI` files,
    /// every skin is bound by 0 or 1 bones, so the bind pose is the only pose
    /// the archive can express. A model whose skeleton carries a scale or
    /// constraint controller is refused by `skeleton_pose` rather than drawn in
    /// the wrong pose, and the refusal is reported once through `warnings`.
    pub fn load_models(
        &mut self,
        root_path: &str,
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        let mut models = std::mem::take(&mut self.models);
        // Every effect model in `gfx.pck` lives under one `gfx/models/` root, so
        // the parent is the same string for every root in the archive. The
        // argument is kept because a host may pass an already-extracted member
        // name, and the join below tolerates a path that already starts there.
        let _ = root_path;
        let parent = "gfx/models";
        self.collect_models(&mut models, &parent, read, 0);
        self.models = models;
        Ok(())
    }

    fn collect_models(
        &mut self,
        out: &mut Vec<Vec<EffectMesh>>,
        parent: &str,
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
        depth: usize,
    ) {
        for (index, element) in self.root.elements.iter().enumerate() {
            let Spec::Model(model) = &element.spec else {
                continue;
            };
            let fallback = match texture_path(&element.common.texture) {
                Ok(path) => path,
                Err(_) => String::new(),
            };
            out.push(
                match posed_model_meshes(&model.path, parent, &fallback, read) {
                    Ok(meshes) => meshes,
                    Err(error) => {
                        let line = format!("element {index} model {}: {error}", model.path);
                        if !self.warnings.contains(&line) {
                            self.warnings.push(line);
                        }
                        Vec::new()
                    }
                },
            );
        }
        // Children were resolved at construction; walk their elements too so a
        // nested root's models are not silently dropped.
        if depth >= MAX_CHILD_DEPTH {
            return;
        }
        for element in &mut self.elements {
            if let Runtime::Child(child) = &mut element.runtime {
                child.player.collect_models(out, parent, read, depth + 1);
            }
        }
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

    /// The camera offset this root's `ShakeCam` asks for on the frame `advance`
    /// just produced, in the placement's own world units. The client keeps it at
    /// root+0x1fc..+0x204 and hands it to the camera itself — `0x41a4e0` applies
    /// it, `0x41a480` saves the untouched camera once on the first frame of the
    /// window and the exit path `0x6e5150` restores it — so a viewer that owns no
    /// camera returns the offset and leaves the applying to its caller.
    /// `Vec3::ZERO` covers every frame the client would not shake: `ShakeCam`
    /// clear, no controller, outside the authored window, or the camera at or
    /// beyond 30 units from the effect.
    pub fn camera_shake(&self) -> Vec3 {
        self.shake_offset
    }

    /// True once a play-once root has run past its last keypoint.
    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn time_ms(&self) -> f32 {
        self.time
    }

    /// The clock of the first nested root, for the child-timing audits. A root
    /// with no child element returns `None`, so a probe cannot silently read the
    /// parent's clock and mistake it for the child's.
    pub fn child_time_ms(&self) -> Option<f32> {
        self.elements
            .iter()
            .find_map(|element| match &element.runtime {
                Runtime::Child(child) => Some(child.player.time_ms()),
                _ => None,
            })
    }

    /// Restart the clock, which is what the client does when a looping root runs
    /// out and starts again. Everything an element has accumulated survives —
    /// live particles, ribbon history, the noise a fractal strip is half way
    /// through — except the state `ResetLoopEnd` clears; see `reset_at_loop_end`,
    /// which also records what is still unresolved about the three other classes
    /// that override the same client hook.
    pub fn reset(&mut self) {
        self.time = 0.;
        self.finished = false;
        for element in &mut self.elements {
            element.seeded = -1;
            element.state = CtrlState::new();
            // A restart gives every element its clock back, so the binding
            // dependency has to be re-armed as well: a dependent whose target
            // ended during the previous loop would otherwise stay ended for
            // good. The resolution itself is structural and survives.
            element.ended = false;
            element.placement = None;
            if self.root.elements[element.index].common.reset_loop_end {
                reset_at_loop_end(&mut element.runtime);
            }
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
        self.advance_with_ground(dt_ms, view, transform, scale, alpha, None)
    }

    /// Advance one frame with the ground under the root already sampled.
    ///
    /// The owner runs the raycast itself (`0x6e53f0` calls `0x41a430` with the
    /// owner's own position) and the effect only consumes the height and the
    /// normal, so the viewer takes them as an argument instead of owning terrain
    /// it does not have. `None` stands for an owner whose byte at +0x1f4 is
    /// clear, and every ground path is gated on that byte — `0x6e540f` skips the
    /// whole pass, `0x6fa7c5` and `0x6fa813` both fall through to the un-snapped
    /// frame — so `MatchGrnd` and `GroundHeight` stay inert exactly as they are
    /// in the client for an owner that cannot raycast.
    ///
    /// This is [`Self::advance_with_terrain`] without a surface query, which is the
    /// signature add-on modules already compile against.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_ground(
        &mut self,
        dt_ms: f32,
        view: &View,
        transform: Mat4,
        scale: f32,
        alpha: f32,
        ground: Option<GroundHit>,
    ) -> Result<(), String> {
        self.advance_with_terrain(dt_ms, view, transform, scale, alpha, ground, None)
    }

    /// The same frame with both kinds of ground data: the single sample the
    /// element frames are built from, and the surface under the sheet.
    ///
    /// `terrain` is what the ground *drape* needs. The client's owner raycasts
    /// once, at its own position, and consumes that pair for the element
    /// frames, but `A3DDecalEx::Fill_Verts_3D` then asks the surface under every
    /// vertex of a ground sheet for itself; one sample cannot answer that, so a
    /// host that can answer it passes the query and the sprite path uses it. A
    /// caller with terrain and no `GroundHit` still gets the drape — the
    /// near-ground byte it gates on is measured from the query — and a caller
    /// with neither gets exactly what it got before.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_terrain(
        &mut self,
        dt_ms: f32,
        view: &View,
        transform: Mat4,
        scale: f32,
        alpha: f32,
        ground: Option<GroundHit>,
        // The surface under the owner, for the ground-draped decal path. It is
        // deliberately separate from `ground`: the drape needs a height at
        // every vertex of the sheet, while `GroundHit` is the single sample the
        // owner's own frame is built from, and a caller can have one without
        // the other.
        terrain: Option<GroundHeightQuery<'_>>,
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
        // The owner's ground pass runs once per frame, before any element asks
        // for a frame (`0x6e53f0` is the owner's tick, `0x6fa7c0` is called from
        // the corner builder later in the same frame).
        let frames = ground.and_then(|hit| self.ground_frames(hit, transform, base));
        // `A3DDecalEx::Fill_Verts_3D` takes its ground branch only for an owner
        // that was placed with ground data and sits within
        // `GROUND_NEAR_DISTANCE` of the surface (`m_pGfx->IsCloseToGrnd() &&
        // m_pGfx->MatchGrnd()`). The client measures that at `0x6e54a1` as the
        // owner's y minus the raycast height, signed, so an owner below the
        // surface counts as near it too; the sampler answers the same question
        // here, since it is the only ground data this path is given.
        let drape = terrain.map(|height| {
            let origin = transform.w_axis.truncate();
            GroundSampler {
                height,
                near_ground: matches!(
                    height(origin.x, origin.z),
                    Some(surface) if origin.y - surface < GROUND_NEAR_DISTANCE
                ),
            }
        });
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
        for slot in 0..self.elements.len() {
            let index = self.elements[slot].index;
            // The element's own frame is the one `0x6fa7c0` selects from the
            // ground state; a bind target then substitutes its placement, which
            // is what `0x710121` does with the target's matrix.
            let own = self.ground_parent(frames.as_ref(), &self.root.elements[index], base);
            // A bound element takes its parent basis from its target's last
            // placement. `0x710121` multiplies the element's own basis by the
            // matrix `0x6fe120` copies out of the target, which is the same
            // substitution with the target standing in for the root.
            let (parent, target_ended) = match self.elements[slot].bind_target {
                Some(target) => (
                    self.elements[target].placement.unwrap_or(own),
                    self.elements[target].ended,
                ),
                None => (own, false),
            };
            if target_ended {
                // `0x6fa69a`: the tick returns without updating. The target's
                // clock cannot restart, so neither can this element's, and it
                // counts as done for the root's own finish test.
                self.elements[slot].ended = true;
                continue;
            }
            let element = &self.root.elements[index];
            // Models are stored in element order, skipping the elements that are
            // not models, so find this element's turn by counting earlier models.
            let model_slot = self.root.elements[..index]
                .iter()
                .filter(|element| matches!(element.spec, Spec::Model(_)))
                .count();
            let element_meshes: Vec<EffectMesh> = match self.root.elements[index].spec {
                Spec::Model(_) => self.models.get(model_slot).cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            let runtime = &mut self.elements[slot];
            let done = advance_element(
                element,
                runtime,
                time,
                dt_s,
                play_speed,
                version,
                face_to_viewer,
                view,
                parent,
                color_scale,
                scale,
                [self.root.aabb_center, self.root.aabb_half],
                &mut rng,
                &mut stats,
                &mut out,
                &element_meshes,
                drape,
            )?;
            runtime.ended = done;
            finished = finished && done;
        }
        // The camera shake is the root's own per-frame work, driven by the root's
        // millisecond tick rather than any element's, which is why it sits after
        // the element loop: `0x6e4b7d` is called once per root update and reads
        // root+0x1cc directly.
        let distance = (view.position - base.transform_point3(Vec3::ZERO)).length();
        self.shake_offset = camera_shake_frame(
            &self.root,
            &mut self.shake,
            &mut rng,
            self.shake_seed,
            self.time,
            distance,
        );
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

    /// The owner's per-frame ground pass, `0x6e53f0`. One query result becomes
    /// the near-ground byte at owner+0x284 and the two frames `0x6fa7c0` chooses
    /// between. The frames carry the same scale `base` does, so matching the
    /// ground can move and tilt an effect but never resize it.
    fn ground_frames(&self, hit: GroundHit, transform: Mat4, base: Mat4) -> Option<GroundFrames> {
        if !hit.height.is_finite() || !hit.normal.is_finite() {
            return None;
        }
        let origin = transform.w_axis.truncate();
        // 0x6e54a1/0x6e54ac: the owner's y against the raycast height, with the
        // 0.2 at 0x8eb828.
        let near_ground = origin.y - hit.height < GROUND_NEAR_DISTANCE;
        let own = Quat::from_mat4(&normalize_basis(transform));
        let scale = Mat4::from_scale(Vec3::splat(base.x_axis.length()));
        // 0x6e5540..0x6e556e: the ground frame's translation is (pos.x, the
        // raycast height, pos.z).
        let at_ground = Vec3::new(origin.x, hit.height, origin.z);
        // 0x6e54d8 copies owner+0x1a4 into owner+0x1b4 before the query, and
        // 0x6e5506 skips the product when 0x6e5b90 rejects the normal, so a
        // rejected surface leaves the owner's own orientation in place.
        let aligned =
            own * ground_alignment(hit.normal.normalize_or_zero()).unwrap_or(Quat::IDENTITY);
        Some(GroundFrames {
            near_ground,
            aligned: Mat4::from_rotation_translation(aligned, at_ground) * scale,
            height_only: Mat4::from_rotation_translation(own, at_ground) * scale,
            normal_only: Mat4::from_rotation_translation(aligned, origin) * scale,
        })
    }

    /// `0x6fa7c0`, the frame the corner builder asks for at `0x71eb3b`, as a
    /// truth table over the two element bytes and the owner state:
    ///
    /// ```text
    /// no ground query                        -> the un-snapped frame
    /// GroundHeight &&  MatchGrnd             -> owner+0xfc   aligned
    /// GroundHeight && !MatchGrnd             -> owner+0x13c  height only
    /// !GroundHeight && MatchGrnd && near     -> owner+0xfc   aligned
    /// otherwise                              -> the un-snapped frame
    /// ```
    ///
    /// Both bytes are reduced with `setne` at load time, so the number authored
    /// for `GroundHeight` does not matter, only whether it is nonzero. The
    /// companion selector `0x6fa810` picks the element's *orientation* with the
    /// same conditions — the ground-aligned quaternion at owner+0x1b4 exactly
    /// when `MatchGrnd && (GroundHeight || owner+0x284)` — which is why one
    /// frame substitution covers both: the aligned frame is built from that
    /// quaternion at `0x6e5539`, so it already carries the aligned orientation.
    fn ground_parent(&self, frames: Option<&GroundFrames>, element: &Element, base: Mat4) -> Mat4 {
        let Some(frames) = frames else { return base };
        // element+0x150 `GrndNormOnly` takes precedence over both selectors
        // below: it asks for the ground normal alone, which is neither of the
        // frames `GroundHeight`/`MatchGrnd` choose between. 456 corpus sprites
        // set it, almost all of them weapon trails that have to lie in the
        // struck surface without dropping to it.
        if matches!(&element.spec, Spec::Sprite(s) if s.ground_normal_only) {
            return frames.normal_only;
        }
        if element.common.ground_height != 0. {
            return if element.common.match_ground {
                frames.aligned
            } else {
                frames.height_only
            };
        }
        if element.common.match_ground && frames.near_ground {
            return frames.aligned;
        }
        base
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
    // The owner's placement scale, for the screen-space sprite path.
    owner_scale: f32,
    // The root's `Vec:` pair, which element 190 uses as its span endpoints.
    span: [Vec3; 2],
    rng: &mut Rng,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
    // Models already posed at load time, in element order. Empty when the host
    // supplied no model reader, in which case model elements keep counting.
    meshes: &[EffectMesh],
    // The owner's surface, for the ground-draped sprite path. Copy, so every
    // element of the root can take its own.
    drape: Option<GroundSampler<'_>>,
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
    // Publish the world placement before anything consumes it, so an element
    // bound to this one follows the matrix it actually used. `0x6fe120` reads
    // the target's stored transform at +0x58 rather than recomputing it, which
    // is what keeps a mutual bind from recursing.
    runtime.placement = Some(placement);
    let context = ElementContext {
        id: element.id,
        placement,
        spin: direction,
        keypoint_direction: sampled.direction,
        view,
        common: &element.common,
        version,
        color: apply_color_offset(color, &runtime.state.color_offset),
        span,
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
        owner_scale,
    };
    // `IsDummy` marks an element the client builds but never submits: `冲击波1`
    // keeps a whole child element behind it. Note that `DummyEle` is *not* this
    // flag — it names another element to take a basis from, and the drawable
    // elements of `场景鱼群` and `水瀑术_手轨迹` are `IsDummy: 0` with empty
    // `DummyEle`, so gating on it deletes real geometry.
    if element.common.is_dummy {
        stats.dummies += 1;
        return Ok(done);
    }
    match &mut runtime.runtime {
        Runtime::Clock => {
            emit_clock(element, &context, face_to_viewer, stats, out, drape);
            if matches!(element.spec, Spec::Model(_)) {
                emit_model(meshes, &context, out);
            }
        }
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
                let amplitude = *amplitude * NOISE_CM_PER_UNIT;
                state.noise = Vec3::new(sample, sample * 0.5, sample * 0.25) * amplitude;
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

/// The camera shake's reach. `0x6e5037` compares root+0x29c — the distance the
/// LOD pass uses too — against the 30.0 at `0x8eb710`, and at or beyond it the
/// driver writes a zero offset instead of sampling the noise.
const SHAKE_MAX_DISTANCE: f32 = 30.;

/// `0x8e6f14`, what every `rand()` draw is multiplied by: 2π over 32767, MSVC's
/// `rand()` returning 0..32767, so one draw spans exactly one turn.
const SHAKE_ANGLE_PER_RAND_UNIT: f32 = std::f32::consts::TAU / 32_767.;

/// The three fields the shake driver keeps on the root: the latch at +0x1f7, the
/// frame counter at +0x1f8 and the offset at +0x1fc..+0x204.
#[derive(Clone, Copy, Debug, Default)]
struct ShakeState {
    /// root+0x1f7. Set on the first frame inside the window, where the client
    /// saves the untouched camera (`0x41a480`), and cleared by the exit path
    /// `0x6e5150`, which restores it — so leaving the window and coming back
    /// starts the shake over from frame zero.
    started: bool,
    /// root+0x1f8. Incremented every frame inside the window; `0x6e5015` reduces
    /// it modulo two, sign included, and the parity picks the branch.
    counter: u32,
    /// root+0x1fc..+0x204, the offset last handed to the camera. Odd frames
    /// negate it in place (`0x6e50f3`), which is what makes the camera oscillate
    /// around the position that was saved for it.
    offset: Vec3,
}

/// What the driver reads off the controller: the window in seconds
/// (controller+0x8 and +0xc) and the five fractal-noise parameters its loader
/// `0x6ece00` reads into controller+0xf8..+0x108, where the constructor
/// `0x6f10f0` had already put 256, 2.0, 1, 0.25 and 5.
#[derive(Clone, Copy, Debug)]
struct ShakeSpec {
    start: f32,
    end: f32,
    buffer_length: u32,
    amplitude: f32,
    wave_length: f32,
    persistence: f32,
    octaves: u32,
}

/// The shake a root asks for, or `None`. The driver `0x6e4dc0` needs both
/// root+0x1f6 — the `setne` of `ShakeCam`, read from version 24 on at `0x6e38a4`
/// — and root+0x208, the controller `0x6eae10` builds when that flag is set.
/// `0x6eae10` reads a root-level `CtrlType: %d` and hands it to the factory
/// `0x6ea910`, which subtracts 100 and indexes the thirteen-entry table at
/// `0x6eaddc`, so the shake is `CtrlType: 112` and nothing else. All 241 corpus
/// roots that shake author exactly that, with `BufLen` 256, `WaveLen` 1,
/// `Persistence` 0.25 and `OctaveNum` 5 — the constructor defaults — and only
/// `Amplitude` varying, from 0.05 to 2.0; the other 5,218 author no `CtrlType` at
/// all. A nonzero `ShakeCam` with a `CtrlType` outside 100..=112 leaves
/// `0x6eae10` returning null, which fails the whole root load at `0x6e38f8`. The
/// viewer reports no shake instead of rejecting the file, the way it warns about
/// an unresolvable child reference rather than dropping the root that holds it.
fn root_shake(root: &Root) -> Option<ShakeSpec> {
    if root.shake_cam == 0 {
        return None;
    }
    root.controls.iter().find_map(|control| {
        let ControlData::Fractal {
            buffer_length,
            amplitude,
            wave_length,
            persistence,
            octaves,
            ..
        } = &control.data
        else {
            return None;
        };
        (control.kind == 112).then_some(ShakeSpec {
            start: control.start,
            end: control.end,
            buffer_length: *buffer_length,
            amplitude: *amplitude,
            wave_length: *wave_length,
            persistence: *persistence,
            octaves: *octaves,
        })
    })
}

/// `0x6e4e2b`: the direction the offset is pushed along. Two `rand()` draws
/// (`0x8d5478` in the import table), each scaled by 2π/32767, become axis-angle
/// rotations through `0x7b6fd0` about the globals at `0x9c0348` and `0x9c0358`,
/// are composed by `0x4071f0` and applied to a unit axis by `0x4093d0`. Both
/// globals live past the raw image, so their bytes are not in the file, but two
/// static-initialiser thunks write them: `0x6ea170` stores (0, 1, 0) and
/// `0x6ea140` stores (1, 0, 0), which makes them the unit Y and unit X axes. Both draws happen every frame inside the window, ahead of the
/// parity test, and only the even frames use the result. Which rotation the
/// client applies first is not readable out of the quaternion helper sequence,
/// so this is the viewer's reading of it — a fresh random unit direction from the
/// same two angles — rather than a claim about the composition order.
fn shake_direction(rng: &mut Rng) -> Vec3 {
    let angle = |value: u32| (value & 0x7fff) as f32 * SHAKE_ANGLE_PER_RAND_UNIT;
    let first = angle(rng.next_u32());
    let second = angle(rng.next_u32());
    let rotation = Quat::from_axis_angle(Vec3::Y, first) * Quat::from_axis_angle(Vec3::X, second);
    rotation * Vec3::X
}

/// The exit path `0x6e5150`: the camera goes back to what was saved for it and
/// the latch is cleared. The counter is left alone, because the client zeroes it
/// when the latch is next set rather than when it is cleared.
fn stop_camera_shake(state: &mut ShakeState) {
    state.started = false;
    state.offset = Vec3::ZERO;
}

/// One frame of the shake driver `0x6e4dc0`. `time_ms` is root+0x1cc, the root's
/// millisecond tick, which the driver scales by the 0.001 at `0x8d5c70` before
/// comparing it against the window; `distance` is root+0x29c, measured from the
/// camera to the root's origin.
fn camera_shake_frame(
    root: &Root,
    state: &mut ShakeState,
    rng: &mut Rng,
    seed: u32,
    time_ms: f32,
    distance: f32,
) -> Vec3 {
    let Some(shake) = root_shake(root) else {
        stop_camera_shake(state);
        return Vec3::ZERO;
    };
    let seconds = time_ms * 0.001;
    // `0x6e4dfb` leaves when the clock has not reached `StartTime`, and
    // `0x6e4e0c` compares `EndTime` against the 0.0 at `0x8d58a0` first: a
    // negative end skips the end test entirely, which is the constructor's own
    // default of -1.0 at `0x6f111f` and what a root below version 21 gets, since
    // `0x6eaef8` writes 0 and -1.0 instead of reading the two keys.
    if seconds < shake.start || (shake.end >= 0. && seconds > shake.end) {
        stop_camera_shake(state);
        return Vec3::ZERO;
    }
    if !state.started {
        state.started = true;
        state.counter = 0;
    }
    let odd = state.counter % 2 == 1;
    state.counter = state.counter.wrapping_add(1);
    let direction = shake_direction(rng);
    if odd {
        state.offset = -state.offset;
        return state.offset;
    }
    state.offset = if distance >= SHAKE_MAX_DISTANCE {
        // `0x6e5044`: at or beyond thirty units three zeroes are written and
        // still applied, so the camera is put back exactly where it was.
        Vec3::ZERO
    } else {
        // `0x6e5067`: below thirty units the offset is the noise times
        // `1 - distance / 30`, times the direction. The noise object is sampled
        // with the millisecond tick as its coordinate, so the lattice advances
        // once per millisecond and consecutive 60 Hz frames land sixteen cells
        // apart — a jitter rather than a wobble. The amplitude is authored on the
        // controller and the client's noise object owns its gradient table, so it
        // scales the sample the way element 150's amplitude scales its offsets.
        let fade = 1. - distance / SHAKE_MAX_DISTANCE;
        let sample = fbm(
            seed,
            time_ms,
            shake.wave_length.max(1e-3),
            shake.persistence,
            shake.octaves.max(1) as usize,
            shake.buffer_length,
        );
        direction * (fade * shake.amplitude * sample)
    };
    state.offset
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

/// FNV-1a over an archive path, forced odd. The client seeds an emitter from a
/// runtime stream this viewer does not reproduce; what a replay needs is that one
/// root keeps one stream and that two roots playing side by side — a row of
/// torches, two waterfalls — do not share one, or every copy emits the same
/// particles in lockstep. Odd because a zero seed stalls the xorshift in
/// [`Rng`] at zero forever.
pub fn path_seed(path: &str) -> u32 {
    let mut seed = 2_166_136_261u32;
    for byte in path.as_bytes() {
        seed ^= u32::from(*byte);
        seed = seed.wrapping_mul(16_777_619);
    }
    seed | 1
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
        // A missing `TexInterval` is 0, and 0 used to be substituted with
        // **1 ms**: a sheet with `TexRow`/`TexCol` above one but no interval
        // flipped a thousand cells a second, which at 60 Hz is a strobe rather
        // than an animation. Nothing authors a one-millisecond flip, so an
        // absent interval holds the first cell. Every corpus element that
        // enables the flipbook writes an interval, so this only decides a
        // damaged or hand-written record.
        let interval = if common.tex_interval == 0 {
            f32::INFINITY
        } else {
            common.tex_interval as f32
        };
        let limit = if common.tex_anim_max_time > 0. {
            ((common.tex_anim_max_time / interval) as u32).clamp(1, cells)
        } else {
            cells
        };
        let index = if interval.is_finite() {
            ((clock_ms.max(0.) / interval).floor() as u32) % limit
        } else {
            0
        };
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
        priority: context.common.priority,
        positions,
        uv,
        color,
        mesh: None,
    });
}

/// Controller 107's `Amplitude` is authored in centimetres, not world units.
///
/// This was briefly removed and had to be put back: with the value read as
/// world units, controller-107 effects throw themselves far outside their own
/// bounds, which is the "effects play fast and out of bounds" report. The
/// corpus is unambiguous. 1,088 controller-107 amplitudes, 761 of them above
/// ten and landing on round authored numbers — 200 (p50), 500 (p90), 800, 1000
/// — on roots barely a quarter of a unit across. As world units the median is
/// 64x its own root's `Vec:` half-extent and the worst is 3,998x. Divided by
/// 100 the median is 0.64 of its bounds, which is what a wobble occupies.
///
/// Note the *fractal element's* own `Amplitude` key is a different quantity,
/// authored 0.05..5.0, and must never take this conversion.
const NOISE_CM_PER_UNIT: f32 = 0.01;

/// `_default_sects` in `A3DGFXRing.cpp`: the section count a ring falls back to
/// when its `Sects` record is zero, which is what the editor writes for "leave
/// it alone". It is a count of ring segments, so the band is built from this
/// many quads.
const DEFAULT_RING_SECTIONS: usize = 36;

/// The stateless classes: sprites, cylinders and grids. Lights belong to
/// `gfx_light`, models to the model queue and sounds to the audio path; all
/// three are counted so the status line stays honest.
fn emit_clock(
    element: &Element,
    context: &ElementContext,
    face_to_viewer: bool,
    stats: &mut Stats,
    out: &mut Vec<EffectQuad>,
    drape: Option<GroundSampler<'_>>,
) {
    match &element.spec {
        Spec::Sprite(sprite) => emit_sprite(sprite, context, face_to_viewer, out, drape),
        Spec::Cylinder(cylinder) => emit_cylinder(cylinder, context, out),
        Spec::Grid(grid) => emit_grid(grid, context, out),
        Spec::Light(_) => stats.lights += 1,
        Spec::Sound(_) => stats.sounds += 1,
        Spec::Model(_) => stats.models += 1,
        Spec::Curve(curve) => emit_curve(curve, context, out),
        Spec::Unknown { .. } => stats.unrendered += 1,
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
    drape: Option<GroundSampler<'_>>,
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
    // A ground sheet is not a card. `Fill_Verts_3D` cuts it into two-unit cells
    // and asks the terrain for each vertex's own height; everything below is the
    // flat fallback for the sheets that cannot be draped.
    if let Some(sampler) = drape {
        if drape_sprite(sprite, context, &texture, color, sampler, out) {
            return;
        }
    }
    // `Fill_Verts_2D` sizes the card from `m_pGfx->GetActualScale()` — the GFX's
    // own `DedaultScale` — and takes only the card's *centre* from
    // `GetParentTM()`, so the owner's map scale must not be multiplied into the
    // extents. Measured against the 10 November census: `树林光束` is authored
    // `DedaultScale: 0.7` with 0.5 x 10 cards, and this map places it at 8.776,
    // which drew a 6 x 123 tree-top shaft where retail draws 0.7 x 14.
    let owner = if context.owner_scale.is_finite() && context.owner_scale > 1e-6 {
        context.owner_scale
    } else {
        1.
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
        1. / owner,
    );
    let uv = flipbook_uv(context.common, context.clock);
    push_quad(out, context, &texture, positions, uv, color);
}

/// `A3DDecalEx::Fill_Verts_3D`'s ground branch (`A3DDecalEx.cpp:449-542`),
/// which is what turns a ground sheet into a piece of the surface.
///
/// It is entered only when the element is the 3D decal, sets `MatchGrnd`, does
/// not set `GrndNormOnly`, and the owner was placed with ground data and sits
/// within `GROUND_NEAR_DISTANCE` of the surface. The sheet is then laid out in
/// the **world** XZ plane instead of the element's own plane: column `j` sits at
/// `-Width1 + j * cell` along world X, row `i` at `Height2 - i * cell` along
/// world Z, the cell count is the parent-scaled extent over `GROUND_GRID_CELL`
/// truncated and clamped to `GROUND_MAX_ROW_OR_COL`, and every vertex takes its
/// own height from the terrain plus the 0.2 lift. The flipbook rect is spread
/// over the whole lattice rather than repeated per cell, and the element's
/// pitch, yaw and 2D spin are all dropped — only `YawEffect` puts them back and
/// that routine is not implemented, which leaves 19 of the 906 ground-matched
/// corpus elements axis-aligned.
///
/// Returns false when the sprite has to stay the flat quad: a sampler that
/// misses under any lattice point leaves the whole sheet alone, because half a
/// drape tears, and this path owns no partial mesh to fall back on.
fn drape_sprite(
    sprite: &Sprite,
    context: &ElementContext,
    texture: &str,
    color: [f32; 4],
    sampler: GroundSampler<'_>,
    out: &mut Vec<EffectQuad>,
) -> bool {
    // Element 102 builds its vertices through `Update_Billboard` and 101 is
    // drawn in screen space; the ground branch only exists on the 3D update.
    if context.id != 100
        || !context.common.match_ground
        || sprite.ground_normal_only
        || !sampler.near_ground
    {
        return false;
    }
    // `fParentScale` scales the extents in the client before the cell count is
    // worked out. The placement already carries the root and placement scale, so
    // the offsets below would lose it if they went through the matrix, which is
    // why the same product the billboard branch reads is taken here.
    let parent_scale = context.placement.x_axis.truncate().length();
    let scale_x = (if sprite.no_scale[0] {
        1.
    } else {
        context.scale
    }) * parent_scale;
    let scale_y = (if sprite.no_scale[1] {
        1.
    } else {
        context.scale
    }) * parent_scale;
    let width1 = sprite.width * 2. * sprite.origin[0] * scale_x;
    let height0 = sprite.height * 2. * sprite.origin[1] * scale_y;
    let width2 = sprite.width * 2. * scale_x - width1;
    let height2 = sprite.height * 2. * scale_y - height0;
    let width = width1 + width2;
    let height = height0 + height2;
    let cells = |extent: f32| -> usize {
        let count = (extent / GROUND_GRID_CELL) as i32;
        count.clamp(1, GROUND_MAX_ROW_OR_COL as i32) as usize
    };
    let cols = cells(width);
    let rows = cells(height);
    let cell_w = width / cols as f32;
    let cell_h = height / rows as f32;
    let centre = context.placement.transform_point3(Vec3::ZERO);
    // The query is a field, not a method: `sampler.height(..)` would look for a
    // method on the struct, so it is bound first.
    let height = sampler.height;
    let uv = flipbook_uv(context.common, context.clock);
    let sample = |i: usize, j: usize| -> Option<([f32; 3], [f32; 2])> {
        let x = centre.x - width1 + j as f32 * cell_w;
        let z = centre.z + height2 - i as f32 * cell_h;
        let y = height(x, z)? + GROUND_DRAPE_LIFT;
        let fi = i as f32 / rows as f32;
        let fj = j as f32 / cols as f32;
        let u = uv[0][0] + (uv[1][0] - uv[0][0]) * fj;
        let v = uv[0][1] + (uv[2][1] - uv[0][1]) * fi;
        Some(([x, y, z], [u, v]))
    };
    // Sample the whole lattice before emitting anything, so a miss under one
    // corner cannot leave the rest of the sheet drawn over a hole.
    let mut lattice = Vec::with_capacity((cols + 1) * (rows + 1));
    for i in 0..=rows {
        for j in 0..=cols {
            let Some(point) = sample(i, j) else {
                return false;
            };
            lattice.push(point);
        }
    }
    for i in 0..rows {
        for j in 0..cols {
            let corners = [
                lattice[i * (cols + 1) + j],
                lattice[i * (cols + 1) + j + 1],
                lattice[(i + 1) * (cols + 1) + j],
                lattice[(i + 1) * (cols + 1) + j + 1],
            ];
            let mut positions = [[0f32; 3]; 4];
            let mut uvs = [[0f32; 2]; 4];
            for (slot, (position, corner_uv)) in corners.into_iter().enumerate() {
                positions[slot] = position;
                uvs[slot] = corner_uv;
            }
            push_quad(out, context, texture, positions, uvs, color);
        }
    }
    true
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

/// Emit a model element's meshes, transformed by the element's placement.
///
/// The geometry was posed at load time with the model's own world matrix left at
/// identity, so the element's frame is the only transform: the placement carries
/// the owner's position and orientation and the keypoint scale, which is what
/// the client multiplies a model's vertices by. Model colour is taken from the
/// element's keypoint like any other class, through `context.rgba()`.
fn emit_model(meshes: &[EffectMesh], context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    for mesh in meshes {
        if mesh.texture.is_empty() || mesh.vertices.len() < 3 {
            continue;
        }
        let placed: Vec<EffectVertex> = mesh
            .vertices
            .iter()
            .map(|vertex| {
                let position = context
                    .placement
                    .transform_point3(Vec3::from_array(vertex.position));
                EffectVertex {
                    position: position.to_array(),
                    uv: vertex.uv,
                }
            })
            .filter(|vertex| vertex.position.iter().all(|value| value.is_finite()))
            .collect();
        if placed.len() < 3 {
            continue;
        }
        out.push(EffectQuad {
            texture: mesh.texture.clone(),
            source_blend: context.common.source_blend,
            destination_blend: context.common.destination_blend,
            depth_test: depth_test(context.common, context.version),
            layer: context.common.render_layer,
            priority: context.common.priority,
            positions: [[0.; 3]; 4],
            uv: [[0.; 2]; 4],
            color,
            mesh: Some(EffectMesh {
                texture: mesh.texture.clone(),
                vertices: placed,
            }),
        });
    }
}

/// Load one element-160 model and return its posed meshes as triangle lists.
///
/// This is the same chain the ECM path uses (`ModelAsset::load_static_skins_with`
/// then `pose_bind`), driven by the three relative names a `.SMD` carries. It is
/// deliberately static: the effect archive's skins are bound to 0 or 1 bones, so
/// there is no animation to sample, and inventing one would be a guess.
fn posed_model_meshes(
    path: &str,
    parent: &str,
    fallback_texture: &str,
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<EffectMesh>, String> {
    // `ModelPath` already carries the model's own folder and is relative to the
    // archive's `gfx/models/` root, *not* to the GFX file that names it:
    // `gfx/场景/新种族地图/暗语者营地.gfx` authoring `暗语者营地\水晶.SMD`
    // resolves to the member `gfx/models/暗语者营地/水晶.smd`, which is shared
    // between that root and every other root naming the same model. Measured
    // against `gfx.pck`: none of the 124 shipped `.smd` members sit under the
    // GFX folder's own subdirectory.
    let smd_path = if parent.is_empty() {
        crate::ecmodel::normalize(path)
    } else {
        format!("{parent}/{}", crate::ecmodel::normalize(path))
    };
    let smd_bytes = read(&smd_path).map_err(|error| format!("{smd_path}: {error}"))?;
    let model = crate::ecmodel::parse_smd(&smd_bytes).map_err(|e| format!("{smd_path}: {e}"))?;
    // `skeleton` and `skins` are bare names relative to the SMD's own folder,
    // which is one level below `gfx/models/`: the `.bon` of
    // `gfx/models/白色爪痕/白色爪痕.smd` is `gfx/models/白色爪痕/白色爪痕.bon`.
    // Joining them to `gfx/models/` instead loses the model's own directory and
    // resolved every skeleton to a missing member.
    let parent = smd_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let join = |name: &str| {
        let name = crate::ecmodel::normalize(name);
        if parent.is_empty() {
            name
        } else {
            format!("{parent}/{name}")
        }
    };
    let skeleton_path = join(&model.skeleton);
    let skins: Vec<String> = model.skins.iter().map(|skin| join(skin)).collect();
    let asset =
        crate::model_asset::ModelAsset::load_static_skins_with(&skeleton_path, &skins, |name| {
            read(name)
        })?;
    let posed = asset.pose_bind(Mat4::IDENTITY)?;
    let mut meshes = Vec::with_capacity(posed.len());
    for piece in posed {
        let skin = asset
            .skins
            .get(piece.skin)
            .ok_or("posed skin out of range")?;
        let mesh = skin
            .data
            .meshes
            .get(piece.mesh)
            .ok_or("posed mesh out of range")?;
        // A mesh with no texture index is drawn with the element's own `TexFile`
        // when it has one. Only 4 of the 792 shipped meshes are in this state
        // (`白色爪痕`, `投石车状态`, `攻击`'s footstep plane and
        // `血鲷封印台罩子`), and they still carry real geometry — 33 vertices
        // and 108 indices for the claw mark. Dropping them lost the whole model
        // for the roots that name them.
        let texture = if mesh.texture >= 0 {
            match skin.texture_paths(mesh.texture as usize) {
                Ok(paths) => paths[0].clone(),
                Err(_) => fallback_texture.to_string(),
            }
        } else {
            fallback_texture.to_string()
        };
        // The GPU pass falls back to a flat white stand-in for a texture it
        // cannot read, so an untextured mesh must keep its bucket rather than be
        // dropped here: `白色爪痕` is a pure white claw mark with no map at all,
        // and the element that names it has an empty `TexFile` too.
        let texture = if texture.is_empty() {
            UNTEXTURED_MESH.to_string()
        } else {
            texture
        };
        let mut vertices = Vec::with_capacity(mesh.indices.len());
        for index in &mesh.indices {
            let Some(vertex) = piece.vertices.get(*index as usize) else {
                continue;
            };
            let Some(source) = mesh.vertices.get(*index as usize) else {
                continue;
            };
            if !vertex.position.iter().all(|value| value.is_finite()) {
                continue;
            }
            vertices.push(EffectVertex {
                position: vertex.position,
                uv: source.uv,
            });
        }
        if vertices.len() >= 3 {
            meshes.push(EffectMesh { texture, vertices });
        }
    }
    Ok(meshes)
}

fn emit_cylinder(cylinder: &Cylinder, context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let color = context.rgba();
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    // `Sects: 0` is not "three sections". `A3DGFXRing::InitStreamBuffer` reads
    // `if (m_nSects) nSect = m_nSects; else nSect = _default_sects;` and
    // `_default_sects` is 36, so the zero the editor writes means "the default"
    // — and it writes it on 1,064 of the corpus's 1,116 element-140 records.
    // Clamping the zero up to three drew every one of those rings as a
    // three-sided prism: the city's light rings and the altar bands came out as
    // angular slabs instead of round bands.
    let sections = match cylinder.sections {
        0 => DEFAULT_RING_SECTIONS,
        count => usize::try_from(count).unwrap_or(DEFAULT_RING_SECTIONS),
    }
    .max(3);
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
    // `Pitch` tilts the band, and it does so through the radii, not through an
    // angular offset: `fDeltaRadius = fHalfHeight * tan(pitch)` after
    // `tan(pitch)` is clamped to `fLimit = m_fRadius / fHalfHeight`, then
    // `fUppderRadius = m_fRadius - fDeltaRadius` for the upper row and
    // `fLowerRadius = m_fRadius + fDeltaRadius` for the lower one. The band's
    // foot is therefore the wider edge and its roof the narrower one, which is
    // what makes a flat ring texture read as a surface seen at an angle.
    let half_height = height * 0.5;
    let (upper_radius, lower_radius) = if half_height > 0. {
        let limit = radius / half_height;
        let mut tan = cylinder.pitch.tan();
        if !tan.is_finite() {
            tan = limit;
        }
        let delta = half_height * tan.clamp(-limit, limit);
        (radius - delta, radius + delta)
    } else {
        (radius, radius)
    };
    let base_y = if cylinder.origin_at_center {
        -height * 0.5
    } else {
        0.
    };
    let matrix = context.placement;
    let uv = flipbook_uv(context.common, context.clock);
    let mut positions = [[0f32; 3]; 4];
    for index in 0..sections {
        let a0 = index as f32 / sections as f32 * std::f32::consts::TAU;
        let a1 = (index + 1) as f32 / sections as f32 * std::f32::consts::TAU;
        let corners = [
            Vec3::new(a0.cos() * lower_radius, base_y, a0.sin() * lower_radius),
            Vec3::new(a1.cos() * lower_radius, base_y, a1.sin() * lower_radius),
            Vec3::new(
                a1.cos() * upper_radius,
                base_y + height,
                a1.sin() * upper_radius,
            ),
            Vec3::new(
                a0.cos() * upper_radius,
                base_y + height,
                a0.sin() * upper_radius,
            ),
        ];
        for (slot, corner) in corners.into_iter().enumerate() {
            positions[slot] = matrix.transform_point3(corner).to_array();
        }
        // `tu = i * m_fTexWid / nSects + m_fTexU`, and the lower row of the
        // strip carries `tv = m_fTexV + m_fTexHei` while the upper carries
        // `m_fTexV`.
        let u_start = uv[0][0];
        let u_span = uv[1][0] - uv[0][0];
        let u0 = u_start + u_span * index as f32 / sections as f32;
        let u1 = u_start + u_span * (index + 1) as f32 / sections as f32;
        let (v_low, v_high) = (uv[2][1], uv[0][1]);
        let strip = [[u0, v_low], [u1, v_low], [u1, v_high], [u0, v_high]];
        push_quad(out, context, &texture, positions, strip, color);
    }
}

/// Element 190: a curve, built by `0x708460` in the client.
///
/// What the builder does, recovered from the disassembly:
///
/// * It transforms the element's keypoint position and the root's own `Vec`
///   pair (`root+0x188`, `root+0x18c`) by the current matrix (`0x7872d0`), then
///   takes the element's local point from `0x6fa810` and subtracts it.
/// * It cross-multiplies two of those vectors to get a side vector, then
///   normalises that vector and the chord between its endpoints.
/// * The segment count comes from dividing 1.0 by the normalised length.
/// * The two `Vec` endpoints are written to the caller's two output vertices,
///   and the spanning quad is closed from there.
///
/// The curve reaches for two *root-level* vectors rather than one element
/// position, which is the far-field placement convention the root `Vec:` pair
/// exists for: it is how the map authors a strip that spans a long distance —
/// the floating city's beams — instead of a point effect. That is why every
/// placed curve in the corpus sits in `0远完美浮空城.gfx`, the distant floating
/// city seen from the starting map.
///
/// The client skips the quad entirely once the chord is degenerate: both
/// `fcoml -1e-6` and `fcoml 1e-6` are tested and either answer drops the
/// element to the zero-segment path.
///
/// The span between the two vectors is reproduced exactly. What is not claimed
/// is the cross-product side vector's exact orientation: the two vectors whose
/// cross product the builder takes are chosen inside the routine, and this
/// viewer orients the ribbon across the chord by the view instead. The segment
/// count and both endpoints are the recovered values.
fn emit_curve(curve: &Curve, context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let [first, second] = context.span;
    let start = context.placement.transform_point3(first);
    let end = context.placement.transform_point3(second);
    let chord = end - start;
    let length = chord.length();
    if !length.is_finite() || length <= 1e-6 {
        return;
    }
    // Segment count for a constant stretch per segment.
    //
    // The count was `(1.0 / length) * coefficient`, which is inverted: it gave
    // a *longer* chord fewer segments. Its own comment states the invariant it
    // is meant to hold -- "keeps the stretch per segment constant" -- and
    // `stretch = length / segments` is `length^2 / coefficient` under that
    // formula, so it holds for nothing. Any chord longer than the coefficient
    // collapsed to the one-segment floor and drew the whole beam or trail as a
    // single billboard stretched across its span, which is the flat smear this
    // element is visible as. `length / coefficient` makes `stretch` the
    // coefficient exactly, which is the constant the comment names.
    //
    // `Coeff` is authored 1.0 on every corpus element 190 (the parser defaults
    // it there), so a 10-unit beam is ten quads and a 40-unit beam forty, and
    // the 256 cap still bounds the worst case. The count remains an inference:
    // the client's builder at the element-190 routine was never read, only the
    // endpoints and the degenerate-chord gate.
    let segments = (length / curve.coefficient.max(1e-3)).clamp(1.0, 256.0) as usize;
    let segments = segments.max(1);
    let uv = flipbook_uv(context.common, context.clock);
    let color = rgba(context.color, context.color_scale);
    if color[3] < ALPHA_CUTOFF {
        return;
    }
    // Half-width across the chord, in the plane the camera sees.
    let across = {
        let side = chord.cross(context.view.forward);
        if side.length_squared() > 1e-9 {
            side.normalize()
        } else {
            chord.cross(context.view.up).normalize_or_zero()
        }
    };
    let half = (curve.height.max(0.) * 0.5) * context.scale;
    let mut positions = [[0f32; 3]; 4];
    for step in 0..segments {
        let a = step as f32 / segments as f32;
        let b = (step + 1) as f32 / segments as f32;
        let (p0, p1) = (start + chord * a, start + chord * b);
        let corners = [
            p0 - across * half,
            p1 - across * half,
            p0 + across * half,
            p1 + across * half,
        ];
        for (slot, corner) in corners.into_iter().enumerate() {
            positions[slot] = corner.to_array();
        }
        push_quad(out, context, &texture, positions, uv, color);
    }
}

fn emit_grid(grid: &Grid, context: &ElementContext, out: &mut Vec<EffectQuad>) {
    let Ok(texture) = texture_path(&context.common.texture) else {
        return;
    };
    let matrix = context.placement;
    let rect = flipbook_uv(context.common, context.clock);
    let mut positions = [[0f32; 3]; 4];

    // A grid is a lattice of `columns * rows` **vertices**, not cells: all 216
    // corpus grids write exactly one `x y z` row and one `dwColor` per vertex,
    // and none is degenerate. Two columns by four rows is therefore a curtain
    // one quad wide and three tall — the shape every waterfall in the game is
    // authored as, 23 units wide and 36 tall, with the lower rows tinted
    // `88caff` so the sheet fades out at its foot. Building the quads from
    // `fGridSize` instead collapses that curtain to a 0.5-unit patch lying flat
    // on the ground, which is the motionless white rectangle on screen.
    //
    // The authored positions are already element-local, so they take the
    // element matrix but not the `scale` the flat fallback multiplies in.
    let authored = !grid.vertices.is_empty();
    let cell = grid.cell_size * context.scale;
    if !authored && (!cell.is_finite() || cell <= 0.) {
        return;
    }
    let half = cell * 0.5;
    let (span_x, span_z) = if authored {
        (grid.columns.saturating_sub(1), grid.rows.saturating_sub(1))
    } else {
        (grid.columns, grid.rows)
    };
    // `A3DGridDecalEx::_setUVCoord` spreads **one** texture rect across the whole
    // lattice: U from `m_fTexU` in `m_nWidthVertNum - 1` steps and V from
    // `m_fTexV` in `m_nHeightVertNum - 1` steps. Handing every cell the whole
    // rect instead tiled the sheet once per row — a 2x4 curtain repeated the
    // image three times down its height and restarted the `TexSpeed` scroll at
    // every row boundary, which reads as three stacked copies sliding over each
    // other rather than one sheet flowing.
    let (u_start, v_start) = (rect[0][0], rect[0][1]);
    let (u_span, v_span) = (rect[1][0] - rect[0][0], rect[2][1] - rect[0][1]);
    for row in 0..span_z {
        for col in 0..span_x {
            let index = (row * grid.columns + col) as usize;
            let Some(value) = grid.colors.get(index) else {
                continue;
            };
            // One flat colour per quad: the corner with the lowest index wins.
            // A vertex that fades to transparent therefore still draws while
            // any of its neighbours is opaque, which keeps the top of a curtain
            // solid and stops only the fully transparent foot.
            let color = rgba(*value, context.color_scale);
            if color[3] < ALPHA_CUTOFF {
                continue;
            }
            let corners = if authored {
                let columns = grid.columns as usize;
                let at = |offset: usize| grid.vertices.get(index + offset).copied();
                match (at(0), at(1), at(columns), at(columns + 1)) {
                    (Some(a), Some(b), Some(c), Some(d)) => [a, b, c, d],
                    _ => continue,
                }
            } else {
                let x = (col as f32 - (grid.columns as f32 - 1.) * 0.5) * cell;
                let z = (row as f32 - (grid.rows as f32 - 1.) * 0.5) * cell;
                [
                    Vec3::new(x - half, grid.z_offset, z - half),
                    Vec3::new(x + half, grid.z_offset, z - half),
                    Vec3::new(x - half, grid.z_offset, z + half),
                    Vec3::new(x + half, grid.z_offset, z + half),
                ]
            };
            for (slot, corner) in corners.into_iter().enumerate() {
                positions[slot] = matrix.transform_point3(corner).to_array();
            }
            let ua = u_start + u_span * col as f32 / span_x as f32;
            let ub = u_start + u_span * (col + 1) as f32 / span_x as f32;
            let va = v_start + v_span * row as f32 / span_z as f32;
            let vb = v_start + v_span * (row + 1) as f32 / span_z as f32;
            let uv = [[ua, va], [ub, va], [ua, vb], [ub, vb]];
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
            oriented_basis(context, state.direction)
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

/// The basis of a particle that keeps its own travel direction instead of facing
/// the camera (`Is3D`).
///
/// `state.direction` lives in the element's own frame: the emission samplers
/// produce it there, and the integrator that moves a particle by it
/// (`state.position += state.direction * step`) moves a position that
/// `quad_corners` sends through `context.placement`. `View`'s axes are world.
/// Mixing the two made the orientation depend on the element's placement
/// rotation twice — once through the direction and once through the quad that
/// direction built — so a spun element gave its 3D particles a basis no frame
/// agreed with. Bring the direction into the world frame with the same placement
/// the billboard path uses for its centre, then mix.
fn oriented_basis(context: &ElementContext, direction: Vec3) -> (Vec3, Vec3) {
    particle_basis(context.placement.transform_vector3(direction), context.view)
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
    // Id 120's block at `0x7272aa` is the odd one out. It reads neither
    // `AreaSize` nor the basis: it spends a single draw on an azimuth about the
    // global Z, tilts by `pi/2 - Angle` about the spun X, multiplies the two into
    // node+0x34 (`0x70a320` at `0x727448`), rotates the global Y by that into
    // node+0x44, and sets node+0x10 to the same vector times the 0.001 at
    // `0x8ee224` — the unit conversion the volume path applies twice as well
    // (`0x7276bc`, `0x7276d6`). Its particles therefore sit one unit out, on the
    // surface of a cone of half-angle `Angle` about the local Z.
    //
    // Id 121's `0x727800` is a box surface, and it is the only sampler that
    // chooses *which* face to use: `0x72780d`-`0x727843` form the three face
    // products xy, yz and xz of `AreaSize`, their sum, and the ratio yz/sum, then
    // one draw decides — below xy/sum takes the Z faces (`0x727868`), below
    // yz/sum the X faces (`0x7278df`), otherwise the Y faces. The second test is
    // against yz/sum alone rather than the cumulative (xy+yz)/sum, which is the
    // client's own arithmetic and skews the split; it is reproduced as it stands.
    // The chosen branch writes a +/-1.0f (`0xbf800000`, flipped to `0x3f800000`
    // when a second draw is not below the 0.5 at `0x8d5894`) into the slot the
    // tail multiplies by the face's own basis row, and two `2/32767 - 1` draws
    // into the other two. `0x727991`-`0x727a41` is the basis multiply, so the
    // position is a point on the box the nine floats at +0xc0 span, written to
    // node+0x10 with no direction — id 121's birth flow calls the direction slot
    // afterwards, so the cone still applies. Ids 122 and 125 never read the flag.
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
            121 => (box_surface_sample(rng, half), None),
            120 => {
                // One draw, and the direction is the position: the client builds
                // node+0x44 and node+0x10 from the same vector.
                let direction =
                    to_emitter_local(particle, context, surface_cone(rng, particle.angle));
                (direction, Some(direction))
            }
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
            let (axis, perp) = emission_axis(particle, context);
            if particle.angle != 0. {
                cone_direction(axis, perp, particle.angle, rng)
            } else {
                axis
            }
        }
    };
    // Both fields are authored ranges, and the editor wrote a good many of them
    // backwards: 724 of the corpus's 12,360 particle elements put the larger
    // value in `ScaleMin`. Reading the pair as an unordered interval is what
    // reproduces the authored sizes; feeding `range(low, high)` a reversed pair
    // still yields a value inside the interval, so the visible symptom is not a
    // wrong size but a silently wrong distribution — except at the ends, where
    // `ScaleMax: 0` makes `range` return exactly 0 and the particle is dropped
    // by the non-positive guard in `quad_corners`. Normalising the pair is the
    // reading that keeps every authored particle on screen.
    let (scale_low, scale_high) = if particle.scale_min <= particle.scale_max {
        (particle.scale_min, particle.scale_max)
    } else {
        (particle.scale_max, particle.scale_min)
    };
    let (rot_low, rot_high) = if particle.rot_min <= particle.rot_max {
        (particle.rot_min, particle.rot_max)
    } else {
        (particle.rot_max, particle.rot_min)
    };
    let scale = rng.range(scale_low, scale_high);
    let rotation = rng.range(rot_low, rot_high);
    ParticleState {
        age: 0.,
        ttl: particle.ttl,
        rotation,
        color: random_color(particle, rng),
        position,
        previous: position,
        direction,
        displacement: Vec3::ZERO,
        origin: position,
        acc_speed: 0.,
        speed: particle.speed,
        scale,
        color_remainder: [0.; 4],
        seed: rng.next_u32(),
    }
}

/// The emission direction and the perpendicular axis its cone is built from, in
/// this player's local frame.
///
/// Both come from the direction slot `0x723440`, which every emitter vtable
/// shares and which the classes call with `&node+0x44`: ids 121 and 124 from
/// their birth flows (`0x729bc1`), id 123 from the end of its *volume* sampler
/// `0x728270` (`mov (%esi),%edx` then `call *0x4(%edx)` at `0x72840b`–`0x728413`,
/// with `add $0x44,%eax` at `0x72840d` naming the destination). Only id 123's
/// surface sampler writes the direction itself, which is why the `inward` case
/// in `spawn` bypasses this.
///
/// `0x723447` tests `IsBind` at emitter+0x85. A bound emitter gets the global Z
/// axis `0x9c0338` copied verbatim (`0x723451`, `0x723503`); an unbound one gets
/// `rotate(Z, quat(emitter+0x74))` (`0x72345b`–`0x7234ff`). The cone's own axis
/// is the global X `0x9c0358` when bound (`0x723556`) and `rotate(X, q)` when
/// unbound (`0x723560`), perpendicular to the direction either way.
///
/// `ParIniDir` is not part of it. The slot never reads emitter+0xa0, where the
/// parser composes `Euler(ParIniDir) * quat(+0x74)` (`0x7243b8`); those four
/// floats go to node+0x34, the particle's initial orientation — `0x7283f5`–
/// `0x728408` copies them inside the same volume sampler — and this player models
/// particle orientation as the 2D spin in `rotation` instead. Reading `ParIniDir`
/// as a direction was wrong for the 100 records that author a non-zero one, and
/// the Y fallback was wrong for the 9,326 that author zero.
///
/// Both of the client's vectors live in the emitter's frame, so `spin` — the
/// rotation `placement` adds on top of the parent basis — undoes them. For an
/// unbound emitter `q` is `Q * keypoint Direction` and the parent basis is `Q`,
/// which leaves `orbit^-1 * Z`; the orbit is a rotation about Z, so the direction
/// is the local Z exactly. For a bound one the client's Z is bone local while
/// this player's local frame carries the keypoint direction too, hence the
/// inverse spin rather than Z outright: 2,770 of the corpus's 9,142 bound
/// particle keypoints (30.3%) author a non-identity `Direction`.
fn emission_axis(particle: &Particle, context: &ElementContext) -> (Vec3, Vec3) {
    (
        to_emitter_local(particle, context, Vec3::Z),
        to_emitter_local(particle, context, Vec3::X),
    )
}

/// A vector the client built in the emitter's frame, brought into this player's
/// local frame.
///
/// A bound emitter works in bone-local axes, so its vector passes through; an
/// unbound one carries the keypoint direction too, and `placement` adds `spin` on
/// top of the parent basis, so the inverse spin undoes both. Every sampler that
/// writes a direction goes through here — the shared direction slot's basis pair,
/// and id 120's cone.
fn to_emitter_local(particle: &Particle, context: &ElementContext, vector: Vec3) -> Vec3 {
    let forward = if particle.is_bind {
        vector
    } else {
        context.keypoint_direction * vector
    };
    context.spin.conjugate() * forward
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
fn cone_direction(axis: Vec3, perp: Vec3, angle: f32, rng: &mut Rng) -> Vec3 {
    // Draw order matches `0x723440`: the polar draw comes first (`0x723541`,
    // scaled by `Angle` and the 1/32767 at `0x8e6f10`), the azimuth second
    // (`0x723626`, scaled by the 2*pi/32767 at `0x8e6f14`).
    let polar = rng.unit() * angle;
    let azimuth = rng.unit() * std::f32::consts::TAU;
    cone_sample(axis, perp, polar, azimuth)
}

/// The cone's construction, drawn out. `0x72363b` turns the azimuth into an
/// axis-angle quaternion about the emission direction, `0x723661`–`0x7236e9`
/// rotate the perpendicular axis by it, `0x723718` turns the polar angle into a
/// quaternion about that spun axis, and `0x723741`–`0x7237c1` tilt the emission
/// direction by it and copy the result into node+0x44 (`0x7237d0`). Spinning the
/// perpendicular first is what makes the azimuth uniform around the direction,
/// and tilting by the polar angle leaves the sample exactly `polar` away from it.
fn cone_sample(axis: Vec3, perp: Vec3, polar: f32, azimuth: f32) -> Vec3 {
    let axis = if axis.length_squared() > 1e-9 {
        axis.normalize()
    } else {
        Vec3::Z
    };
    let perp = if perp.length_squared() > 1e-9 {
        perp.normalize()
    } else {
        Vec3::X
    };
    let spun = Quat::from_axis_angle(axis, azimuth) * perp;
    Quat::from_axis_angle(spun, polar) * axis
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

/// Id 120's surface direction, in the emitter's frame, from a single draw.
///
/// `0x7272aa` draws one uniform, `0x7272bd` scales it by the 2*pi/32767 at
/// `0x8e6f14`, and `0x7b6fd0` at `0x7272d1` turns it into a quaternion about the
/// global Z `0x9c0338`. `0x7272ec`-`0x727376` rotate the global X `0x9c0358` by
/// that quaternion — the spun perpendicular. `0x72737b` takes `pi/2 - Angle`
/// (`0x8ee228` minus emitter+0x10) and `0x727395` builds a second quaternion
/// about the spun axis. `0x72739a`-`0x727445` multiply the two, tilt first and
/// azimuth second, `0x70a320` at `0x727448` stores the product in node+0x34, and
/// `0x727552`-`0x7275f0` rotate the global Y `0x9c0348` by it into node+0x44.
///
/// Composed, that is `(-sin Angle * sin theta, sin Angle * cos theta, cos
/// Angle)`, checked numerically against the client's chain to 3e-16: every
/// particle sits at polar angle `Angle` exactly, on the surface of the cone. The
/// shared direction slot instead draws its polar angle uniformly inside
/// `[0, Angle]`, which fills the cone.
fn surface_cone(rng: &mut Rng, angle: f32) -> Vec3 {
    let tau = std::f32::consts::TAU;
    let azimuth = Quat::from_axis_angle(Vec3::Z, rng.unit() * tau);
    let tilt = Quat::from_axis_angle(azimuth * Vec3::X, std::f32::consts::FRAC_PI_2 - angle);
    (tilt * azimuth) * Vec3::Y
}

/// Id 121's surface sample: a point on the face of the box `half` describes.
///
/// `0x727800` spends four draws on every particle, in this order. The first
/// chooses the face: `0x72780d`-`0x727843` build the three face products of
/// `AreaSize` and their sum, and the draw is compared against `xy/sum`
/// (`0x727857`-`0x727868`, the Z faces) and then against `yz/sum` on its own
/// (`0x7278d6`-`0x7278df`, the X faces), falling through to the Y faces. That
/// second comparison is not the cumulative ratio, so the X faces are reachable
/// only when `z > x` and a cube never emits from them at all; it is what the
/// client does. The second draw picks
/// the sign of the face's own coordinate, `-1.0` unless the draw is not below the
/// 0.5 at `0x8d5894` (`0x727884`, `0x7278f9`, `0x72794c`). The third and fourth
/// are `2/32767 - 1` uniforms for the two in-face coordinates, and which slot
/// each lands in depends on the branch (`0x7278af`/`0x7278c9`, `0x727924`,
/// `0x727977`).
///
/// The tail at `0x727991` multiplies all three by the basis rows at emitter+0xc0,
/// +0xcc and +0xd8 and writes node+0x10, which for this player is the
/// componentwise product with `half`.
///
/// A zero extent makes every ratio 0/0 in the client, both comparisons false, and
/// the Y-face branch runs with a zero basis — the same branch the guard below
/// picks, and the same zero position.
fn box_surface_sample(rng: &mut Rng, half: Vec3) -> Vec3 {
    let xy = half.x * half.y;
    let yz = half.y * half.z;
    let xz = half.x * half.z;
    let total = xy + yz + xz;
    let pick = rng.unit();
    let sign = if rng.unit() >= 0.5 { 1. } else { -1. };
    let first = rng.signed();
    let second = rng.signed();
    let unit = if total > 0. && pick < xy / total {
        Vec3::new(first, second, sign)
    } else if total > 0. && pick < yz / total {
        Vec3::new(sign, first, second)
    } else {
        Vec3::new(first, sign, second)
    };
    unit * half
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
                // `orig` rather than `position`: the element-level path assigns
                // its noise (`state.noise = ...`), so a particle must not
                // accumulate it. Recomputing from the birth point each frame
                // gives the same value a fresh particle would have, which is
                // what makes `Amplitude` a wobble instead of a drift.
                let amplitude = *amplitude * NOISE_CM_PER_UNIT;
                let offset = Vec3::new(sample, sample * 0.5, sample * 0.25) * amplitude;
                state.position = state.origin + state.displacement + offset;
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

/// The trail's per-frame ring update, `0x713e90`. Element 152 shares the fractal
/// strip geometry with element 150, but which vertices are live is decided here.
///
/// `isappend` picks between two paths, and the corpus uses both: 138 of the 156
/// trail elements author 1 and 18 author 0. Clear, and `0x713ed4` rebuilds the
/// ring to exactly the authored vertex count whenever it is not already full,
/// every record born that frame with age 0, and no aging pass ever runs — a
/// permanent static strip. Set, and `0x713ebc` calls the append pass `0x714030`
/// followed by the aging pass `0x713f60`, so vertices are laid down over time and
/// then expire.
fn update_trail(
    fractal: &mut FractalState,
    trail: &Trail,
    element: &Element,
    dt_ms: f32,
    segments: usize,
) {
    let life = trail.vertices_life.max(0.);
    let ring = &mut fractal.trail;
    if !trail.is_append {
        ring.fill(segments, life);
        return;
    }
    // `0x714030` lays one vertex down per `interval / count` milliseconds. That
    // interval is `*(*(trail+0x28)+0x34)`: two dereferences through a field that
    // is only ever read in this region and never written, so the object owning it
    // is not identified and the number cannot be quoted. The element's own
    // authored duration is the closest recoverable equivalent — it is the window
    // the keypoints give this element, and 140 of the 156 corpus trails author a
    // finite one.
    let interval = match element.duration_ms() {
        Some(interval) if interval > 0. => interval,
        // The other 16 run a forever keypoint, so no rate can be recovered for
        // them, and one of those asks to append. Inventing a rate would either
        // freeze the trail or make it expire and never return, so these fall back
        // to the static strip the non-append path builds — which is what they
        // drew before this pass as well.
        _ => {
            ring.fill(segments, life);
            return;
        }
    };
    if segments == 0 {
        return;
    }
    // element+0x314 accumulates the frame delta; `0x714067` needs it strictly
    // past one step before anything is laid down, and `0x714072` gives up once
    // the whole strip is down. Both give up on the append only — the caller
    // `0x713e90` ages the ring and runs the expiry pass afterwards whatever the
    // append did, so a fully laid trail still fades away.
    ring.accumulator += dt_ms;
    let step = interval / segments as f32;
    if ring.laid < segments && ring.accumulator > step {
        // `0x714072`: as many vertices as the accumulated time covers, clamped to
        // the authored count at `0x714088`, and the accumulator keeps the
        // remainder (`0x71409d`), so a long frame lays several vertices instead
        // of one.
        let steps = (ring.accumulator / step) as usize;
        let before = ring.laid;
        ring.laid = (ring.laid + steps).min(segments);
        ring.accumulator %= step;
        for index in before..ring.laid {
            ring.push(TrailVertex {
                index: index as u32,
                life,
                age: 0.,
            });
        }
    }
    ring.age(dt_ms);
    if trail.is_tail_disappear {
        ring.drop_expired();
    }
}

/// What the client's per-element "reset at the end of the loop" virtual throws
/// away, for the part of it the viewer can reproduce without guessing.
///
/// Slot 17 of the element vtable. Ten of the fourteen element classes leave it
/// at the base `0x6fa640`, which zeroes the texture-animation clocks at +0xac,
/// +0xbc, +0xc0 and +0x128 when `ResetLoopEnd` (+0x13d) is set *and* the element
/// has a multi-frame flipbook (+0x133, written by the common parser at
/// `0x6fa161` and clear only when `TexRow` and `TexCol` are both 1). The viewer
/// needs no code for that half: its flipbook index is derived from the element
/// clock, which a restart already rewinds.
///
/// Four classes override the slot, each identifiable from the id its constructor
/// writes next to the vtable it installs. The trail's is decoded exactly:
/// `0x714620` calls the base and then zeroes the ring's accumulator (+0x314),
/// laid count (+0x318) and live count (+0x328) under the same `ResetLoopEnd`
/// test, so a looping trail starts its strip again from nothing instead of
/// carrying the previous loop's vertices into the next one. 98 of the 156 corpus
/// trails author that flag non-zero. That is the part implemented here.
///
/// The other three are attributed but not implemented, because the call site is
/// not recovered and the gate is therefore ambiguous. The ribbon (110, vtable
/// `0x8ec474`, installed at `0x6f9353` beside `movl $0x6e,0xc`) zeroes its
/// vertex-ring count at +0x14c and sets a byte at +0x18c (`0x70e3c0`); the child
/// reference (200, vtable `0x8ed690`) calls a virtual on the effect it owns at
/// +0x144 with a zero argument, gated by a byte at +0x14d (`0x7121b0`); and the
/// particle emitter (120 to 125, vtable `0x8edab4`) frees both of its node lists
/// at +0x168 and +0x188, sets a byte at +0x20c and zeroes +0x1a8 (`0x715600`).
/// None of the three tests `ResetLoopEnd` itself — only the base and the trail
/// do — so either the caller gates the whole slot and those three are
/// unconditional once it fires, or the caller does not gate and ribbons, children
/// and particle emitters restart at every loop end whatever the file says. The
/// slot is reached through a computed pointer rather than a `call *0x44`, so
/// neither reading can be settled from the call site yet, and emptying a ribbon
/// or a particle emitter on the wrong reading would be visible on every looping
/// effect in the corpus. Both stay as they are until that is resolved.
fn reset_at_loop_end(runtime: &mut Runtime) {
    if let Runtime::Fractal(fractal) = runtime {
        fractal.trail = TrailRing::default();
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
    // `Segs` counts cross-sections, not quads. The client's strip builder
    // divides the draw index by `+0x2b0` (which the loader fills from `Segs`)
    // and stops one short of it — `0x7092f9` passes `+0x2b0 - 1` where the loop
    // counter stops `jl` — so a `Segs: 10` ribbon is 9 quads spanning the full
    // axis, each one `1/10` of the length. Using `Segs` quads here drew one
    // extra section past the authored end.
    let segments = spec.segments.max(2) as usize;
    let quad_count = segments - 1;
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
    if let Some(trail) = trail {
        update_trail(fractal, trail, element, dt * 1000., segments);
    }
    let matrix = context.placement;
    let start = matrix.transform_point3(spec.start_position);
    let end = matrix.transform_point3(spec.end_position);
    let mut axis = end - start;
    // A strip whose authored ends coincide is not empty in the client. Its
    // direction routine (`0x709ca1`..`0x709d1c`) normalises `EndPos - StartPos`,
    // finds the tiny magnitude and takes the `jmp 0x709d48` branch, which leaves
    // the direction vector at zero and keeps drawing — the tapers then hang off
    // a zero-length axis. 502 of the 734 corpus fractals author
    // `StartPos == EndPos == 0.0, 0.0, 0.0`, and 63 of those placements are
    // installed in `world.ecwld` (for example
    // `gfx/场景/新种族地图/09黄色的电球.gfx`). Those effects are a tapered point,
    // not an absent element; aborting here is what silently dropped them.
    let degenerate = axis.length_squared() < 1e-12;
    if degenerate {
        // Keep the strip visible by giving the collapsed axis the direction the
        // client falls back to when a caller hands it a zero vector. The
        // authored shape (three-point tapers below) is preserved, so the patch
        // is the same silhouette the authored widths describe.
        axis = Vec3::Z;
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
    // Element 152 draws only the ring's live records, oldest first, each still
    // carrying its place in the authored strip; element 150 has no ring and draws
    // the whole strip, which is what a full ring amounts to anyway.
    let (first, drawn) = if trail.is_some() {
        match fractal.trail.records.first() {
            Some(oldest) => (oldest.index as usize, fractal.trail.records.len()),
            None => return,
        }
    } else {
        (0, segments)
    };
    for slot in 0..drawn {
        let index = first + slot;
        if index >= quad_count {
            break;
        }
        let t0 = index as f32 / segments as f32;
        let t1 = (index + 1) as f32 / segments as f32;
        let a = start.lerp(end, t0) + fractal.offsets[index];
        let b = start.lerp(end, t1) + fractal.offsets[index + 1];
        let width0 = spec.width_at(t0).max(0.) * context.scale * 0.5;
        let width1 = spec.width_at(t1).max(0.) * context.scale * 0.5;
        let alpha0 = spec.alpha_at(t0);
        let mut alpha1 = spec.alpha_at(t1);
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

    /// A grid is a lattice of authored vertices, so a 2x4 grid must produce a
    /// 1x3 sheet at the authored corners — not a 2x4 block of `fGridSize` cells
    /// lying on the floor, which is what the waterfall rendered as: a motionless
    /// white rectangle. And because `TexSpeed` scrolls its map, that sheet must
    /// move every frame.
    #[test]
    fn a_grid_builds_its_authored_sheet_and_scrolls_it() {
        let grid = "GFXELEMENTID: 210\nName: curtain\nSrcBlend: 5\nDestBlend: 2\n\
                   TexFile: 物体\\waterfall.dds\nTexRow: 1\nTexCol: 1\n\
                   TexSpeed: -0.400000\nwNumber: 2\nhNumber: 4\n\
                   -58.299149 35.844849 9.029253\ndwColor: ffffffff\n\
                   -34.900089 35.844849 9.029253\ndwColor: ffffffff\n\
                   -58.299149 22.257586 9.029253\ndwColor: ffffffff\n\
                   -34.900089 22.257586 9.029253\ndwColor: ffffffff\n\
                   -58.299149 12.348745 9.029253\ndwColor: ffffffff\n\
                   -34.900089 12.348745 9.029253\ndwColor: ffffffff\n\
                   -58.299149 -0.006341 9.029253\ndwColor: 88caff\n\
                   -34.900089 -0.006341 9.029253\ndwColor: 88caff\n\
                   fGridSize: 0.500000\nfZOffset: 0.000000\n\
                   StartTime: 0\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\n\
                   TimeSpan: -1\nPosition: 0.000000, 0.000000, 0.000000\n\
                   Color: -13261313\nScale: 1.000000\n\
                   Direction: 0.000000, 0.000000, 0.000000, 1.000000\n\
                   Rad_2D: 0.000000\nCtrlMethodCount: 0\n";
        let mut player = build(&(header37(1) + grid));
        step(&mut player, 0.);
        let quads = player.quads().to_vec();
        assert_eq!(
            quads.len(),
            3,
            "a 2x4 lattice is one quad column, three tall"
        );

        // The authored extent, in element space: 23.4 wide and 35.85 tall.
        let (mut lo_y, mut hi_y, mut lo_x, mut hi_x) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for quad in &quads {
            for point in &quad.positions {
                lo_y = lo_y.min(point[1]);
                hi_y = hi_y.max(point[1]);
                lo_x = lo_x.min(point[0]);
                hi_x = hi_x.max(point[0]);
            }
        }
        // The fixture's `DedaultScale` is 2, so the authored extent is doubled;
        // the *ratio* is what proves the authored corners were used, since a
        // `fGridSize` patch would be 1.0 by 1.5 and never 23.4 by 35.85.
        let scale = 2.;
        assert!(
            (hi_y - lo_y - 35.85 * scale).abs() < 0.1,
            "the curtain hangs {} units, not the authored {}",
            hi_y - lo_y,
            35.85 * scale
        );
        assert!((hi_x - lo_x - 23.4 * scale).abs() < 0.1);
        assert!(
            (hi_y - lo_y) / (hi_x - lo_x) > 1.4,
            "the sheet must be taller than it is wide, as authored"
        );

        // `TexSpeed` scrolls V, so no two frames may share the same uv.
        let first = quads[0].uv;
        let mut moved = 0;
        for _ in 0..10 {
            step(&mut player, 33.);
            if player.quads()[0].uv != first {
                moved += 1;
            }
        }
        assert_eq!(moved, 10, "the sheet must scroll on every frame");

        // Ten frames is a blink; a player that scrolls briefly and then stops
        // reads as a still image to anyone watching. Run the clock for twenty
        // seconds and require the scroll to still be advancing at the end, with
        // no frame repeating the one before it.
        let mut stalled = 0;
        let mut previous = player.quads()[0].uv;
        for _ in 0..600 {
            step(&mut player, 33.);
            let now = player.quads()[0].uv;
            if now == previous {
                stalled += 1;
            }
            previous = now;
        }
        assert_eq!(
            stalled, 0,
            "the curtain stopped scrolling {stalled} times over twenty seconds; \
             a waterfall that stalls is the static sheet the report describes"
        );
    }

    /// `A3DGridDecalEx::_setUVCoord` spreads **one** texture rect across the
    /// whole lattice — U from `m_fTexU` in `m_nWidthVertNum - 1` steps and V
    /// from `m_fTexV` in `m_nHeightVertNum - 1` — so a 2x3 lattice samples half
    /// the sheet per row. Handing every cell the whole rect, which is what the
    /// code did, stacked one copy of the image per row and restarted the
    /// `TexSpeed` scroll at every row boundary.
    #[test]
    fn a_grid_spreads_one_texture_rect_over_its_lattice() {
        let grid = "GFXELEMENTID: 210\nName: curtain\nSrcBlend: 5\nDestBlend: 2\n\
                    TexFile: 物体\\\\waterfall.dds\nTexRow: 1\nTexCol: 1\n\
                    TexSpeed: -0.400000\nwNumber: 2\nhNumber: 3\n\
                    -1.000000 20.000000 0.000000\ndwColor: ffffffff\n\
                    1.000000 20.000000 0.000000\ndwColor: ffffffff\n\
                    -1.000000 10.000000 0.000000\ndwColor: ffffffff\n\
                    1.000000 10.000000 0.000000\ndwColor: ffffffff\n\
                    -1.000000 0.000000 0.000000\ndwColor: ffffffff\n\
                    1.000000 0.000000 0.000000\ndwColor: ffffffff\n\
                    fGridSize: 0.500000\nfZOffset: 0.000000\n\
                    StartTime: 0\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\n\
                    TimeSpan: -1\nPosition: 0.000000, 0.000000, 0.000000\n\
                    Color: -1\nScale: 1.000000\n\
                    Direction: 0.000000, 0.000000, 0.000000, 1.000000\n\
                    Rad_2D: 0.000000\nCtrlMethodCount: 0\n";
        let mut player = build(&(header37(1) + grid));
        step(&mut player, 0.);
        let quads = player.quads().to_vec();
        assert_eq!(quads.len(), 2, "a 2x3 lattice is one quad column, two tall");
        let (first, second) = (quads[0].uv, quads[1].uv);
        assert!(
            (second[0][1] - first[0][1] - 0.5).abs() < 1e-5,
            "the second row must start half a sheet lower, not on the same texel: \
             {first:?} then {second:?}"
        );
        assert!(
            (first[2][1] - first[0][1] - 0.5).abs() < 1e-5,
            "one row of a three-row lattice is half the sheet, not all of it: {first:?}"
        );
        assert!(
            (first[1][0] - 1.).abs() < 1e-5,
            "the single column still spans the full width: {first:?}"
        );
    }

    /// The element-140 record of a ring at radius 1, height 1, with the given
    /// `Pitch` and the editor's `Sects: 0`.
    fn ring_quads(pitch: f32) -> Vec<EffectQuad> {
        let ring = format!(
            "GFXELEMENTID: 140\r\nName: ring\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             TexFile: 圆环光\\circle032.dds\r\nRepeatCount: 1\r\nRepeatDelay: 0\r\n\
             Radius: 1.000000\r\nHeight: 1.000000\r\nPitch: {pitch:.6}\r\nSects: 0\r\n\
             NoRadScale: 0\r\nNoHeiScale: 0\r\nOrgAtCenter: 0\r\n\
             StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
             Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
             Rad_2D: 0\r\nCtrlMethodCount: 0\r\n"
        );
        let mut player = build(&(header37(1) + ring.as_str()));
        step(&mut player, 0.);
        player.quads().to_vec()
    }

    /// A ring is world geometry built in the parent frame, so the owner's
    /// placement scale reaches its radius through `GetParentTM()`, while a 100/101
    /// card is a screen-space quad that must not grow. The two paths have to
    /// disagree: this map places `场景/树林光束.gfx` at 8.776 with 0.5 x 10 cards,
    /// and scaling those drew 123-unit shafts over the tree tops, while the
    /// Nightfire `能量喷泉` particles at 5.019 really are five times bigger.
    #[test]
    fn a_ring_follows_the_owner_scale_where_a_sprite_does_not() {
        let ring = "GFXELEMENTID: 140\r\nName: ring\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             TexFile: 圆环光\\circle032.dds\r\nRepeatCount: 1\r\nRepeatDelay: 0\r\n\
             Radius: 1.000000\r\nHeight: 1.000000\r\nPitch: 0.000000\r\nSects: 0\r\n\
             NoRadScale: 0\r\nNoHeiScale: 0\r\nOrgAtCenter: 0\r\n\
             StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
             Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
             Rad_2D: 0\r\nCtrlMethodCount: 0\r\n";
        let radius = |scale: f32| {
            let mut player = build(&(header37(1) + ring));
            player
                .advance(0., &view(), Mat4::from_scale(Vec3::splat(scale)), scale, 1.)
                .unwrap();
            let point = player.quads()[0].positions[0];
            point[0].hypot(point[2])
        };
        let unit = radius(1.);
        let widened = radius(3.);
        assert!(unit > 0., "the ring has a radius to compare");
        assert!(
            (widened - unit * 3.).abs() < 1e-3,
            "a ring is world geometry and follows the placement scale: {widened} from {unit}"
        );
    }

    /// `Sects: 0` is the client's `_default_sects`, and `A3DGFXRing.cpp` sets
    /// that to 36 (`if (m_nSects) nSect = m_nSects; else nSect = _default_sects;`).
    /// The editor writes the zero on 1,064 of the corpus's 1,116 element-140
    /// records, so clamping it up to three turned nearly every ring in the game
    /// into a three-sided prism.
    #[test]
    fn a_ring_authored_without_sects_is_thirty_six_sections() {
        assert_eq!(
            ring_quads(0.).len(),
            36,
            "Sects 0 must fall back to the client's 36 sections, not three"
        );
    }

    /// `Pitch` is a texture pitch, spent as `tan(pitch)` on the band's two
    /// radii after clamping to `radius / halfHeight`: the foot is wider and the
    /// roof narrower. The code used it as an angular offset of the first
    /// vertex, which turned the band on its axis and left both rows the same
    /// size.
    #[test]
    fn a_ring_pitch_tilts_the_band_through_its_radii() {
        let radius = |point: [f32; 3]| point[0].hypot(point[2]);
        let rows = |quads: &[EffectQuad]| {
            let quad = &quads[0];
            // Corners 0 and 1 are the foot row, 2 and 3 the roof.
            (
                (radius(quad.positions[0]) + radius(quad.positions[1])) * 0.5,
                (radius(quad.positions[2]) + radius(quad.positions[3])) * 0.5,
            )
        };
        let (flat_foot, flat_roof) = rows(&ring_quads(0.));
        assert!(
            (flat_foot - flat_roof).abs() < 1e-4,
            "a level ring keeps one radius: {flat_foot} against {flat_roof}"
        );
        // atan(0.5) gives `tan = 0.5`, so the rows differ by
        // `halfHeight * tan` = 0.25 on a height of 1: half the radius either way.
        let (foot, roof) = rows(&ring_quads(0.463_648));
        assert!(
            foot > flat_foot && roof < flat_roof,
            "pitch must widen the foot and narrow the roof: {foot} / {roof}"
        );
        assert!(
            ((foot - roof) / flat_foot - 0.5).abs() < 1e-3,
            "the two rows must differ by two half-height tangents, half the \
             radius here: {foot} against {roof}"
        );
    }

    /// Controller 107's noise is a displacement of the particle's birth point,
    /// not a per-frame step. Adding it every frame compounded: `夜空飘尘`'s
    /// `Amplitude: 400` walked its 0.4-unit dust out past +/-7,900 units inside
    /// forty frames, which is the effect that covers a whole map.
    #[test]
    fn a_particle_noise_controller_does_not_accumulate() {
        let fractal = "GFXELEMENTID: 121\r\nName: dust\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             TexFile: 粒子\\particle001.dds\r\nQuota: -1\r\nParticleWidth: 0.4\r\n\
             ParticleHeight: 0.4\r\nEmissionRate: 100\r\nSpeed: 0.1\r\nTTL: 10\r\n\
             ColorMin: -1\r\nColorMax: -1\r\nScaleMin: 0.2\r\nScaleMax: 0.1\r\n\
             RotMin: 0\r\nRotMax: 0\r\nIsSurface: 0\r\nIsBind: 0\r\n\
             AreaSize: 1.0, 1.0, 1.0\r\nAffectorCount: 1\r\nCtrlType: 107\r\n\
             StartTime: 0\r\nEndTime: -1\r\nBufLen: 256\r\nAmplitude: 400\r\n\
             WaveLen: 100\r\nPersistence: 0.25\r\nOctaveNum: 5\r\n\
             BaseColor: -1\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\n\
             TimeSpan: -1\r\nPosition: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\n\
             Direction: 0, 0, 0, 1\r\nRad_2D: 0\r\nCtrlMethodCount: 0\r\n";
        let mut player = build(&(header37(1) + fractal));
        let mut worst = 0f32;
        for _ in 0..60 {
            step(&mut player, 33.);
            for quad in player.quads() {
                for point in &quad.positions {
                    worst = worst.max(point[0].abs().max(point[1].abs()).max(point[2].abs()));
                }
            }
        }
        // `Amplitude` 400 with the birth point held still stays within a few
        // hundred units; the compounding bug reached thousands by frame 30.
        assert!(
            worst < 900.,
            "noise accumulated to {worst} units; it must stay near the birth point"
        );
    }

    /// Controller 107's `Amplitude` is centimetres, and the parser keeps the
    /// effect inside its own bounds because of it.
    ///
    /// The conversion was removed once, on the reasoning that a large authored
    /// number must be a large world distance. It is not: read as world units the
    /// sample throws the element thousands of units outside a quarter-unit root,
    /// which is exactly the "effects play fast and run out of bounds" report.
    /// The corpus measurement that settles it is in `NOISE_CM_PER_UNIT`; this
    /// pins the observable consequence, so the conversion cannot be dropped
    /// again without this failing.
    #[test]
    fn a_controller_107_amplitude_stays_inside_its_own_bounds() {
        // A particle in a one-unit area shaken by the authored 400. As world
        // units that is 400 times the volume it lives in.
        let fractal = "GFXELEMENTID: 121\r\nName: dust\r\nSrcBlend: 5\r\nDestBlend: 2\r\nTexFile: 粒子\\particle001.dds\r\nQuota: -1\r\nParticleWidth: 0.4\r\nParticleHeight: 0.4\r\nEmissionRate: 100\r\nSpeed: 0.1\r\nTTL: 10\r\nColorMin: -1\r\nColorMax: -1\r\nScaleMin: 0.2\r\nScaleMax: 0.1\r\nRotMin: 0\r\nRotMax: 0\r\nIsSurface: 0\r\nIsBind: 0\r\nAreaSize: 1.0, 1.0, 1.0\r\nAffectorCount: 1\r\nCtrlType: 107\r\nStartTime: 0\r\nEndTime: -1\r\nBufLen: 256\r\nAmplitude: 400\r\nWaveLen: 100\r\nPersistence: 0.25\r\nOctaveNum: 5\r\nBaseColor: -1\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\nPosition: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\nRad_2D: 0\r\nCtrlMethodCount: 0\r\n";
        let mut player = build(&(header37(1) + fractal));
        let mut peak = 0f32;
        for _ in 0..40 {
            step(&mut player, 33.);
            for quad in player.quads() {
                for point in &quad.positions {
                    peak = peak.max(point[0].abs().max(point[2].abs()));
                }
            }
        }
        // As centimetres the sample wanders a few units — a wobble inside the
        // volume. Read as world units it leaves them by a factor of hundreds.
        assert!(
            peak < 20.,
            "the authored amplitude 400 moved the sprite {peak} units, so the \
             centimetre conversion is missing and the effect is out of bounds"
        );
    }

    /// 724 of 12,360 particle elements author `ScaleMin` larger than `ScaleMax`
    /// — `夜空飘尘` writes `ScaleMin: 0.2` against `ScaleMax: 0.1`. Reading the
    /// pair as an unordered interval is what keeps the authored spread; feeding
    /// a reversed pair to `range` skews the distribution and collapses it to
    /// zero at the ends, which drops the particle entirely.
    #[test]
    fn an_inverted_scale_range_is_read_as_an_interval() {
        let mut rng = Rng::new(11);
        // The authored pair from `夜空飘尘`, inverted.
        let (low, high) = if 0.2f32 <= 0.1f32 {
            (0.2, 0.1)
        } else {
            (0.1, 0.2)
        };
        let mut seen_low = false;
        let mut seen_high = false;
        for _ in 0..256 {
            let value = rng.range(low, high);
            assert!(
                (0.1..=0.2).contains(&value),
                "a value of {value} escaped the authored interval"
            );
            if value < 0.12 {
                seen_low = true;
            }
            if value > 0.18 {
                seen_high = true;
            }
        }
        assert!(
            seen_low && seen_high,
            "the whole interval must be reachable"
        );
    }

    /// An empty `TexFile` is *not* drawable geometry. 3,459 of the corpus's
    /// elements author no map, and the GPU pass substitutes a flat opaque white
    /// quad for a texture it cannot read, so drawing them turns whole maps
    /// white: `冲击波1` alone is twelve textureless plasma particles at
    /// `ParticleWidth: 1`, and every one of them landed on screen as a white
    /// slab. Dropping the element instead costs nothing visible.
    #[test]
    fn an_element_with_no_texfile_draws_nothing() {
        let untextured = (header37(1) + SPRITE).replace("TexFile: 粒子\\glow.dds", "TexFile: ");
        let mut player = build(&untextured);
        step(&mut player, 0.);
        assert!(
            player.quads().is_empty(),
            "a textureless element must not become a flat white quad"
        );
    }

    /// `IsDummy` marks an element the client builds but never submits, and it is
    /// the *only* such flag: `DummyEle` names another element to inherit a basis
    /// from, so gating on it deletes real geometry — the `场景鱼群` shoal and the
    /// `水瀑术_手轨迹` particle are both `IsDummy: 0` with empty `DummyEle`.
    #[test]
    fn a_dummy_element_is_built_but_never_drawn() {
        let dummy = (header37(1) + SPRITE).replace("IsDummy: 0", "IsDummy: 1");
        let mut player = build(&dummy);
        step(&mut player, 0.);
        assert!(player.quads().is_empty());
        assert!(player.stats().dummies > 0, "the element was still built");

        // Same root, flag cleared: it draws again.
        let mut live = build(&(header37(1) + SPRITE));
        step(&mut live, 0.);
        assert_eq!(live.quads().len(), 1);
    }

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

    /// A fractal (element 150) at version 79 with the authored ends, widths and
    /// alphas interpolated in. `Segs: 4` gives three quads over four
    /// cross-sections, matching the client's `+0x2b0 - 1` loop bound.
    fn fractal(start: &str, end: &str, widths: [f32; 3], alphas: [f32; 3]) -> String {
        format!(
            "{}GFXELEMENTID: 150\r\nName: strip\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             RepeatCount: 1\r\nRepeatDelay: 0\r\nTexFile: 粒子\\strip.dds\r\nBindEle: \r\n\
             ZEnable: 0\r\nMatchGrnd: 0\r\nTexRow: 1\r\nTexCol: 1\r\nTexInterval: 50\r\n\
             Priority: 0\r\nIsDummy: 0\r\nDummyEle: \r\nWarp: 0\r\nTileMode: 0\r\n\
             TexSpeed: 0\r\nTexSpeed: 0\r\nUReverse: 0\r\nVReverse: 0\r\nUVExchg: 0\r\n\
             RenderLayer: 0\r\nNoDownSample: 0\r\nResetLoopEnd: 0\r\nTexAnimMaxTime: 0\r\n\
             BufLen: 256\r\nAmplitude: 1.000000\r\nWaveLen: 1\r\nPersistence: 1.000000\r\n\
             OctaveNum: 1\r\nStartPos: {start}\r\nEndPos: {end}\r\nSegs: 4\r\nLightNum: 0\r\n\
             WaveLen: 1.000000\r\nInterval: 100\r\nWidth: {}\r\nWidth: {}\r\nWidth: {}\r\n\
             Alpha: {}\r\nAlpha: {}\r\nAlpha: {}\r\nAmplitude: 0.000000\r\nPos1Enable: 0\r\n\
             Pos2Enable: 0\r\nUseNormal: 0\r\nNormal: 0, 0, 0\r\nFilter: 1\r\n\
             StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
             Position: 0, 0, 0\r\nColor: -1\r\nScale: 1.000000\r\n\
             Direction: 0, 0, 0, 1\r\nRad_2D: 0\r\nCtrlMethodCount: 0\r\n",
            header79(1),
            widths[0],
            widths[1],
            widths[2],
            alphas[0],
            alphas[1],
            alphas[2],
        )
    }

    /// The three authored `Width` lines are `[end, midpoint, start]`, not
    /// `[head, mid, tail]`. The client's strip builder interpolates
    /// `Width[1] -> Width[0]` over the first half of the axis and
    /// `Width[2] -> Width[0]` over the second (disassembly `0x709a58`..`0x709aeb`),
    /// so a profile whose middle line is its widest draws a strip that pinches at
    /// both ends. Reading only the first two lines — which is what the emitter
    /// did before this was measured — loses the apex and 17.5% of the strip's
    /// area across the corpus.
    #[test]
    fn a_three_point_width_profile_peaks_in_the_middle_of_the_strip() {
        let mut spec = gfx_file::Fractal::default();
        spec.widths = vec![0.2, 1.0, 0.2];
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(near(spec.width_at(0.0), 0.2), "the first line is one end");
        assert!(near(spec.width_at(1.0), 0.2), "the third line is the other");
        assert!(
            near(spec.width_at(0.5), 1.0),
            "the second line is the midpoint"
        );
        assert!(
            near(spec.width_at(0.25), 0.6),
            "first half ramps end -> midpoint"
        );
        assert!(
            near(spec.width_at(0.75), 0.6),
            "second half midpoint -> start"
        );
        // A single authored line repeats, which collapses to a constant strip;
        // 356 corpus fractals author one width and read the same either way.
        spec.widths = vec![0.4];
        assert!(near(spec.width_at(0.75), 0.4));
        // Two lines interpolate end to start with no midpoint.
        spec.widths = vec![0.0, 2.0];
        assert!(near(spec.width_at(0.5), 1.0));
    }

    /// A fractal whose authored ends coincide still draws. The client's
    /// direction routine (`0x709ca1`..`0x709d1c`) sees the tiny `EndPos -
    /// StartPos`, leaves the direction at zero and jumps past the divide, so the
    /// strip keeps its authored tapers instead of vanishing. 502 of the 734
    /// corpus fractals author `StartPos == EndPos == 0`, and 63 of those
    /// placements are installed in the world.
    #[test]
    fn a_fractal_with_coincident_ends_still_draws_its_tapers() {
        let strip = fractal("0, 0, 0", "0, 0, 0", [0.0, 1.0, 0.0], [0.5, 1.0, 0.5]);
        let mut player = build(&strip);
        step(&mut player, 16.);
        let quads = player.quads();
        assert_eq!(quads.len(), 3, "four cross-sections, three quads");
        // The apex is authored in the middle, so the first quad is thinner than
        // the strip's own middle. A zero-length strip is not an empty one.
        let width = |quad: &EffectQuad| {
            let a = Vec3::from_array(quad.positions[0]);
            let b = Vec3::from_array(quad.positions[1]);
            (a - b).length()
        };
        assert!(width(&quads[0]) < width(&quads[1]), "the tapers survive");
        assert!(quads.iter().all(|q| q.color[3] > 0.), "and stay visible");
    }

    /// A model element (160) draws its meshes as triangle lists, transformed by
    /// the element's own placement, with the element's keypoint colour.
    ///
    /// The geometry is built at load time through the same chain the ECM path
    /// uses (`SMD -> BON + SKI -> bind pose`); here a minimal archive is handed
    /// in through the reader so the test needs no files on disk.
    #[test]
    fn a_model_element_draws_its_posed_meshes() {
        let source = format!(
            "{}GFXELEMENTID: 160\r\nName: crystal\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             RepeatCount: 1\r\nRepeatDelay: 0\r\nTexFile: \r\nBindEle: \r\nZEnable: 0\r\n\
             MatchGrnd: 0\r\nTexRow: 1\r\nTexCol: 1\r\nTexInterval: 50\r\nPriority: 0\r\n\
             IsDummy: 0\r\nDummyEle: \r\nWarp: 0\r\nTileMode: 0\r\nTexSpeed: 0\r\n\
             TexSpeed: 0\r\nUReverse: 0\r\nVReverse: 0\r\nUVExchg: 0\r\nRenderLayer: 0\r\n\
             NoDownSample: 0\r\nResetLoopEnd: 0\r\nTexAnimMaxTime: 0\r\n\
             ModelPath: 测试\\三角形.SMD\r\nModelActName: \r\nLoops: 1\r\nAlphaCmp: 0\r\n\
             WriteZ: 1\r\nUse3DCam: 0\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\n\
             InterpolateMode: 0\r\nTimeSpan: -1\r\nPosition: 0, 0, 0\r\nColor: -1\r\n\
             Scale: 1.000000\r\nDirection: 0, 0, 0, 1\r\nRad_2D: 0\r\nCtrlMethodCount: 0\r\n",
            header79(1),
        );
        let mut player = build(&source);
        // No archive attached: the element keeps counting and nothing is drawn.
        assert_eq!(player.models().len(), 0, "no reader, no meshes");
        step(&mut player, 16.);
        assert!(player.quads().is_empty(), "and nothing is invented");

        // An empty archive is a named warning, not a silent drop.
        let mut player = build(&source);
        let mut empty = |path: &str| -> Result<Vec<u8>, String> {
            Err(format!(
                "{} is not in the test archive",
                crate::ecmodel::normalize(path)
            ))
        };
        player.load_models("gfx/测试.gfx", &mut empty).unwrap();
        assert!(
            player
                .warnings()
                .iter()
                .any(|warning| warning.contains("is not in the test archive")),
            "a missing model is reported, not silently dropped"
        );

        // A one-triangle model: the mesh resolves and the element emits a
        // triangle list rather than four corners.
        let archive = one_triangle_model();
        let mut player = build(&source);
        let mut read = |path: &str| -> Result<Vec<u8>, String> {
            let name = crate::ecmodel::normalize(path);
            archive
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| format!("{name} is not in the test archive"))
        };
        player.load_models("gfx/测试.gfx", &mut read).unwrap();
        assert!(player.warnings().is_empty(), "{:?}", player.warnings());
        assert_eq!(player.models().len(), 1, "one mesh for one model element");
        step(&mut player, 16.);
        let meshes: Vec<&EffectMesh> = player
            .quads()
            .iter()
            .filter_map(|quad| quad.mesh.as_ref())
            .collect();
        assert_eq!(meshes.len(), 1, "the element draws one mesh");
        assert_eq!(meshes[0].vertices.len(), 3, "a triangle list, not a quad");
        assert!(
            player.quads().iter().any(|quad| quad.mesh.is_some()),
            "and the quad array carries it"
        );
    }

    /// The smallest archive that goes through the whole model chain: an `.smd`
    /// naming a `.bon` skeleton and a `.ski` skin, each one a single triangle
    /// with no texture of its own.
    ///
    /// Built from the parsers' own layouts rather than from the shipped files so
    /// the test does not depend on `google_drive`, and so a change in any of the
    /// three formats fails here.
    fn one_triangle_model() -> Vec<(String, Vec<u8>)> {
        // `.ski`: MOXB + IKSA version 8, one rigid mesh with three vertices.
        // Layout from `skin::Skin::parse`: version, skin/rigid/morph counts, the
        // texture, material and bone counts, two scalars, the supple and muscle
        // counts, 52 bytes of padding, then the texture names, the materials and
        // the meshes. A rigid mesh stores 32 bytes per vertex and carries an
        // explicit bone index.
        let mut ski = Vec::new();
        ski.extend_from_slice(b"MOXB");
        ski.extend_from_slice(b"IKSA");
        ski.extend_from_slice(&8u32.to_le_bytes()); // version
        ski.extend_from_slice(&0u32.to_le_bytes()); // skinned meshes
        ski.extend_from_slice(&1u32.to_le_bytes()); // rigid meshes
        ski.extend_from_slice(&0u32.to_le_bytes()); // morph skinned
        ski.extend_from_slice(&0u32.to_le_bytes()); // morph rigid
        ski.extend_from_slice(&0u32.to_le_bytes()); // textures
        ski.extend_from_slice(&1u32.to_le_bytes()); // materials
        ski.extend_from_slice(&1u32.to_le_bytes()); // bones
        ski.extend_from_slice(&0f32.to_le_bytes()); // min weight
        ski.extend_from_slice(&1i32.to_le_bytes()); // skeleton bones
        ski.extend_from_slice(&0u32.to_le_bytes()); // supple
        ski.extend_from_slice(&0u32.to_le_bytes()); // muscle
        ski.extend_from_slice(&[0u8; 52]);
        // The one material is a C string and four RGBA vectors.
        ski.extend_from_slice(b"MATERIAL \0");
        for _ in 0..4 {
            for value in [0f32, 0., 0., 1.] {
                ski.extend_from_slice(&value.to_le_bytes());
            }
        }
        ski.extend_from_slice(&1f32.to_le_bytes()); // power
        ski.push(0); // not two-sided
                     // The mesh: a length-prefixed name, its bone, texture and material
                     // indices, then its vertex and index counts.
        ski.extend_from_slice(&3u32.to_le_bytes());
        ski.extend_from_slice(b"tri");
        ski.extend_from_slice(&0i32.to_le_bytes()); // rigid bone 0
        ski.extend_from_slice(&(-1i32).to_le_bytes()); // no texture of its own
        ski.extend_from_slice(&0i32.to_le_bytes()); // material 0
        ski.extend_from_slice(&3u32.to_le_bytes()); // three vertices
        ski.extend_from_slice(&3u32.to_le_bytes()); // one triangle
        for index in 0..3u32 {
            let angle = index as f32 * 2.094_395;
            for value in [angle.cos(), angle.sin(), 0.] {
                ski.extend_from_slice(&value.to_le_bytes()); // position
            }
            for value in [0f32, 0., 1.] {
                ski.extend_from_slice(&value.to_le_bytes()); // normal
            }
            for value in [0f32, 0.] {
                ski.extend_from_slice(&value.to_le_bytes()); // uv
            }
        }
        for index in [0u16, 1, 2] {
            ski.extend_from_slice(&index.to_le_bytes());
        }

        // `.bon`: MOXB + EKSA version 6, one bone and one joint. Version 6 is the
        // newest the client accepts and the first that keeps its joint tracks out
        // of the file, so the fixture needs no keyframes.
        let mut bon = Vec::new();
        bon.extend_from_slice(b"MOXB");
        bon.extend_from_slice(b"EKSA");
        bon.extend_from_slice(&6u32.to_le_bytes()); // version
        bon.extend_from_slice(&1u32.to_le_bytes()); // one bone
        bon.extend_from_slice(&1u32.to_le_bytes()); // one joint
        bon.extend_from_slice(&0u32.to_le_bytes()); // no hangers
        bon.extend_from_slice(&0i32.to_le_bytes()); // start
        bon.extend_from_slice(&26i32.to_le_bytes()); // end
        bon.extend_from_slice(&30i32.to_le_bytes()); // fps
        bon.extend_from_slice(&[0u8; 64]);
        bon.extend_from_slice(&4u32.to_le_bytes()); // name length
        bon.extend_from_slice(b"bone");
        // The flag byte shares a four-byte word with three pad bytes; the parser
        // reads the whole word and keeps the first byte.
        bon.extend_from_slice(&0u32.to_le_bytes()); // flags: an ordinary joint
        bon.extend_from_slice(&(-1i32).to_le_bytes()); // root, no parent
        bon.extend_from_slice(&0i32.to_le_bytes()); // first joint
        bon.extend_from_slice(&0u32.to_le_bytes()); // no children
        let identity = [
            1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        for value in identity {
            bon.extend_from_slice(&value.to_le_bytes()); // relative
        }
        for value in identity {
            bon.extend_from_slice(&value.to_le_bytes()); // initial
        }
        // One joint: its class, a length-prefixed name, then parent, child and
        // sibling indices.
        bon.extend_from_slice(&15u32.to_le_bytes()); // joint class 15
        bon.extend_from_slice(&4u32.to_le_bytes());
        bon.extend_from_slice(b"bone");
        bon.extend_from_slice(&(-1i32).to_le_bytes());
        bon.extend_from_slice(&(-1i32).to_le_bytes());
        bon.extend_from_slice(&(-1i32).to_le_bytes());

        // `.smd`: MOXB + DMSA version 8, naming the two files above.
        let mut smd = Vec::new();
        smd.extend_from_slice(b"MOXB");
        smd.extend_from_slice(b"DMSA");
        smd.extend_from_slice(&8u32.to_le_bytes());
        smd.extend_from_slice(&1u32.to_le_bytes()); // one skin
        smd.extend_from_slice(&0u32.to_le_bytes()); // no actions
        smd.extend_from_slice(&0u32.to_le_bytes()); // no hangers
        smd.extend_from_slice(&0u32.to_le_bytes()); // no props
        smd.extend_from_slice(&[0u8; 60]);
        // Every name is a `u32` byte length followed by GBK bytes.
        let mut name = |text: &str, out: &mut Vec<u8>| {
            out.extend_from_slice(&(text.len() as u32).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        };
        name("bone.bon", &mut smd);
        name("tri.ski", &mut smd);
        name("", &mut smd); // no physique
        name("", &mut smd); // no track directory
        vec![
            ("gfx/models/测试/三角形.smd".into(), smd),
            ("gfx/models/测试/bone.bon".into(), bon),
            ("gfx/models/测试/tri.ski".into(), ski),
        ]
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
            spin: Quat::IDENTITY,
            keypoint_direction: Quat::IDENTITY,
            view,
            common,
            version: 37,
            color: u32::MAX,
            span: [Vec3::ZERO, Vec3::ZERO],
            color_scale: 1.,
            scale: 1.,
            emission_scale: 1.,
            rotation: 0.,
            clock: 0.,
        }
    }

    /// A 3D particle's travel direction is element-local, so the basis built
    /// from it has to be brought into the world frame the view axes live in.
    /// With the element spun 90 degrees about Z, a particle travelling along its
    /// own +X is travelling along world +Y, and world +Y is the axis its quad
    /// has to span; the local +X the unfixed mix used is a different direction
    /// in every placement that is not axis aligned.
    #[test]
    fn an_oriented_particle_takes_its_direction_into_the_world_frame() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.placement = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let (right, up) = oriented_basis(&context, Vec3::X);
        assert!((up - Vec3::Y).length() < 1e-5, "up was {up:?}");
        assert!(
            (up - Vec3::X).length() > 0.5,
            "up stayed in the local frame"
        );
        assert!(((right.length() - 1.).abs()) < 1e-4, "right was {right:?}");
        assert!(right.dot(up).abs() < 1e-5, "basis is not orthogonal");
        // An identity placement has to be untouched by the change.
        context.placement = Mat4::IDENTITY;
        let (right, up) = oriented_basis(&context, Vec3::X);
        assert!((up - Vec3::X).length() < 1e-5);
        assert!((right - Vec3::Y).length() < 1e-5 || (right + Vec3::Y).length() < 1e-5);
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

    /// Id 120's block spends one draw, not the two the shared direction slot's
    /// cone spends: its polar angle is `Angle` itself, not a sample.
    #[test]
    fn surface_cone_spends_one_draw() {
        let mut rng = Rng::new(71);
        let mut reference = Rng::new(71);
        let _ = surface_cone(&mut rng, 0.6);
        reference.next_u32();
        assert_eq!(rng.next_u32(), reference.next_u32());
    }

    /// The sample sits at polar angle `Angle` for every azimuth, one unit from
    /// the emitter, and `AreaSize` has no say in it — the client's block never
    /// reads emitter+0xb4. The direction and the position are the same vector,
    /// because node+0x44 and node+0x10 are built from it.
    #[test]
    fn id_120_surface_emission_is_a_cone_one_unit_out() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 120;
        let mut particle = Particle::default();
        particle.is_surface = true;
        particle.area_size = Some(Vec3::new(4., 3., 2.));
        particle.angle = 0.7;
        let mut rng = Rng::new(83);
        for _ in 0..400 {
            let state = spawn(&particle, None, &mut rng, &context);
            assert!(
                (state.position.z - 0.7f32.cos()).abs() < 1e-5,
                "polar angle drifted: {}",
                state.position
            );
            assert!(
                (state.position.length() - 1.).abs() < 1e-5,
                "not one unit out: {}",
                state.position
            );
            assert_eq!(state.direction, state.position);
        }
    }

    /// Ids 122 and 125 never read `IsSurface` — `0x7288f0` and `0x726570` do not
    /// test emitter+0x84 at all — so an element of either class still fills the
    /// volume even with the flag set.
    #[test]
    fn other_classes_keep_the_volume_path() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        context.id = 122;
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
            "id 122 should still fill the volume; its closest sample was {closest}"
        );
    }

    /// Every id 121 sample sits on a face: one component at the full half-extent,
    /// the other two inside it.
    #[test]
    fn id_121_surface_emission_lands_on_a_box_face() {
        let half = Vec3::new(1., 0.5, 0.25);
        let mut rng = Rng::new(97);
        for _ in 0..2_000 {
            let p = box_surface_sample(&mut rng, half);
            assert!(
                p.x.abs() <= half.x + 1e-6
                    && p.y.abs() <= half.y + 1e-6
                    && p.z.abs() <= half.z + 1e-6,
                "{p} is outside the box {half}"
            );
            let on = [
                (p.x.abs() - half.x).abs() < 1e-6,
                (p.y.abs() - half.y).abs() < 1e-6,
                (p.z.abs() - half.z).abs() < 1e-6,
            ];
            assert!(on.iter().any(|v| *v), "{p} is on no face of {half}");
        }
    }

    /// The face choice is not the area split, and one pair can be unreachable.
    /// `0x7278d6` compares the draw against `yz/sum` on its own rather than the
    /// cumulative `(xy+yz)/sum`, so the X faces need a draw that is both at least
    /// `xy/sum` and below `yz/sum` — possible only when `z > x`. With half-extents
    /// (1, 0.5, 0.25) the areas would give Z 4/7, X 1/7, Y 2/7, and the client
    /// gives Z 4/7, X none, Y 3/7: the X faces' share falls to the Y faces. A cube
    /// is the extreme case, needing a draw both `>= 1/3` and `< 1/3`. Flipping the
    /// extents so `z > x` opens all three again.
    #[test]
    fn id_121_face_choice_carries_the_clients_skew() {
        let draws = 4_000;
        let mut count = |half: Vec3| {
            let mut rng = Rng::new(97);
            let mut faces = [0usize; 3];
            for _ in 0..draws {
                let p = box_surface_sample(&mut rng, half);
                let on = [
                    (p.x.abs() - half.x).abs() < 1e-6,
                    (p.y.abs() - half.y).abs() < 1e-6,
                    (p.z.abs() - half.z).abs() < 1e-6,
                ];
                for (slot, hit) in faces.iter_mut().zip(on) {
                    if hit {
                        *slot += 1;
                    }
                }
            }
            faces
        };
        let flat = count(Vec3::new(1., 0.5, 0.25));
        assert_eq!(
            flat[0], 0,
            "the X faces are unreachable when z < x: {flat:?}"
        );
        let z_share = flat[2] as f32 / draws as f32;
        assert!(
            (z_share - 4. / 7.).abs() < 0.03,
            "the Z faces took {z_share} of the draws, not the 4/7 the client's              xy/sum test implies"
        );
        assert!(flat[1] > 0, "the Y faces were never chosen: {flat:?}");
        let tall = count(Vec3::new(0.25, 0.5, 1.));
        assert!(
            tall.iter().all(|f| *f > 0),
            "with z > x all three face pairs are reachable: {tall:?}"
        );
    }

    /// Four draws, in the client's order: the face, the sign, then the two
    /// in-face coordinates.
    #[test]
    fn box_surface_sample_spends_four_draws() {
        let mut rng = Rng::new(101);
        let mut reference = Rng::new(101);
        let _ = box_surface_sample(&mut rng, Vec3::new(1., 0.5, 0.25));
        for _ in 0..4 {
            reference.next_u32();
        }
        assert_eq!(rng.next_u32(), reference.next_u32());
    }

    /// 512 corpus elements collapse to a zero extent, where the client's ratios
    /// are all 0/0. Both fall out at the origin rather than as NaN.
    #[test]
    fn a_zero_extent_box_collapses_to_the_origin() {
        let mut rng = Rng::new(103);
        let p = box_surface_sample(&mut rng, Vec3::ZERO);
        assert!(p.is_finite(), "{p} is not finite");
        assert_eq!(p, Vec3::ZERO);
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
        if !root.is_dir() {
            // Say so, loudly and in the test output, rather than returning a
            // quiet `ok`. A corpus test that silently skips is indistinguishable
            // from one that passes, and in this session that is exactly how two
            // wrong fixes came to look verified: the checks that would have
            // contradicted them never ran.
            eprintln!(
                "SKIPPED: no `google_drive/x` corpus on this machine; this test \
                 proves nothing about shipped assets"
            );
            return None;
        }
        Some(root)
    }

    /// `GrndNormOnly` must put the effect on the slope's plane without moving it
    /// down to the surface.
    ///
    /// The three ground frames are distinct: `height_only` keeps the owner's own
    /// orientation at the raycast height, `aligned` takes both the orientation
    /// and the height from the surface, and the frame this flag asks for takes
    /// the orientation only. 456 corpus sprites set the flag, and they are
    /// almost all weapon trails, which is exactly the case where dropping to the
    /// surface is wrong: the trail belongs at the height it was struck.
    #[test]
    fn grnd_norm_only_keeps_the_element_at_its_own_height() {
        // The three ground frames differ only in what they take from the surface:
        // `height_only` takes the height, `aligned` takes the height and the
        // normal, and the frame `GrndNormOnly` asks for takes the normal alone.
        // Tested at the frame level, because the element's own keypoint offset is
        // applied on top of whichever frame is chosen and would mask the choice.
        let place = Mat4::from_translation(Vec3::Y * 2.);
        let base = Mat4::from_scale(Vec3::splat(1.));
        let hit = GroundHit {
            height: -40.,
            normal: tilted_normal(2.),
        };
        // A minimal player, only for its ground helpers.
        let player = build(&(header37(1) + &SPRITE));
        let frames = player
            .ground_frames(hit, place, base)
            .expect("a finite hit must build the frames");
        let parsed = gfx_file::parse(&(header37(1) + &SPRITE)).unwrap();
        let mut element = parsed.elements[0].clone();
        let Spec::Sprite(ref mut sprite) = element.spec else {
            panic!("the fixture is a sprite")
        };
        sprite.ground_normal_only = true;
        let flagged = player.ground_parent(Some(&frames), &element, base);
        element.spec = match element.spec {
            Spec::Sprite(s) => Spec::Sprite(Sprite {
                ground_normal_only: false,
                ..s
            }),
            other => other,
        };
        let unflagged = player.ground_parent(Some(&frames), &element, base);
        // The owner is at y = 2 and the surface at y = -40. The flagged element
        // must stay at the owner; the unflagged one has nothing to ask for and
        // keeps the plain base.
        assert!(
            (flagged.w_axis.y - 2.).abs() < 0.01,
            "GrndNormOnly must stay at the owner's altitude, got {}",
            flagged.w_axis.y
        );
        assert!(
            (unflagged.w_axis.y - 0.).abs() < 0.01,
            "without the flag and without MatchGrnd/GroundHeight the base frame stands, got {}",
            unflagged.w_axis.y
        );
        // And the aligned frame really does reach the surface, which is what the
        // flag is choosing not to do.
        assert!(
            (frames.aligned.w_axis.y + 40.).abs() < 0.01,
            "the aligned frame must sit on the surface, got {}",
            frames.aligned.w_axis.y
        );
    }

    /// Element 190 curves must draw, not just be counted.
    ///
    /// The client's builder is `0x708460`: it takes the two root-level `Vec:`
    /// endpoints, cross-multiplies them for a side vector, normalises the chord,
    /// and derives the segment count from `1.0 / length`. Nothing in that
    /// routine needs more than the two vectors, which is why the curve is the one
    /// class that reads the root `Vec:` pair instead of the element position -
    /// it exists to span a distance rather than mark a point.
    ///
    /// Only one placed root uses it, the distant floating city, so this drives
    /// that file and requires its curve textures to reach the quad list.
    #[test]
    fn corpus_curve_elements_reach_the_quad_list() {
        let Some(root) = corpus_root() else {
            return;
        };
        let path = root.join("gfx/场景/新种族地图/0远完美浮空城.gfx");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let (source, _) = crate::gfx_file::decode(&bytes);
        let parsed = crate::gfx_file::parse(&source).unwrap();
        let curves: Vec<_> = parsed
            .elements
            .iter()
            .filter(|e| matches!(e.spec, Spec::Curve(_)))
            .map(|e| e.common.texture.clone())
            .collect();
        assert_eq!(curves.len(), 2, "the floating city authors two curves");
        let mut load = |child: &str| -> Result<Root, String> {
            let bytes = std::fs::read(root.join(child)).map_err(|e| e.to_string())?;
            let (source, _) = crate::gfx_file::decode(&bytes);
            crate::gfx_file::parse(&source)
        };
        let mut player = Player::new(parsed, 7, &mut load).unwrap();
        let view = view();
        for _ in 0..40 {
            let _ = player.advance_with_ground(33., &view, Mat4::IDENTITY, 1., 1., None);
        }
        for texture in curves {
            let wanted = texture_path(&texture).unwrap();
            assert!(
                player.quads().iter().any(|q| q.texture == wanted),
                "curve texture {wanted} never reached the quad list"
            );
        }
    }

    /// Every texture the shipped effect roots name must resolve to a decoded
    /// image.
    ///
    /// This is the regression for the white-square defect. `Dds::parse` used to
    /// refuse every surface without a FourCC tag, which is 1,232 of the 1,610
    /// distinct textures these roots name, and the draw answered each refusal
    /// with a flat white placeholder. On screen that is a hard white square
    /// animating where a soft smoke puff belongs, which is exactly how the fire
    /// in Etherblade was reported. Checking the paths alone is not enough: a
    /// texture that resolves but fails to decode is the same white square, so
    /// this decodes too.
    #[test]
    fn corpus_effect_textures_all_resolve_and_decode() {
        let Some(root) = corpus_root() else {
            return;
        };
        let textures = std::path::Path::new("google_drive/tex");
        if !textures.is_dir() {
            return;
        }
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let (mut roots, mut clean) = (0usize, 0usize);
        let (mut references, mut missing, mut undecodable) = (0usize, Vec::new(), Vec::new());
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed) = crate::gfx_file::parse(&source) else {
                continue;
            };
            roots += 1;
            let mut failed = false;
            for element in &parsed.elements {
                if element.common.texture.is_empty() {
                    continue;
                }
                references += 1;
                let Ok(path) = texture_path(&element.common.texture) else {
                    missing.push(element.common.texture.clone());
                    failed = true;
                    continue;
                };
                let relative = path.trim_start_matches("gfx/textures/");
                let Ok(image) = std::fs::read(textures.join(relative)) else {
                    missing.push(element.common.texture.clone());
                    failed = true;
                    continue;
                };
                // DDS is this reader's own format; everything else is handed to
                // the image crate by `decode_rgba`, which the harness cannot
                // link. Those are checked for presence only.
                if image.starts_with(b"DDS ")
                    && super::super::model_dds::Dds::parse(&image).is_err()
                {
                    undecodable.push(element.common.texture.clone());
                    failed = true;
                }
            }
            if !failed {
                clean += 1;
            }
        }
        println!("effect roots {roots}, fully decodable {clean}, references {references}");
        // Four distinct names in the whole corpus do not resolve, across 17
        // references: the vertical lightning strip `序列\\闪电竖.tga` (the archive
        // carries it as `.dds`, so the reference and the member disagree on the
        // extension), `图案\\155.dds`, `图案\\场景3紫.dds` and a second copy of the
        // lightning strip under `地面效果3/4.gfx`. The archive genuinely does not
        // carry the other three. Bound the count to what the corpus actually
        // holds, so a path regression cannot hide behind a loose ceiling.
        assert!(
            missing.len() <= 17,
            "{} texture names do not resolve: {:?}",
            missing.len(),
            &missing[..missing.len().min(6)]
        );
        assert!(
            undecodable.is_empty(),
            "{} effect textures do not decode: {:?}",
            undecodable.len(),
            &undecodable[..undecodable.len().min(6)]
        );
        assert!(
            roots > 5000 && clean > roots - 10,
            "roots {roots} clean {clean}"
        );
    }

    /// The effect camera has to come back out of the frame matrix exactly, or
    /// every billboard in the world faces the wrong way.
    ///
    /// This is the regression for the second half of the defect that froze the
    /// world's effects: the eye was read with `transform_point3`, which drops the
    /// perspective divide and so returned the near plane rather than the camera
    /// (3,911 units off for a camera looking across the world), and the axes were
    /// read off the inverse's columns, which carry the clip-space mirror the
    /// viewer applies. Both are checked here on the viewer's own projection
    /// (mirrored scale, 70-degree perspective) and on a plain one, so the method
    /// has to work with and without the handedness flip.
    #[test]
    fn effect_camera_comes_back_out_of_the_frame_matrix() {
        for (name, position, direction, aspect, mirrored) in [
            (
                "across the world",
                Vec3::new(2291., 250., -3160.),
                Vec3::new(0.3, -0.2, 0.9).normalize(),
                16. / 9.,
                true,
            ),
            (
                "at the far edge",
                Vec3::new(20000., 900., -18000.),
                Vec3::new(-0.5, 0.1, 0.8).normalize(),
                1.,
                true,
            ),
            (
                "looking down",
                Vec3::new(10., 500., 10.),
                Vec3::new(0.01, -1.0, 0.02).normalize(),
                1.6,
                true,
            ),
            (
                "plain projection",
                Vec3::new(1., 2., 3.),
                Vec3::new(0., 0., -1.),
                1.,
                false,
            ),
        ] {
            let projection = Mat4::perspective_rh(70f32.to_radians(), aspect, 16000., 1.);
            let view = Mat4::look_to_rh(position, direction, Vec3::Y);
            let matrix = if mirrored {
                Mat4::from_scale(Vec3::new(-1., 1., 1.)) * projection * view
            } else {
                projection * view
            };
            let recovered = View::from_inverse_view_projection(matrix.inverse());
            assert!(
                (recovered.position - position).length() < 0.01,
                "{name}: eye {:?} is not {position:?}",
                recovered.position
            );
            assert!(
                (recovered.forward - direction).length() < 0.001,
                "{name}: forward {:?} is not {direction:?}",
                recovered.forward
            );
            for axis in [recovered.right, recovered.up] {
                assert!((axis.length() - 1.).abs() < 0.01, "{name}: unit axis");
                assert!(
                    axis.dot(recovered.forward).abs() < 0.01,
                    "{name}: screen axis must be perpendicular to the view direction"
                );
            }
            assert!(
                recovered.right.dot(recovered.up).abs() < 0.01,
                "{name}: right and up must be perpendicular"
            );
        }
        // A matrix with no inverse must fall back rather than produce a NaN
        // camera that would collapse every quad.
        let degenerate = View::from_inverse_view_projection(Mat4::ZERO);
        assert!(degenerate.position.is_finite());
        assert!(degenerate.forward.is_finite());
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
            let direction = cone_direction(axis, Vec3::X, std::f32::consts::FRAC_PI_2, &mut rng);
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
            let direction = cone_direction(axis, Vec3::X, std::f32::consts::FRAC_PI_6, &mut rng);
            assert!(
                direction.dot(axis) > rim,
                "{direction} escaped a 30 degree cone"
            );
        }

        let mut behind = 0usize;
        for _ in 0..1_000 {
            let direction = cone_direction(axis, Vec3::X, std::f32::consts::TAU, &mut rng);
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
            let direction = cone_direction(axis, Vec3::X, -std::f32::consts::FRAC_PI_2, &mut rng);
            forward += (direction.dot(axis) > 0.) as usize;
            spread = spread.max(1. - direction.dot(axis));
        }
        assert_eq!(forward, 1_000, "a mirrored cone stays in front of its axis");
        assert!(spread > 0.9, "and is not a single ray: spread {spread}");
    }

    /// The direction slot `0x723440` hands a bound emitter the global Z axis
    /// `0x9c0338` verbatim (`0x723451`) and an unbound one `rotate(Z,
    /// quat(emitter+0x74))` (`0x72345b`), where +0x74 is `Q * keypoint
    /// Direction`. The player's local frame already carries that keypoint
    /// direction through `placement`, so an unbound emitter's direction comes out
    /// as the element's own Z — the Y fallback this replaced aimed every cone,
    /// and every one of the corpus's 4,338 non-zero `Speed` records, at a
    /// different axis.
    #[test]
    fn the_emission_axis_is_the_element_forward_axis() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        let particle = Particle::default();
        let (axis, perp) = emission_axis(&particle, &context);
        assert_eq!(axis, Vec3::Z);
        assert_eq!(perp, Vec3::X);

        // Turn the keypoint: the local axis stays Z, and what `placement` makes
        // of it is the client's `rotate(Z, q)`.
        let turn = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        context.keypoint_direction = turn;
        context.spin = turn;
        context.placement = Mat4::from_quat(turn);
        let (axis, perp) = emission_axis(&particle, &context);
        assert!((axis - Vec3::Z).length() < 1e-6, "{axis}");
        assert!(
            (context.placement.transform_vector3(axis) - turn * Vec3::Z).length() < 1e-6,
            "the world direction has to be the keypoint direction turned Z"
        );
        assert!((perp - Vec3::X).length() < 1e-6, "{perp}");
    }

    /// A bound emitter's direction is bone local — `0x723440` copies the global
    /// Z axis without touching the orientation quaternion — while this player's
    /// local frame carries the keypoint direction, so that direction has to be
    /// undone rather than applied. 2,770 of the corpus's 9,142 bound particle
    /// keypoints author a non-identity one.
    #[test]
    fn bound_emission_unwinds_the_keypoint_direction() {
        let view = view();
        let common = Common::default();
        let mut context = spawn_context(&view, &common);
        let mut particle = Particle::default();
        particle.is_bind = true;
        let turn = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        context.keypoint_direction = turn;
        context.spin = turn;
        context.placement = Mat4::from_quat(turn);
        let (axis, perp) = emission_axis(&particle, &context);
        assert!(
            (context.placement.transform_vector3(axis) - Vec3::Z).length() < 1e-6,
            "in the bone frame the direction is the bone's Z, got {axis}"
        );
        assert!(
            (context.placement.transform_vector3(perp) - Vec3::X).length() < 1e-6,
            "and the cone's axis is the bone's X, got {perp}"
        );
    }

    /// `ParIniDir` is three Euler angles: the parser at `0x7242e2` turns them
    /// into a quaternion through `0x7b6f10`, composes it with emitter+0x74 into
    /// +0xa0 (`0x7243b8`), and the samplers copy those four floats into node+0x34
    /// (`0x7283f5`), the particle's initial *orientation*. The direction slot
    /// never reads +0xa0, so authoring one does not move the emission axis. The
    /// values below are the corpus's own — exact degree conversions, which no
    /// unit direction would be.
    #[test]
    fn par_initial_direction_is_not_an_emission_axis() {
        let view = view();
        let common = Common::default();
        let context = spawn_context(&view, &common);
        let mut particle = Particle::default();
        particle.initial_direction = Vec3::new(0.017453, 0., 1.570796);
        let (axis, perp) = emission_axis(&particle, &context);
        assert_eq!(axis, Vec3::Z);
        assert_eq!(perp, Vec3::X);
    }

    /// The cone spins its perpendicular axis about the emission direction by the
    /// azimuth (`0x72363b`, `0x723661`–`0x7236e9`) and then tilts the direction
    /// by the polar angle about that spun axis (`0x723718`, `0x723741`–`0x7237c1`).
    /// With the direction at Z and the perpendicular at X, a quarter turn with no
    /// azimuth lands on -Y, which is `rotate(Z, axis_angle(X, pi/2))` — the
    /// draw-for-draw consequence of X being the reference axis rather than an
    /// arbitrary perpendicular.
    #[test]
    fn the_cone_tilts_from_the_spun_perpendicular() {
        let d = cone_sample(Vec3::Z, Vec3::X, std::f32::consts::FRAC_PI_2, 0.);
        assert!((d + Vec3::Y).length() < 1e-6, "{d}");
        let d = cone_sample(
            Vec3::Z,
            Vec3::X,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        );
        assert!((d - Vec3::Y).length() < 1e-6, "{d}");
        let d = cone_sample(Vec3::Z, Vec3::X, 0., 1.234);
        assert!((d - Vec3::Z).length() < 1e-6, "{d}");
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
        assert!(
            player.warnings()[0].ends_with("the root plays without this child"),
            "the warning says what happens next: {}",
            player.warnings()[0]
        );
        assert_eq!(player.stats().children, 0);

        let mut player = player;
        step(&mut player, 16.);
        assert_eq!(player.quads().len(), 1, "the sprite next to it still draws");
    }

    /// Two roots playing side by side — a row of torches, two waterfalls from the
    /// same file — must not share one random stream, or every copy emits the same
    /// particles in lockstep, and no seed may be zero, which stalls the xorshift.
    #[test]
    fn path_seeds_are_odd_and_distinct_per_root() {
        let torch = path_seed("gfx/scene/torch.gfx");
        assert_eq!(torch, path_seed("gfx/scene/torch.gfx"), "stable per path");
        assert_ne!(torch, path_seed("gfx/scene/torch2.gfx"));
        assert_ne!(torch, path_seed("gfx/scene/Torch.gfx"));
        let empty = path_seed("");
        assert!(
            empty != 0 && empty & 1 == 1,
            "even the empty path is a usable seed: {empty}"
        );
        for path in ["gfx/a.gfx", "gfx/人物/技能/巫师/技能抑制_闪电.gfx", "x"] {
            assert_eq!(path_seed(path) & 1, 1, "{path}");
        }
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

    /// Element 190's segment count has to hold its own stated invariant: the
    /// stretch per segment is constant. The count was `(1 / length) * coeff`,
    /// which is inverted, so a beam longer than the coefficient collapsed to the
    /// one-segment floor and drew as a single billboard across the whole span.
    #[test]
    fn a_curve_keeps_a_constant_stretch_per_segment() {
        // Two 2-point spans of very different lengths, one element each.
        let curve = |a: &str, b: &str| {
            format!(
                "GFXELEMENTID: 190\nName: cu\nSrcBlend: 5\nDestBlend: 2\n\
                 Coeff: 1.000000\nheight: 1.0\n\
                 RepeatCount: 1\nRepeatDelay: 0\n\
                 StartTime: 0\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\nTimeSpan: -1\n\
                 Position: 0, 0, 0\nColor: -1\nScale: 1\nDirection: 0, 0, 0, 1\n\
                 Rad_2D: 0\nCtrlMethodCount: 0\n"
            ) + &format!("Vec: {a}\nVec: {b}\n")
        };
        let _ = curve;
        // The span comes from the root's two `Vec:` lines, so the length is set
        // on the root header. Compare a 10-unit chord with a 40-unit one: the
        // quad count must scale with the chord, not collapse to one.
        let root_span = |half: f32| {
            format!(
                "MOXTVersion: 79\r\nDedaultScale: 1\r\nPlaySpeed: 1\r\nDefaultAlpha: 1\r\n\
                 Raytrace: 0\r\nFadeByDist: 0\r\nFadeStart: 0\r\nFadeEnd: 0\r\n\
                 Vec: 0, 0, 0\r\nVec: {h}, 0, 0\r\nShakeCam: 0\r\nGFXELEMENTCOUNT: 1\r\n\
                 GFXELEMENTID: 190\r\nName: cu\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
                 TexFile: 轨迹\\beam.dds\r\nPriority: 2\r\nIsDummy: 0\r\n\
                 Coeff: 1.000000\r\nheight: 1.000000\r\n\
                 RepeatCount: 1\r\nRepeatDelay: 0\r\n\
                 StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
                 Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
                 Rad_2D: 0\r\nCtrlMethodCount: 0\r\n",
                h = half
            )
        };
        let count = |half: f32| {
            let mut player = build(&root_span(half));
            step(&mut player, 16.);
            player.quads().len()
        };
        let short = count(5.);
        let long = count(20.);
        assert!(short >= 2, "a 10-unit chord is {short} quads, not one slab");
        assert!(
            long > short,
            "a 40-unit chord ({long}) must carry more segments than a 10-unit one \
             ({short}); the count is inverted if it does not"
        );
        // The invariant itself: stretch per segment is the coefficient.
        for half in [5f32, 20.] {
            let length = half * 2.;
            let segments = count(half).max(1) as f32;
            let stretch = length / segments;
            assert!(
                (stretch - 1.).abs() < 1.05,
                "assumed span {length} -> {segments} quads, stretch {stretch}, \
                 which is not the ~1.0 the coefficient asks for"
            );
        }
    }

    /// A child root's clock must run at the product of the element's own
    /// `PlaySpeed` and the child root's `PlaySpeed`, exactly once each. The call
    /// site passes `dt_s * 1000 * element_play_speed` and `advance` then applies
    /// the child root's own rate, so an accidental second multiplication of the
    /// same factor would show up here as a squared rate.
    #[test]
    fn a_child_clock_runs_at_both_play_speeds_once_each() {
        // The child root's own PlaySpeed is the parameter; its single sprite is
        // a forever keypoint, so nothing but the clock moves.
        let child_root = |rate: f32| {
            format!(
                "MOXTVersion: 37\r\nDedaultScale: 1\r\nPlaySpeed: {rate}\r\nDefaultAlpha: 1\r\n\
                 Raytrace: 0\r\nFadeByDist: 0\r\nFadeStart: 0\r\nFadeEnd: 0\r\n\
                 Vec: 0, 0, 0\r\nVec: 1, 1, 1\r\nShakeCam: 0\r\nGFXELEMENTCOUNT: 1\r\n\
                 GFXELEMENTID: 100\r\nName: inner\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
                 RepeatCount: -1\r\nRepeatDelay: 0\r\n\
                 Width: 1\r\nHeight: 1\r\nRotFromView: 0\r\nGrndNormOnly: 0\r\n\
                 StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
                 Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
                 Rad_2D: 0\r\nCtrlMethodCount: 0\r\n"
            )
        };
        let parent = |rate: f32| {
            format!(
                "MOXTVersion: 37\r\nDedaultScale: 1\r\nPlaySpeed: 1\r\nDefaultAlpha: 1\r\n\
                 Raytrace: 0\r\nFadeByDist: 0\r\nFadeStart: 0\r\nFadeEnd: 0\r\n\
                 Vec: 0, 0, 0\r\nVec: 1, 1, 1\r\nShakeCam: 0\r\nGFXELEMENTCOUNT: 1\r\n\
                 GFXELEMENTID: 200\r\nName: ch\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
                 GfxPath: T\\c.gfx\r\nOutColor: 0\r\nLoopFlag: 1\r\nPlaySpeed: {rate}\r\n\
                 StartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\nTimeSpan: -1\r\n\
                 Position: 0, 0, 0\r\nColor: -1\r\nScale: 1\r\nDirection: 0, 0, 0, 1\r\n\
                 Rad_2D: 0\r\nCtrlMethodCount: 0\r\n"
            )
        };
        // One 16 ms parent frame. The parent root is at PlaySpeed 1, so the
        // child clock must read 16 * element_rate * root_rate.
        for (element_rate, root_rate) in [(1.0f32, 1.0f32), (1.0, 2.0), (2.0, 2.0), (0.5, 1.0)] {
            let child = child_root(root_rate);
            let source = parent(element_rate);
            let root = gfx_file::parse(&source).unwrap();
            let mut player = Player::new(root, 7, &mut |_| gfx_file::parse(&child)).unwrap();
            step(&mut player, 16.);
            let got = player.child_time_ms().expect("the child resolved");
            let want = 16.0 * element_rate * root_rate;
            assert!(
                (got - want).abs() < 0.01,
                "element PlaySpeed {element_rate} x root PlaySpeed {root_rate}: child clock \
                 read {got} ms, want {want} ms"
            );
        }
    }

    /// A flipbook with no authored `TexInterval` must hold its first cell, not
    /// flip a thousand times a second. The fallback used to substitute 1 ms for
    /// the absent interval, so a `TexRow`/`TexCol` sheet with the key omitted
    /// strobed at 1000 Hz — the purest form of "the effect plays too fast".
    #[test]
    fn a_flipbook_without_an_interval_holds_its_first_cell() {
        use crate::gfx_file::Common;
        let mut common = Common::default();
        common.tex_row = 2;
        common.tex_col = 2;
        common.tex_interval = 0;
        // Every sample over two seconds has to be the same cell: the top-left
        // quarter, [0,0]..[0.5,0.5].
        for ms in [0f32, 16., 500., 1000., 1999.] {
            let uv = flipbook_uv(&common, ms);
            assert_eq!(uv[0], [0., 0.], "at {ms} ms the top-left corner moved");
            assert_eq!(uv[3], [0.5, 0.5], "at {ms} ms the cell changed");
        }
        // An authored interval still animates: 100 ms per cell over four cells.
        common.tex_interval = 100;
        assert_eq!(flipbook_uv(&common, 0.)[0], [0., 0.]);
        assert_eq!(flipbook_uv(&common, 150.)[0], [0.5, 0.], "cell 2 at 150 ms");
        assert_eq!(
            flipbook_uv(&common, 350.)[0],
            [0.5, 0.5],
            "cell 4 at 350 ms"
        );
    }

    /// The version-76 boundary on the depth test. The rule came from the static
    /// decal path and `push_quad` applies it to every quad, but nothing tested
    /// the boundary itself: 334 of the corpus's 390 `ZEnable: 0` elements sit in
    /// roots older than 76, so this gate decides most of them.
    #[test]
    fn depth_test_is_forced_below_version_76() {
        let mut common = Common::default();
        common.depth_test = false;
        assert!(
            depth_test(&common, 24),
            "the oldest accepted root is forced"
        );
        assert!(depth_test(&common, 37));
        assert!(depth_test(&common, 75), "one below the boundary is forced");
        assert!(!depth_test(&common, 76), "at 76 the authored value decides");
        assert!(!depth_test(&common, 79));
        common.depth_test = true;
        assert!(depth_test(&common, 79));
        assert!(depth_test(&common, 37));
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

    /// A sprite whose `BindEle` names a sibling takes that sibling's world
    /// placement as its parent basis. That is `0x710121`: the element builds its
    /// own basis from +0x58 with `0x6fe060`, then, because the bind target at
    /// +0xa0 is non-null, multiplies it by the matrix `0x6fe120` copies out of
    /// the target. With `DedaultScale: 2` the target's authored (5, 0, 0) lands
    /// at (10, 0, 0), and the follower — which authors the origin — lands there
    /// too rather than at the root origin where it would sit unbound.
    #[test]
    fn a_bound_element_follows_its_target_placement() {
        let target = SPRITE
            .replace("Name: glow", "Name: target")
            .replace("Position: 1, 2, 3", "Position: 5, 0, 0");
        let follower = SPRITE
            .replace("Name: glow", "Name: follower")
            .replace("BindEle: \r\n", "BindEle: target\r\n")
            .replace("Position: 1, 2, 3", "Position: 0, 0, 0");
        let mut player = build(&(header37(2) + &target + &follower));
        assert_eq!(player.elements[1].bind_target, Some(0));
        step(&mut player, 16.);
        let quads = player.quads();
        assert_eq!(quads.len(), 2, "both sprites draw");
        let centre = quad_centre(&quads[1]);
        assert!(
            (centre - Vec3::new(10., 0., 0.)).length() < 1e-3,
            "the follower sits at its target's world position, got {centre}"
        );
    }

    /// A `BindEle` that names nothing leaves the slot null and raises no
    /// diagnostic, which is what the resolution pass at `0x6e4370` does when the
    /// container's name map has no entry: the element simply plays on its own
    /// basis. The corpus has exactly one such file, whose element 110 names an
    /// absent `Unnamed7`.
    #[test]
    fn a_bind_name_that_resolves_to_nothing_plays_unbound() {
        let target = SPRITE.replace("Name: glow", "Name: target");
        let follower = SPRITE
            .replace("Name: glow", "Name: follower")
            .replace("BindEle: \r\n", "BindEle: nosuch\r\n");
        let mut player = build(&(header37(2) + &target + &follower));
        assert_eq!(player.elements[1].bind_target, None);
        step(&mut player, 16.);
        let centre = quad_centre(&player.quads()[1]);
        assert!(
            (centre - Vec3::new(2., 4., 6.)).length() < 1e-3,
            "unbound, the follower keeps its own (1, 2, 3) at the root scale, got {centre}"
        );
        assert!(player.warnings().is_empty(), "{:?}", player.warnings());
    }

    /// `0x6fa69a` makes a bound element's tick return without updating once the
    /// target's handle at +0x50 has gone negative, so a dependent does not
    /// outlive the element it follows. The target here plays once for 100 ms;
    /// the follower loops forever but stops with it, and a restart re-arms the
    /// dependency instead of leaving it latched.
    #[test]
    fn a_bound_element_stops_when_its_target_ends() {
        let target = SPRITE
            .replace("Name: glow", "Name: target")
            .replace("TimeSpan: -1", "TimeSpan: 100");
        let follower = SPRITE
            .replace("Name: glow", "Name: follower")
            .replace("BindEle: \r\n", "BindEle: target\r\n");
        let mut player = build(&(header37(2) + &target + &follower));
        step(&mut player, 16.);
        assert_eq!(player.quads().len(), 2, "both draw while the target runs");
        step(&mut player, 200.);
        assert!(
            player.quads().is_empty(),
            "the follower stopped with its target, got {} quads",
            player.quads().len()
        );
        assert!(player.elements[1].ended, "the dependent is marked ended");
        player.reset();
        step(&mut player, 16.);
        assert_eq!(
            player.quads().len(),
            2,
            "a reset gives the dependent its clock back"
        );
    }

    /// The follow is one level deep because `0x6fe120` reads the target's stored
    /// matrix instead of recomputing it, so no bind graph can recurse. The
    /// corpus has no 2-cycles and its longest chain is two links (24 elements
    /// bind to something that itself binds), but a mutual bind is legal input
    /// and has to stay finite rather than hang or produce NaN.
    #[test]
    fn a_mutual_bind_resolves_without_recursing() {
        let first = SPRITE
            .replace("Name: glow", "Name: a")
            .replace("BindEle: \r\n", "BindEle: b\r\n");
        let second = SPRITE
            .replace("Name: glow", "Name: b")
            .replace("BindEle: \r\n", "BindEle: a\r\n");
        let mut player = build(&(header37(2) + &first + &second));
        assert_eq!(player.elements[0].bind_target, Some(1));
        assert_eq!(player.elements[1].bind_target, Some(0));
        for frame in 0..8 {
            step(&mut player, 16.);
            assert_eq!(player.quads().len(), 2, "frame {frame}");
            for quad in player.quads() {
                assert!(
                    quad.positions.iter().flatten().all(|c| c.is_finite()),
                    "frame {frame} produced non-finite geometry"
                );
            }
        }
    }

    /// Across the archive 968 elements author a `BindEle` and 967 of them name a
    /// sibling in the same root. Element names are unique within a root in every
    /// file, which is what makes the first-match scan equal to the client's
    /// bucket-chain lookup, and 950 of the 967 targets carry an editor-generated
    /// `Unnamed*` name — they are real siblings, not placeholders.
    #[test]
    fn corpus_bind_names_resolve() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let (mut authored, mut resolved, mut duplicate_names) = (0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let mut names = std::collections::BTreeSet::new();
            for element in &parsed.elements {
                if !names.insert(element.common.name.clone()) {
                    duplicate_names += 1;
                }
                if !element.common.bind_element.is_empty() {
                    authored += 1;
                }
            }
            let Ok(player) = Player::new(parsed, 7, &mut |_| Err("no children".into())) else {
                continue;
            };
            resolved += player
                .elements
                .iter()
                .filter(|element| element.bind_target.is_some())
                .count();
        }
        assert_eq!(duplicate_names, 0, "element names are unique within a root");
        assert_eq!(authored, 968, "the corpus authors 968 BindEle names");
        assert_eq!(resolved, 967, "967 of them resolve to a sibling");
    }

    /// `SPRITE` with the two ground bytes authored as asked and the element at
    /// the origin, so a quad centre is exactly the frame translation the client
    /// builds. `GroundHeight` is only read from version 57 on, so these belong
    /// with `header79`.
    fn ground_sprite(match_ground: bool, ground_height: bool) -> String {
        SPRITE
            .replace(
                "MatchGrnd: 0",
                &format!(
                    "MatchGrnd: {}\r\nGroundHeight: {}",
                    match_ground as u8, ground_height as u8
                ),
            )
            .replace("Position: 1, 2, 3", "Position: 0, 0, 0")
    }

    /// Version 79, the newest in the corpus and one of the twelve that reads
    /// `GroundHeight`.
    fn header79(elements: u32) -> String {
        header37(elements).replace("MOXTVersion: 37", "MOXTVersion: 79")
    }

    /// A trail (element 152) at version 79, where all five trail keys are read.
    /// `RepeatCount: -1` keeps the element alive past its own clock, so these
    /// tests can watch the ring after the whole strip has been laid down, and
    /// `StartPos`/`EndPos` differ because a zero-length strip draws nothing at
    /// all. The element clock lasts `duration_ms`, which is what `update_trail`
    /// divides by `Segs` to get one append step; `Interval` is authored far out
    /// so the noise block does not regenerate inside a window under test.
    #[allow(clippy::fn_params_excessive_bools)]
    fn trail(
        segments: u32,
        is_append: bool,
        is_tail_disappear: bool,
        vertices_life: f32,
        duration_ms: f32,
    ) -> String {
        format!(
            "{header}GFXELEMENTID: 152\r\nName: t\r\nSrcBlend: 5\r\nDestBlend: 2\r\n\
             RepeatCount: -1\r\nTexFile: 轨迹\\trail003.dds\r\nBufLen: 256\r\n\
             Amplitude: 1.000000\r\nWaveLen: 1\r\nPersistence: 0.250000\r\nOctaveNum: 5\r\n\
             StartPos: 0, 0, 0\r\nEndPos: 1, 0, 0\r\nSegs: {segments}\r\nLightNum: 1\r\n\
             WaveLen: 1.000000\r\nInterval: 100000\r\nWidth: 0.050000\r\nWidth: 0.050000\r\n\
             Alpha: 1.000000\r\nAlpha: 1.000000\r\nWidth: 0.050000\r\nAlpha: 1.000000\r\n\
             Amplitude: 0.000000\r\nPos1Enable: 0\r\nPos2Enable: 0\r\nUseNormal: 0\r\n\
             Normal: 0, 0, 0\r\nFilter: 1\r\nResetLoopEnd: 0\r\nisappend: {is_append}\r\n\
             renderside: 0\r\n\
             istaildisappear: {is_tail_disappear}\r\nvertslife: {vertices_life}\r\n\
             tailfadeout: 0\r\nStartTime: 0\r\nKEYPOINTCOUNT: 1\r\nInterpolateMode: 0\r\n\
             TimeSpan: {duration_ms}\r\nPosition: 0, 0, 0\r\nColor: -1\r\nScale: 1.000000\r\n\
             Direction: 0, 0, 0, 1\r\nRad_2D: 0\r\nCtrlMethodCount: 0\r\n",
            header = header79(1),
            is_append = is_append as u8,
            is_tail_disappear = is_tail_disappear as u8,
        )
    }

    /// Advance by `dt_ms` and report how many quads the trail drew. One quad is
    /// one live ring record, so this reads the ring from the outside.
    fn trail_quads(player: &mut Player, dt_ms: f32) -> usize {
        player
            .advance(dt_ms, &view(), Mat4::IDENTITY, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
        player.quads().len()
    }

    /// `0x713ed4`: a trail that does not append is rebuilt whole, and the rebuild
    /// path never ages its records, so the strip is complete on frame one and
    /// stays complete for as long as the element runs — `vertslife` and
    /// `istaildisappear` have nothing to act on. 18 of the 156 corpus trails
    /// author `isappend: 0`, and this is the strip they drew before the ring
    /// existed.
    #[test]
    fn a_trail_that_does_not_append_draws_its_whole_strip_forever() {
        let mut player = build(&trail(4, false, true, 100., 100.));
        // `Segs: 4` is four cross-sections, so the strip is three quads.
        assert_eq!(
            trail_quads(&mut player, 16.),
            3,
            "the whole strip exists on frame one"
        );
        for frame in 1..20 {
            assert_eq!(
                trail_quads(&mut player, 16.),
                3,
                "frame {frame}: a rebuilt strip is never aged, so nothing expires"
            );
        }
    }

    /// `0x714030` lays one vertex down per `interval / Segs` milliseconds, and
    /// `0x714067` needs the accumulator strictly past a whole step first. With a
    /// 100 ms element clock and four segments that is 25 ms a vertex, so 20 ms
    /// frames build the strip one segment at a time and the fifth frame is the
    /// one that does not quite reach the next step.
    #[test]
    fn an_appending_trail_lays_one_vertex_per_step_of_its_duration() {
        let mut player = build(&trail(4, true, false, 100_000., 100.));
        let drawn: Vec<usize> = (0..6).map(|_| trail_quads(&mut player, 20.)).collect();
        assert_eq!(
            drawn,
            vec![0, 1, 2, 3, 3, 3],
            "one vertex per 25 ms, and the frame that only reaches 25 ms lays none"
        );
    }

    /// `0x714072` divides the accumulated time by the step rather than laying a
    /// single vertex, so a frame longer than one step catches the strip up
    /// instead of falling permanently behind.
    #[test]
    fn a_long_frame_lays_several_trail_vertices_at_once() {
        let mut player = build(&trail(4, true, false, 100_000., 100.));
        assert_eq!(
            trail_quads(&mut player, 60.),
            2,
            "60 ms over a 25 ms step lays two vertices in one frame"
        );
    }

    /// `0x713f60` ages every record by the frame delta and `0x713fbe` drops the
    /// first record that has reached its life together with every older one. The
    /// strip therefore climbs to its full length, holds while vertices are laid
    /// as fast as they expire, and then shortens from the old end until nothing
    /// is left — the tail disappearing that `istaildisappear` authors.
    #[test]
    fn a_trail_vertex_disappears_once_it_has_lived_its_life() {
        let mut player = build(&trail(4, true, true, 100., 100.));
        let drawn: Vec<usize> = (0..7).map(|_| trail_quads(&mut player, 30.)).collect();
        assert_eq!(
            drawn,
            vec![1, 2, 3, 2, 1, 0, 0],
            "the strip grows, holds at three, then loses its oldest vertex a frame at a time"
        );
    }

    /// The expiry pass at `0x713fbe` is guarded by `istaildisappear`, so with it
    /// clear the records still age and still clamp at their life, but nothing is
    /// ever dropped and the laid strip stays on screen. 20 of the 156 corpus
    /// trails author `istaildisappear: 0`.
    #[test]
    fn istaildisappear_clear_keeps_the_expired_vertices() {
        let mut player = build(&trail(4, true, false, 100., 100.));
        let drawn: Vec<usize> = (0..7).map(|_| trail_quads(&mut player, 30.)).collect();
        assert_eq!(
            drawn,
            vec![1, 2, 3, 3, 3, 3, 3],
            "the strip reaches its full length and stays there"
        );
    }

    /// The ring is 256 records: `push $0xc00` at `0x713820` is 256 times twelve
    /// bytes, `movl $0x100,0x31c(%esi)` stores that as the capacity and `0x713fd1`
    /// stops the count growing past it while the head keeps advancing. The parser
    /// accepts `Segs` up to 4096, so a strip longer than the ring loses its
    /// oldest vertices rather than overflowing it. No corpus trail goes past 100.
    #[test]
    fn the_trail_ring_holds_at_most_256_vertices() {
        let mut player = build(&trail(300, false, true, 100., 100.));
        assert_eq!(
            trail_quads(&mut player, 16.),
            TRAIL_RING_CAPACITY - 1,
            "a 300-section strip draws the newest 255 quads of its vertices"
        );
    }

    /// The two ring passes the append path calls, on the ring itself: aging
    /// clamps at the authored life instead of running past it, a record already
    /// at its life is left alone, and the drop keeps the newer records because
    /// the count shrinks from the old end (`count += -1 - position`, `0x71400f`).
    #[test]
    fn the_ring_ages_to_its_life_and_drops_from_the_old_end() {
        let mut ring = TrailRing::default();
        for index in 0..3 {
            ring.push(TrailVertex {
                index,
                life: 100.,
                age: 0.,
            });
        }
        ring.age(60.);
        ring.drop_expired();
        assert_eq!(ring.records.len(), 3, "nothing has reached its life yet");
        ring.records[0].age = 100.;
        ring.drop_expired();
        let kept: Vec<u32> = ring.records.iter().map(|record| record.index).collect();
        assert_eq!(
            kept,
            vec![1, 2],
            "the expired record and nothing newer goes"
        );
        ring.age(1000.);
        assert!(
            ring.records.iter().all(|record| record.age == record.life),
            "ages clamp at the life, they do not keep growing"
        );
        ring.drop_expired();
        assert!(
            ring.records.is_empty(),
            "and once every record is at its life the whole ring goes"
        );
    }

    /// `0x714620`: when the root's loop restarts, a trail that authors
    /// `ResetLoopEnd` zeroes its accumulator (+0x314), its laid count (+0x318)
    /// and its live count (+0x328), so the next loop lays the strip down again
    /// from its first vertex instead of carrying the previous loop's vertices
    /// with it.
    #[test]
    fn reset_loop_end_empties_the_trail_ring_on_a_restart() {
        let source =
            trail(4, true, false, 100_000., 100.).replace("ResetLoopEnd: 0", "ResetLoopEnd: 1");
        let mut player = build(&source);
        for _ in 0..4 {
            trail_quads(&mut player, 30.);
        }
        assert_eq!(player.quads().len(), 3, "the whole strip is down");
        player.reset();
        assert_eq!(
            trail_quads(&mut player, 16.),
            0,
            "a restart leaves the ring empty, so there is nothing to draw yet"
        );
        assert_eq!(
            trail_quads(&mut player, 30.),
            1,
            "and the strip is laid down again from its first vertex"
        );
    }

    /// The same restart without the flag: `0x714620` tests `ResetLoopEnd` at
    /// +0x13d before it zeroes anything, so a trail that does not author it
    /// keeps the vertices it has already laid and simply goes on aging them.
    #[test]
    fn a_trail_without_reset_loop_end_keeps_its_ring_across_a_restart() {
        let mut player = build(&trail(4, true, false, 100_000., 100.));
        for _ in 0..4 {
            trail_quads(&mut player, 30.);
        }
        assert_eq!(player.quads().len(), 3, "the whole strip is down");
        player.reset();
        assert_eq!(
            trail_quads(&mut player, 16.),
            3,
            "the ring survives the restart and the strip is still whole"
        );
    }

    /// A ground normal tilted `degrees` away from world up, in the XY plane.
    fn tilted_normal(degrees: f32) -> Vec3 {
        let radians = degrees.to_radians();
        Vec3::new(radians.sin(), radians.cos(), 0.).normalize()
    }

    fn step_grounded(player: &mut Player, transform: Mat4, ground: Option<GroundHit>) {
        player
            .advance_with_ground(16., &view(), transform, 1., 1., ground)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// The same frame with terrain but no `GroundHit`, which is what the world's
    /// effects get: the drape answers to the query alone.
    fn step_draped(player: &mut Player, transform: Mat4, terrain: Option<GroundHeightQuery<'_>>) {
        player
            .advance_with_terrain(16., &view(), transform, 1., 1., None, terrain)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// A `MatchGrnd` 3D decal whose parent-scaled extents are four units each
    /// way: two cells an axis at `_grnd_grid = 2`, and the keypoint scale is
    /// pinned to one so the arithmetic in the assertions is the client's own.
    fn draped_sheet(match_ground: bool, ground_normal_only: bool) -> String {
        header79(1)
            + &ground_sprite(match_ground, false)
                .replace("Width: 0.25", "Width: 1")
                .replace("Height: 0.5", "Height: 1")
                .replace("Scale: 2", "Scale: 1")
                .replace(
                    "GrndNormOnly: 0",
                    &format!("GrndNormOnly: {}", ground_normal_only as u8),
                )
    }

    /// `0x6e5b90` builds the rotation that takes world up onto the ground
    /// normal: the axis is `cross(up, normal)` divided by its own length
    /// (`fdivrs` against the 1.0 at `0x8d5c28`, `0x6e5c5e`), the angle is
    /// `acos(up·normal)` through `_CIacos` at `0x892f06`, and the up vector is
    /// the global `0x6ea170` initialises to (0.0, 1.0, 0.0). Applying the result
    /// to +Y must therefore land on the normal itself.
    #[test]
    fn ground_alignment_takes_world_up_onto_the_normal() {
        for degrees in [0f32, 0.5, 1., 2., 3., 4.9] {
            let normal = tilted_normal(degrees);
            let alignment = ground_alignment(normal)
                .unwrap_or_else(|| panic!("{degrees}° is inside the five-degree limit"));
            let up = alignment * Vec3::Y;
            assert!(
                (up - normal).length() < 1e-4,
                "{degrees}°: up landed on {up}, expected {normal}"
            );
        }
    }

    /// The rejection threshold is cos(5°), which `0x6e5b80` computes once at
    /// startup as `fcos` of the double at `0x8d5990`; `0x6e5bc1` fails the query
    /// below it, so a steep slope or a wall is not ground and the effect keeps
    /// the orientation it was authored with.
    #[test]
    fn a_surface_tilted_past_five_degrees_is_not_ground() {
        assert!(ground_alignment(tilted_normal(4.)).is_some());
        assert!(ground_alignment(tilted_normal(6.)).is_none());
        assert!(
            ground_alignment(tilted_normal(90.)).is_none(),
            "a wall is not ground"
        );
        assert_eq!(ground_alignment(Vec3::Y), Some(Quat::IDENTITY));
        // Straight down passes the five-degree test but the cross product
        // vanishes, and `0x6e5c5e` divides by its length without a guard, so
        // there is no native result to reproduce.
        assert_eq!(ground_alignment(Vec3::NEG_Y), None);
    }

    /// `MatchGrnd` with the root standing on the ground tilts the element onto
    /// the surface: `0x6fa7fd` returns owner+0xfc, which `0x6e5539` builds from
    /// the quaternion `0x6e57e0` composes out of the owner's own orientation and
    /// the ground alignment. The root is at the ground height here, so the byte
    /// at owner+0x284 is set and `MatchGrnd` on its own is enough.
    #[test]
    fn match_ground_tilts_the_element_onto_the_surface() {
        let normal = tilted_normal(3.);
        let mut player = build(&(header79(1) + &ground_sprite(true, false)));
        let transform = Mat4::from_translation(Vec3::new(3., 7., -2.));
        step_grounded(
            &mut player,
            transform,
            Some(GroundHit { height: 7., normal }),
        );
        let placement = player.elements[0]
            .placement
            .expect("the element was placed");
        let up = placement.transform_vector3(Vec3::Y).normalize();
        assert!(
            (up - normal).length() < 1e-3,
            "the element's up axis lies on the ground normal, got {up} for {normal}"
        );
        let centre = quad_centre(&player.quads()[0]);
        assert!(
            (centre - Vec3::new(3., 7., -2.)).length() < 1e-3,
            "and it stays over the point it was authored above, got {centre}"
        );
    }

    /// `GroundHeight` without `MatchGrnd` takes the height and leaves the
    /// orientation alone: `0x6fa7e3` returns owner+0x13c, the copy of the base
    /// matrix `0x6e5592` re-transforms with the raycast height, and the
    /// orientation selector `0x6fa810` returns the un-aligned owner+0x1a4
    /// because `MatchGrnd` is clear. Every corner is therefore the corner the
    /// element would have had with no ground at all, moved down by the
    /// difference in height.
    #[test]
    fn ground_height_alone_moves_the_element_without_tilting_it() {
        let source = header79(1) + &ground_sprite(false, true);
        let transform = Mat4::from_translation(Vec3::new(3., 7., -2.));
        let mut without = build(&source);
        step_grounded(&mut without, transform, None);
        let mut with = build(&source);
        step_grounded(
            &mut with,
            transform,
            Some(GroundHit {
                height: 2.,
                normal: tilted_normal(3.),
            }),
        );
        let expected = without.quads()[0].positions;
        let got = with.quads()[0].positions;
        for index in 0..4 {
            let want = Vec3::from_array(expected[index]) + Vec3::new(0., 2. - 7., 0.);
            assert!(
                (Vec3::from_array(got[index]) - want).length() < 1e-4,
                "corner {index} is a pure translation of the un-snapped one: {got:?} against {want}"
            );
        }
        let centre = quad_centre(&with.quads()[0]);
        assert!(
            (centre.y - 2.).abs() < 1e-4,
            "the element sits at the ground height, got {centre}"
        );
    }

    /// `MatchGrnd` on its own only applies within 0.2 units of the ground:
    /// `0x6fa7ee` reads owner+0x284, which `0x6e54ac` sets from `pos.y - height`
    /// against the float at `0x8eb828`. A root floating above the surface keeps
    /// the frame it was authored with, which is how the corpus can put
    /// `MatchGrnd` on an effect that is meant to hang in the air and only tilt
    /// when it actually reaches the ground.
    #[test]
    fn match_ground_alone_needs_the_root_on_the_ground() {
        let mut player = build(&(header79(1) + &ground_sprite(true, false)));
        let transform = Mat4::from_translation(Vec3::new(0., 9., 0.));
        step_grounded(
            &mut player,
            transform,
            Some(GroundHit {
                height: 0.,
                normal: tilted_normal(3.),
            }),
        );
        let centre = quad_centre(&player.quads()[0]);
        assert!(
            (centre.y - 9.).abs() < 1e-4,
            "nine units up the element stays where it was authored, got {centre}"
        );
    }

    /// `GroundHeight` has no such limit — `0x6fa7e3` returns the height frame
    /// without consulting owner+0x284 — and with `MatchGrnd` set the same root
    /// takes the aligned frame from any distance, which is the
    /// `GroundHeight && MatchGrnd` row of `0x6fa7c0`.
    #[test]
    fn ground_height_snaps_from_any_distance() {
        let normal = tilted_normal(3.);
        let mut player = build(&(header79(1) + &ground_sprite(true, true)));
        let transform = Mat4::from_translation(Vec3::new(0., 9., 0.));
        step_grounded(
            &mut player,
            transform,
            Some(GroundHit { height: 0., normal }),
        );
        let centre = quad_centre(&player.quads()[0]);
        assert!(
            centre.y.abs() < 1e-4,
            "snapped down to the ground, got {centre}"
        );
        let up = player.elements[0]
            .placement
            .expect("placed")
            .transform_vector3(Vec3::Y)
            .normalize();
        assert!(
            (up - normal).length() < 1e-3,
            "and tilted onto the surface, got {up}"
        );
    }

    /// Without a ground query nothing moves. The byte at owner+0x1f4 gates the
    /// whole owner pass at `0x6e540f` and both selectors at `0x6fa7c5` and
    /// `0x6fa813`, and no key in a `.gfx` file sets it, so a host that cannot
    /// raycast leaves `MatchGrnd` and `GroundHeight` inert. That is the state
    /// every effect in the viewer was in before this, and it is why 1,781 corpus
    /// elements authoring a non-zero `MatchGrnd` needed an owner rather than a
    /// parser change.
    #[test]
    fn ground_keys_are_inert_without_a_query() {
        let source = header79(1) + &ground_sprite(true, true);
        let transform = Mat4::from_translation(Vec3::new(0., 9., 0.));
        let mut plain = build(&source);
        plain
            .advance(16., &view(), transform, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut grounded = build(&source);
        step_grounded(&mut grounded, transform, None);
        assert_eq!(plain.quads()[0].positions, grounded.quads()[0].positions);
        let centre = quad_centre(&grounded.quads()[0]);
        assert!(
            (centre.y - 9.).abs() < 1e-4,
            "an owner that cannot raycast leaves the element alone, got {centre}"
        );
        assert!(grounded.warnings().is_empty(), "{:?}", grounded.warnings());
    }

    /// `A3DDecalEx::Fill_Verts_3D`'s ground branch: a `MatchGrnd` sheet is cut
    /// into two-unit cells and every vertex takes its own height from the
    /// terrain. Four units each way is two cells an axis, so the flat quad
    /// becomes four, the sheet lies in the world XZ plane, and each corner sits
    /// 0.2 above the slope underneath it — the number at `0x8eb828` that both
    /// the lift and the near-ground test read.
    #[test]
    fn a_ground_matched_sprite_is_draped_over_the_terrain() {
        let source = draped_sheet(true, false);
        let mut player = build(&source);
        let slope = |x: f32, _z: f32| Some(x);
        let terrain: GroundHeightQuery<'_> = &slope;
        step_draped(&mut player, Mat4::IDENTITY, Some(terrain));
        let quads = player.quads();
        assert_eq!(
            quads.len(),
            4,
            "two cells an axis should be four quads, got {}",
            quads.len()
        );
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
        for quad in quads {
            for corner in quad.positions {
                let [x, y, z] = corner;
                assert!(
                    (y - (x + 0.2)).abs() < 1e-4,
                    "corner ({x}, {y}, {z}) should sit 0.2 over the slope, not {y}"
                );
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_z = min_z.min(z);
                max_z = max_z.max(z);
            }
        }
        assert!(
            (min_x + 2.).abs() < 1e-4
                && (max_x - 2.).abs() < 1e-4
                && (min_z + 2.).abs() < 1e-4
                && (max_z - 2.).abs() < 1e-4,
            "the sheet should span four units on the ground, got \
             {min_x}..{max_x} by {min_z}..{max_z}"
        );
        // The flipbook rect is spread over the lattice, not repeated in every
        // cell: a 2x2 sheet with a one-cell texture covers three distinct
        // coordinates an axis and never more.
        let mut us: Vec<f32> = quads
            .iter()
            .flat_map(|q| q.uv.iter().map(|p| p[0]))
            .collect();
        let mut vs: Vec<f32> = quads
            .iter()
            .flat_map(|q| q.uv.iter().map(|p| p[1]))
            .collect();
        us.sort_by(f32::total_cmp);
        vs.sort_by(f32::total_cmp);
        let distinct = |values: &mut Vec<f32>| {
            values.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
            values.len()
        };
        assert_eq!(
            (distinct(&mut us), distinct(&mut vs)),
            (3, 3),
            "a spread rect has three coordinates an axis, a repeated one has two: {us:?} {vs:?}"
        );
        assert!(player.warnings().is_empty(), "{:?}", player.warnings());
    }

    /// The branches that keep the flat quad: a terrain that misses under the
    /// sheet, an element that asks for the normal only, and a sprite that does
    /// not set `MatchGrnd` at all. A partial drape would tear, so a miss leaves
    /// the whole sheet alone.
    #[test]
    fn a_ground_sheet_stays_flat_when_it_cannot_be_draped() {
        let miss = |_x: f32, _z: f32| None;
        let terrain: GroundHeightQuery<'_> = &miss;
        let mut missing = build(&draped_sheet(true, false));
        step_draped(&mut missing, Mat4::IDENTITY, Some(terrain));
        assert_eq!(missing.quads().len(), 1, "a miss keeps the flat quad");
        let flat = |x: f32, z: f32| Some(x + z);
        let terrain: GroundHeightQuery<'_> = &flat;
        let mut normal_only = build(&draped_sheet(true, true));
        step_draped(&mut normal_only, Mat4::IDENTITY, Some(terrain));
        assert_eq!(
            normal_only.quads().len(),
            1,
            "GrndNormOnly leaves the sheet to the normal-aligned frame"
        );
        let mut plain = build(&draped_sheet(false, false));
        step_draped(&mut plain, Mat4::IDENTITY, Some(terrain));
        assert_eq!(plain.quads().len(), 1, "MatchGrnd: 0 never drapes");
    }

    /// All 46 roots that carry a trail (element 152) play through the vertex
    /// ring, and the ring is doing real work on retail data rather than sitting
    /// at `Segs`: the 138 trails that author `isappend: 1` are laid down over
    /// their own duration and then lose their oldest vertices, so the live count
    /// moves from frame to frame. Nothing may hold more than the ring's 256
    /// records and no corner may go non-finite.
    #[test]
    fn corpus_trails_play_through_the_vertex_ring() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let (mut roots, mut trails, mut animated, mut nonfinite) = (0usize, 0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(probe) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let slots: Vec<usize> = probe
                .elements
                .iter()
                .enumerate()
                .filter(|(_, element)| matches!(element.spec, Spec::Trail(_)))
                .map(|(slot, _)| slot)
                .collect();
            if slots.is_empty() {
                continue;
            }
            trails += slots.len();
            let Ok(parsed) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let Ok(mut player) = Player::new(parsed, 7, &mut |_| Err("no children".into())) else {
                continue;
            };
            roots += 1;
            let mut lengths: Vec<Vec<usize>> = slots.iter().map(|_| Vec::new()).collect();
            for _ in 0..60 {
                if player.advance(33., &view, Mat4::IDENTITY, 1., 1.).is_err() {
                    break;
                }
                for (which, slot) in slots.iter().enumerate() {
                    if let Runtime::Fractal(fractal) = &player.elements[*slot].runtime {
                        assert!(
                            fractal.trail.records.len() <= TRAIL_RING_CAPACITY,
                            "the ring overflowed in {}",
                            file.display()
                        );
                        lengths[which].push(fractal.trail.records.len());
                    }
                }
                for quad in player.quads() {
                    for corner in &quad.positions {
                        if corner.iter().any(|component| !component.is_finite()) {
                            nonfinite += 1;
                        }
                    }
                }
            }
            for length in &lengths {
                let first = length.first().copied().unwrap_or(0);
                if length.iter().any(|live| *live != first) {
                    animated += 1;
                }
            }
        }
        println!(
            "trails: {roots} roots, {trails} elements, {animated} changed length, {nonfinite} non-finite"
        );
        assert_eq!(nonfinite, 0, "no trail corner may go non-finite");
        assert_eq!(roots, 46, "the corpus carries 46 roots with a trail");
        assert_eq!(trails, 156, "and 156 trail elements between them");
        // Exactly the 138 trails that author `isappend: 1` move; the other 18 are
        // rebuilt whole and hold one length for their entire life.
        assert_eq!(
            animated, 138,
            "the ring is live on the appending trails and static on the rest"
        );
    }

    /// All 459 roots that author a ground key still play with a query supplied,
    /// and the query is not a no-op on real data: 455 of them ask through
    /// `MatchGrnd` and four through `GroundHeight` alone. The ground is a flat
    /// plane at y = 0 tilted three degrees and the root stands 0.1 above it,
    /// inside the 0.2 that `0x6e54ac` measures, so `MatchGrnd` alone applies and
    /// the aligned frame is what those roots get.
    #[test]
    /// The world's placed effects are emission-driven: a root whose clock has
    /// not run yet draws nothing at all, which is exactly why a placement
    /// whose play speed reads as zero is invisible rather than merely
    /// still. These four shipped roots stand in for that property: empty at
    /// birth, drawing after half a second, and still changing afterwards.
    #[test]
    fn emitted_world_roots_draw_nothing_until_the_clock_runs() {
        let Some(root) = corpus_root() else { return };
        let view = view();
        for name in [
            "gfx/场景/新种族地图/瀑布.gfx",
            "gfx/场景/瀑布水花.gfx",
            "gfx/场景/美丽光柱.gfx",
            "gfx/场景/树叶雨.gfx",
        ] {
            let Ok(bytes) = std::fs::read(root.join(name)) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let archive = root.clone();
            let mut player = Player::new(parsed, 7, &mut |p: &str| {
                let b = std::fs::read(archive.join(p)).map_err(|e| format!("{p}: {e}"))?;
                let (s, _) = crate::gfx_file::decode(&b);
                crate::gfx_file::parse(&s)
            })
            .unwrap();
            let shape = |player: &Player| {
                player
                    .quads()
                    .iter()
                    .map(|quad| (quad.uv[0], quad.positions[0]))
                    .collect::<Vec<_>>()
            };
            assert!(
                player.quads().is_empty(),
                "{name}: drew before its clock ran"
            );
            player.advance(16., &view, Mat4::IDENTITY, 1., 1.).unwrap();
            let first = shape(&player);
            for _ in 0..29 {
                player.advance(16., &view, Mat4::IDENTITY, 1., 1.).unwrap();
            }
            let later = shape(&player);
            assert!(!later.is_empty(), "{name}: drew nothing after 480 ms");
            assert_ne!(first, later, "{name}: frozen after its first frame");
        }
    }

    fn corpus_plays_through_with_a_ground_query() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let view = view();
        let ground = GroundHit {
            height: 0.,
            normal: tilted_normal(3.),
        };
        let transform = Mat4::from_translation(Vec3::new(0., 0.1, 0.));
        let (mut played, mut changed, mut nonfinite) = (0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(probe) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let asks = probe
                .elements
                .iter()
                .any(|element| element.common.match_ground || element.common.ground_height != 0.);
            if !asks {
                continue;
            }
            let mut players = Vec::new();
            for _ in 0..2 {
                let Ok(parsed) = crate::gfx_file::parse(&source) else {
                    break;
                };
                match Player::new(parsed, 7, &mut |_| Err("no children".into())) {
                    Ok(player) => players.push(player),
                    Err(_) => break,
                }
            }
            if players.len() != 2 {
                continue;
            }
            let mut grounded = players.pop().expect("two players were built");
            let mut plain = players.pop().expect("two players were built");
            played += 1;
            let mut differs = false;
            for _ in 0..8 {
                if plain.advance(16., &view, transform, 1., 1.).is_err() {
                    break;
                }
                if grounded
                    .advance_with_ground(16., &view, transform, 1., 1., Some(ground))
                    .is_err()
                {
                    break;
                }
                assert_eq!(
                    plain.quads().len(),
                    grounded.quads().len(),
                    "the ground pass cannot change how much geometry a root emits"
                );
                for (index, quad) in plain.quads().iter().enumerate() {
                    let other = &grounded.quads()[index];
                    for corner in &other.positions {
                        if corner.iter().any(|component| !component.is_finite()) {
                            nonfinite += 1;
                        }
                    }
                    if quad.positions != other.positions {
                        differs = true;
                    }
                }
            }
            if differs {
                changed += 1;
            }
        }
        println!("corpus ground: {played} roots asked, {changed} changed");
        assert_eq!(
            nonfinite, 0,
            "the ground frames produced non-finite geometry"
        );
        assert!(
            played >= 450,
            "only {played} of the 459 ground-authoring roots played"
        );
        assert!(
            changed > 0,
            "no root's geometry changed, so the ground pass is inert"
        );
    }

    /// A root that shakes the camera: `ShakeCam` set, and the `CtrlType: 112`
    /// controller the factory table at `0x6eaddc` maps to the shake class. The
    /// five noise keys hold the constructor's own defaults — 256, 2.0, 1, 0.25
    /// and 5 — because that is what every corpus root that shakes authors,
    /// `Amplitude` aside, which they vary from 0.05 to 2.0.
    fn shake(amplitude: f32, start: f32, end: f32) -> String {
        header37(0).replace(
            "ShakeCam: 0\r\n",
            &format!(
                "ShakeCam: 1\r\nCtrlType: 112\r\nStartTime: {start}\r\nEndTime: {end}\r\n\
                 BufLen: 256\r\nAmplitude: {amplitude}\r\nWaveLen: 1\r\nPersistence: 0.25\r\n\
                 OctaveNum: 5\r\n"
            ),
        )
    }

    /// `0x6e4dc9` gates the whole driver on root+0x1f6, the `setne` of
    /// `ShakeCam`, so a root that does not ask never writes an offset — which is
    /// also what the 5,218 corpus roots that author no `CtrlType` at all do.
    #[test]
    fn a_root_without_shakecam_never_moves_the_camera() {
        let mut player = build(&header37(0));
        for frame in 0..8 {
            step(&mut player, 16.);
            assert_eq!(player.camera_shake(), Vec3::ZERO, "frame {frame}");
        }
    }

    /// `0x6e5015` reduces the frame counter modulo two, sign included, and
    /// `0x6e50f3` negates the stored offset in place on the odd frames, so the
    /// camera is pushed out and pulled back along the same vector: consecutive
    /// frames are exact negatives of one another, and the counter is incremented
    /// before the test, which makes the first frame in the window an even one.
    #[test]
    fn the_shake_alternates_the_sign_of_its_offset() {
        let mut player = build(&shake(1., 0., -1.));
        step(&mut player, 16.);
        let first = player.camera_shake();
        step(&mut player, 16.);
        let second = player.camera_shake();
        step(&mut player, 16.);
        let third = player.camera_shake();
        assert!(
            first != Vec3::ZERO,
            "an open window shakes from its first frame"
        );
        assert_eq!(second, -first, "the odd frame negates the stored offset");
        assert!(third != Vec3::ZERO);
        assert_ne!(third, first, "an even frame draws a fresh random direction");
    }

    /// `0x6e4dfb` leaves before `StartTime` and `0x6e4e0c` leaves after
    /// `EndTime`, and the exit path `0x6e5150` clears the latch on the way out,
    /// so the authored window alone decides whether the camera moves. A negative
    /// `EndTime` — the constructor's own default at `0x6f111f`, and what a root
    /// below version 21 gets, since `0x6eaef8` writes 0 and -1.0 instead of
    /// reading the two keys — skips the end test entirely.
    #[test]
    fn the_shake_runs_only_inside_its_window() {
        // `StartTime: 0.1` is 100 ms, so the first six 16 ms frames are outside.
        let mut late = build(&shake(1., 0.1, -1.));
        for frame in 0..6 {
            step(&mut late, 16.);
            assert_eq!(late.camera_shake(), Vec3::ZERO, "frame {frame} is early");
        }
        step(&mut late, 16.);
        assert_ne!(late.camera_shake(), Vec3::ZERO, "t = 112 ms is inside");

        let mut closed = build(&shake(1., 0., 0.05));
        step(&mut closed, 16.);
        assert_ne!(closed.camera_shake(), Vec3::ZERO, "t = 16 ms is inside");
        for _ in 0..4 {
            step(&mut closed, 16.);
        }
        assert_eq!(
            closed.camera_shake(),
            Vec3::ZERO,
            "t = 80 ms is past EndTime 0.05"
        );
    }

    /// `0x6e5037` compares root+0x29c against the 30.0 at `0x8eb710`: at or
    /// beyond it three zeroes are written, and below it the offset carries the
    /// `1 - distance / 30` that `0x6e5067` seeds into the noise call's second
    /// slot. Both players run from the same seed, so their directions match and
    /// only the fade differs.
    #[test]
    fn the_shake_fades_out_over_thirty_units() {
        let at = |distance: f32| View {
            position: Vec3::new(0., 0., distance),
            ..view()
        };
        let mut player = build(&shake(1., 0., -1.));
        player
            .advance(16., &at(30.), Mat4::IDENTITY, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            player.camera_shake(),
            Vec3::ZERO,
            "thirty units away is already out of reach"
        );

        let mut near = build(&shake(1., 0., -1.));
        let mut far = build(&shake(1., 0., -1.));
        near.advance(16., &at(10.), Mat4::IDENTITY, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
        far.advance(16., &at(15.), Mat4::IDENTITY, 1., 1.)
            .unwrap_or_else(|error| panic!("{error}"));
        let ratio = far.camera_shake().length() / near.camera_shake().length();
        assert!(
            (ratio - 0.75).abs() < 1e-5,
            "fifteen units is three quarters of ten: {ratio}"
        );
    }

    /// The amplitude is authored on the controller and the client's noise object
    /// applies it, so it scales the sample the way element 150's amplitude scales
    /// its offsets — the driver itself only multiplies by the fade and pushes the
    /// result along the direction.
    #[test]
    fn the_authored_amplitude_scales_the_offset() {
        let mut loud = build(&shake(1., 0., -1.));
        let mut quiet = build(&shake(0.5, 0., -1.));
        step(&mut loud, 16.);
        step(&mut quiet, 16.);
        let (loud, quiet) = (loud.camera_shake(), quiet.camera_shake());
        assert_ne!(loud, Vec3::ZERO);
        assert!(
            (quiet - loud * 0.5).length() <= 1e-6 * loud.length().max(1.),
            "Amplitude 0.5 should halve the offset: {loud} against {quiet}"
        );
    }

    /// The census behind the shake: 241 of the 5,459 corpus roots set `ShakeCam`,
    /// every one of them authors the `CtrlType: 112` controller the factory table
    /// maps to the shake class, and their noise keys are the constructor's
    /// defaults. This also plays them, which is the check that the offsets stay
    /// finite for real authored parameters rather than only for the fixture's.
    #[test]
    fn corpus_camera_shakes_all_author_the_noise_controller() {
        let Some(root) = corpus_root() else {
            return;
        };
        let mut files = Vec::new();
        corpus_files(&root, &mut files);
        let (mut shaking, mut without, mut defaults) = (0usize, 0usize, 0usize);
        let (mut built, mut moved, mut nonfinite) = (0usize, 0usize, 0usize);
        for file in &files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let (source, _) = crate::gfx_file::decode(&bytes);
            let Ok(parsed) = crate::gfx_file::parse(&source) else {
                continue;
            };
            if !parsed.shakes_camera() {
                continue;
            }
            shaking += 1;
            let Some(shake) = root_shake(&parsed) else {
                without += 1;
                continue;
            };
            if shake.buffer_length == 256
                && shake.wave_length == 1.
                && shake.persistence == 0.25
                && shake.octaves == 5
            {
                defaults += 1;
            }
            // Roots whose children this sweep cannot load are skipped the way the
            // other sweeps skip them.
            let Ok(reparse) = crate::gfx_file::parse(&source) else {
                continue;
            };
            let Ok(mut player) = Player::new(reparse, 7, &mut |_| Err("no children".into())) else {
                continue;
            };
            built += 1;
            // Two seconds covers every authored `StartTime`, the largest of which
            // is 1.7, at any `PlaySpeed` of 1 or faster.
            for _ in 0..125 {
                step(&mut player, 16.);
                let offset = player.camera_shake();
                if !offset.is_finite() {
                    nonfinite += 1;
                }
                if offset != Vec3::ZERO {
                    moved += 1;
                    break;
                }
            }
        }
        println!(
            "corpus shake: {shaking} roots ask, {built} played, {moved} moved, \
             {defaults} on the constructor defaults"
        );
        assert_eq!(
            without, 0,
            "a root that asks for a shake needs the CtrlType 112 controller"
        );
        assert_eq!(nonfinite, 0, "the shake produced a non-finite offset");
        assert!(
            shaking >= 230,
            "only {shaking} of the 241 roots were counted"
        );
        assert_eq!(
            defaults, shaking,
            "every shaking root leaves the noise keys at their defaults"
        );
        assert!(moved > 0, "no corpus root shook, so the driver is inert");
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
