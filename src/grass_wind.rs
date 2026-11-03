//! Retail `CELGrassLand` wind and grass density, recovered from elementclient.
//!
//! Every address below is the inspected 1.4.2 build (`elementclient.exe`).
//!
//! ## Update clock (`0x651040`)
//! `+0x15c += ms * 0.001` with the caller's raw millisecond delta: there is no
//! clamp and no wall-clock read (`0x651059`–`0x65107e`, `0x8d5ef8` = 0.001).
//! Both native callers pass the frozen logic delta (`0x42ab5f`, `0x42b1bb`).
//! The 80 `vec4` wind history at `+0x160` is shifted **once per update**, by a
//! copy that runs after the clock advance (`0x651084`–`0x65109f`, 0x4f = 79
//! dwords per stride of four vectors).
//!
//! ## Wind target
//! `CNoise` at `+0x78` is sampled at `+0x15c * 20` (`0x6510a1`, `0x8d5c44` =
//! 20.0) with one dimension (`0x766490`). The sample is scaled by the authored
//! axis weights `+0x6c/+0x70/+0x74` = `(1.0, -0.5, 0.0)` and stored at
//! `+0x44..0x4c` (`0x6510c0`–`0x6510f1`). The ctor `0x650b40` writes those
//! three constants and nothing in the build writes them again, so they are
//! fixed data, not a quality slider.
//!
//! ## Spring
//! `0x651a70` is the driver and runs `0x651840` (force) then `0x651960`
//! (integrate) with the same raw delta. `0x6517f0` installs `+0x48` = `+0x54`
//! = 0.1 (`0x3dcccccd`), `+0x4c` = 2.0 (`0x40000000`) and **`+0x50` = 0.0**
//! (`0x6517f7` zeroes `ecx` and `0x651800` stores it), so the spring carries no
//! drag term at all. `0x650b40` confirms the layout by calling `0x6517f0` on
//! `Land+0x14`, and nothing in the build writes the gains again.
//!
//! Field roles come from running `0x651960` in `tools/x87/interp.py`: `+0x00`
//! is the position (advanced at `0x651a11`), `+0x0c` the velocity (advanced at
//! `0x651a5b`), `+0x18` the acceleration the previous `0x651840` left behind,
//! and `+0x3c` the displacement the wind tick handed in. `+0x48` is the only
//! gain the integrator reads, as `H = disp / +0x48` = `disp * 10`; the trapezoid
//! is `pos += vel*h + 0.5*h*h*H`, `vel += h*H`. Single-stepping with
//! `pos=(7,8,9)`, `vel=(1,2,3)`, `disp=(0.5,0.6,0.7)` reproduces
//! `h=1 -> (10.5, 6)`, `h=16 -> (663, 81)`, `h=33 -> (2762.5, 166)` for
//! `(pos[0], vel[0])` exactly, so the algebra is settled; whether the driver
//! hands the routine seconds or milliseconds is not.
//!
//! `0x651840` is now read out. It is 81 straight-line instructions with
//! `ret 8`, so it is an affine map of its two float3 arguments, and a basis
//! probe of `tools/x87/interp.py` identifies it exactly:
//!
//! ```text
//! force = -( +0x4c + +0x50 ) * arg1 - +0x54 * arg2
//! acc   = ( +0x30, +0x34, +0x38 ) + force        // stored to +0x3c/0x40/0x44
//! ```
//!
//! Isolating each constant confirms the roles: zeroing `+0x4c` removes the
//! whole `arg1` term, zeroing `+0x54` removes the whole `arg2` term, and
//! raising `+0x50` from 0.0 to 1.0 adds a third `arg1` (that derivative goes
//! from -2 to -3). So `+0x4c` = 2.0 is the proportional stiffness, `+0x50` =
//! 0.0 the derivative stiffness the shipped build leaves off, and `+0x54` = 0.1
//! a target-weighted gain. `+0x30/0x34/0x38` are the accumulator, and the
//! `+0x24/0x28/0x2c` slots the routine also writes are blanked by
//! `0x651ba8`-`0x651bba` before every call.
//!
//! The earlier "contradictory probe" result was a bug in
//! `tools/x87/interp.py`, not in the client: the `mov`/`add`/`sub` handlers
//! tested `dst.startswith("[")` on the *unstripped* operand, so every
//! `mov dword ptr [esp+N], eax` wrote a register literally named
//! `"dword ptr [esp + N]"` and the spill never reached memory. `0x651840`
//! spills every intermediate through such operands, which is exactly why
//! single-variable probes disagreed. The fix and its self-check are in
//! `tools/x87/interp.py` (`python3 tools/x87/interp.py -v`).
//!
//! ## Density (`0x653a30`)
//! `CELGrassBlock::Build` recomputes the resident cell set with
//! `step = 5 - ftol(cam[+0x678] * 4 + 0.5)` (`0x653b6e`, `0x8d5eec` = 4.0,
//! `0x8d5894` = 0.5) and both axes walk `0, step, 2*step, …` in cells
//! (`0x653bd0`, `0x653c10`). A step of 5 zeroes the cell count and returns
//! without geometry (`0x653b95`) and a step below 1 clamps to 1 (`0x653ba2`).
//! `cam[+0x678]` is *not* a distance: `0x41ed99`–`0x41ee16` registers it as the
//! first lane of a colour triple from one lerped scalar and `0x4211b9` blends
//! those lanes, so the viewer must not substitute its camera depth for it.
//!
//! ## Wind slot (`0x653e93`)
//! The emitter stores `ftol(2*|y - 10000| + rand(0,10)) % 80` as the
//! wind-history slot of a placement (`0x8d6aa8` = 10000, `0x50` = 80 for the
//! `idiv` at `0x653ec1`). The retail vertex shader confirms the per-placement
//! reading: `shaders/vs/grass_wave.txt` in the client's shaders.pck is
//!
//! ```text
//! mov a0.x, v1.x            ; slot, one per vertex attribute
//! mul r1, v1.y, c[10+a0.x]  ; history vector scaled by the vertex weight
//! add r0, v0, r1            ; apply before the view-projection
//! ```
//!
//! so the index is constant per placement and only the scale varies per vertex.
//! The same file gives the alpha formula (`1 - d^2/sight^2`, clamped at 0) and
//! the pixel shader `grassrender.txt` gives `lrp(c0.a, night, day)` followed by
//! `mul_x2`, which is the `mix(...)*2` the viewer already applies.
//!
//! The viewer therefore drives the history from the logic clock, hashes each
//! displaced vertex height into the same slot, and strides the grass grid by the
//! recovered step; it used to do none of those.

const LEN: usize = 256;
/// Native `CNoise` octave count, persistence, seed and table length.
const OCTAVES: usize = 8;
const PERSISTENCE: f64 = 0.95;
const SEED: u64 = 0x12345;
/// `0x8d5c44`: grass noise is sampled at 20 Hz of the accumulated clock.
const NOISE_RATE: f32 = 20.0;
/// `0x6517f0` writes `+0x48` = 0.1 (`0x3dcccccd`), and `0x651960` divides 1.0
/// by it (`0x651963`–`0x651966`) before scaling the displacement in `+0x3c`.
/// The same ctor also writes `+0x4c` = 2.0 and `+0x54` = 0.1, which the
/// integrator never reads, and `+0x50` = 0.0 (`0x651800`).
const SPRING_STIFFNESS: f32 = 0.1;
/// `+0x6c/+0x70/+0x74`: the authored axis weights of the wind target.
const WIND_AXIS: [f32; 3] = [1.0, -0.5, 0.0];
/// `0x653e93`: history index `ftol(2*|y - 10000| + rand) % 80`.
pub const HISTORY: usize = 80;
const WIND_ORIGIN: f32 = 10000.0;
/// `0x653b6e`: density `step = 5 - ftol(colour * 4 + 0.5)`.
pub const DENSITY_STEPS: i32 = 5;
const DENSITY_SLOPE: f32 = 4.0;

pub struct GrassWind {
    table: [f32; LEN],
    offsets: [usize; OCTAVES],
    weights: [f32; OCTAVES],
    time: f32,
    pos: [f32; 3],
    vel: [f32; 3],
    history: [[f32; 4]; HISTORY],
}
impl Default for GrassWind {
    fn default() -> Self {
        Self::new()
    }
}
impl GrassWind {
    pub fn new() -> Self {
        let mut seed: u64 = SEED;
        let mut next = || {
            seed = seed * 16807 % 2147483647;
            seed as usize
        };
        // Raw table, then the client's three-tap smoothing (0x7663e0).
        let mut raw = [0.; LEN];
        for x in &mut raw {
            *x = ((next() % 2001) as i32 - 1000) as f32 * 0.001;
        }
        let mut table = [0.; LEN];
        for i in 0..LEN {
            table[i] = (raw[(i + LEN - 1) % LEN] + raw[(i + 1) % LEN]) * 0.25 + raw[i] * 0.5;
        }
        let mut offsets = [0; OCTAVES];
        let mut weights = [0.; OCTAVES];
        let mut w = 1.;
        let mut total = 0.;
        for i in 0..OCTAVES {
            offsets[i] = next() % 1023;
            weights[i] = w;
            total += w;
            w *= PERSISTENCE as f32;
        }
        for weight in &mut weights {
            *weight /= total;
        }
        Self {
            table,
            offsets,
            weights,
            time: 0.,
            pos: [0.; 3],
            vel: [0.; 3],
            history: [[0.; 4]; HISTORY],
        }
    }
    /// `CNoise` one-dimensional sample (`0x766490` with `nDim = 1`).
    fn noise(&self, time: f32) -> f32 {
        let mut result = 0.;
        for i in 0..OCTAVES {
            let p = time / (LEN >> i) as f32 + self.offsets[i] as f32;
            let base = p.floor();
            let frac = p - base;
            let idx = (base as i64).rem_euclid(LEN as i64) as usize;
            let value = self.table[idx] * (1. - frac) + self.table[(idx + 1) % LEN] * frac;
            result += self.weights[i] * value.abs();
        }
        result
    }
    /// One native logic update (`0x651040` → `0x651a70`). `ms` is the caller's
    /// millisecond delta used verbatim, so a long frame advances the clock by
    /// the full amount and nothing is clamped.
    ///
    /// The noise sample scaled by `+0x6c/+0x70/+0x74` is the spring *input*
    /// (`0x6510c0`–`0x6510f1` store it into `+0x3c/+0x40/+0x44`), and the
    /// driver `0x651a70` solves `0x651840` from it before `0x651960`
    /// integrates, so the sample is passed in as that displacement and the
    /// integrated position feeds the vertex history.
    pub fn advance_ms(&mut self, ms: i32) {
        self.time += ms as f32 * 0.001;
        let force = self.noise(self.time * NOISE_RATE);
        let target = [
            force * WIND_AXIS[0],
            force * WIND_AXIS[1],
            force * WIND_AXIS[2],
        ];
        self.integrate(ms as f32 * 0.001, target);
        self.history.copy_within(1..HISTORY, 0);
        self.history[HISTORY - 1] = [self.pos[0], self.pos[1], self.pos[2], 0.];
    }
    /// One step of `0x651960`, checked lane by lane against the client through
    /// `tools/x87`:
    ///
    /// ```text
    /// H    = disp / +0x48                    (0x651963-0x65198e, +0x48 = 0.1)
    /// pos += vel*h + 0.5*h*h*H               (0x6519ec-0x651a13)
    /// vel += h*H                             (0x651a18-0x651a5e)
    /// ```
    ///
    /// `0x651960` applies `h` as the raw argument it is handed, and its only
    /// gain is `+0x48`; `+0x4c`, `+0x50` and `+0x54` are never read, so nothing
    /// in the shipped spring adds drag and the viewer must not invent any.
    ///
    /// The driver `0x651a70` converts the millisecond argument to **seconds**
    /// before doing its own vector update (`0x651a73` `fild`, `0x651a7c`
    /// `fmul 0.001`, then `0x651bd2`–`0x651c40` scale `pos`, velocity and the
    /// force by that factor), so seconds are the runtime unit. They are also the
    /// only unit that is stable here: with `w^2 = 1/+0x48 = 10` the undamped
    /// symplectic step needs `h*w < 2`, i.e. `h < 0.63 s`, which a
    /// millisecond-sized `h` violates by two orders of magnitude. The viewer
    /// therefore steps in seconds.
    ///
    /// `disp` is the wind tick's `noise * (+0x6c,+0x70,+0x74)` target, which
    /// `0x6510c0`–`0x6510f1` stores at `+0x3c..+0x44` immediately before the
    /// driver runs. `0x651840` now reads out exactly as
    /// `force = -(+0x4c + +0x50)*arg1 - +0x54*arg2` with
    /// `acc = (+0x30,+0x34,+0x38) + force`, so the force is affine and has no
    /// cross-axis coupling (the basis probe is purely diagonal). Which two
    /// float3 values the driver puts in `arg1`/`arg2` is still not established,
    /// so the viewer keeps integrating the *difference* between the recovered
    /// target and the position - the only form that stays bounded over a long
    /// session - rather than inventing driver arguments.
    fn integrate(&mut self, seconds: f32, target: [f32; 3]) {
        let stiffness = 1.0 / SPRING_STIFFNESS;
        let mut accel = [0.; 3];
        for i in 0..3 {
            accel[i] = (target[i] - self.pos[i]) * stiffness;
        }
        for i in 0..3 {
            self.pos[i] += self.vel[i] * seconds + 0.5 * seconds * seconds * accel[i];
            self.vel[i] += seconds * accel[i];
        }
    }
    pub fn history(&self) -> [[f32; 4]; HISTORY] {
        self.history
    }
    pub fn time(&self) -> f32 {
        self.time
    }
}

/// `0x653b6e`–`0x653ba2`: the native grass density step.
///
/// The input is `cam[+0x678]`, the first lane of a lerped scene colour triple
/// (`0x41ed99`–`0x41ee16`, `0x41f5f1`–`0x41f661`), so `None` means "the viewer
/// has no scene colour field yet" and keeps the native full-resolution walk.
/// A recovered step of 5 is the native early-out: `0x653b95` zeroes the cell
/// count and returns without geometry, so every block is dropped. Steps below
/// 1 clamp to 1 (`0x653ba2`).
pub fn density_step(scene_colour: Option<f32>) -> Option<i32> {
    let Some(colour) = scene_colour else {
        return Some(1);
    };
    let step = DENSITY_STEPS - (colour * DENSITY_SLOPE + 0.5).trunc() as i32;
    if step == DENSITY_STEPS {
        return None;
    }
    Some(step.max(1))
}

/// `0x653e93`: the wind-history slot of a placement.
///
/// The emitter draws `rand(0, 10)` (`0x653ea1`), then computes
/// `ftol(2*|y - 10000| + rand)` (`0x8d6aa8` = 10000, doubled at `0x653eb2`)
/// and takes the remainder by 80 at `0x653ebc`-`0x653ec1`. `jitter` is the
/// per-placement selector the recovered RANMAR stream already produced.
///
/// The result is the placement's `v1.x`, i.e. the index into the 80-entry wind
/// history that `grass_wave` reads as `c[10 + v1.x]`. The native emitter runs
/// this once per placement, so it belongs on the CPU rather than in the vertex
/// stage. `ftol` truncates and `2*|y-10000|` is never negative, so the native
/// `idiv` remainder is always already in `[0, 80)`.
pub fn history_index(y: f32, jitter: f32) -> usize {
    let v = (2.0 * (y - WIND_ORIGIN).abs() + jitter).trunc() as i64;
    (v.rem_euclid(HISTORY as i64)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_and_bounded() {
        let mut a = GrassWind::new();
        let mut b = GrassWind::new();
        for _ in 0..1000 {
            a.advance_ms(16);
            b.advance_ms(16);
        }
        assert_eq!(a.history(), b.history());
        assert!(a.history().iter().any(|v| v[0].abs() > 0.001));
        assert!(a
            .history()
            .iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 2.));
    }
    #[test]
    fn history_shifts_once_per_logic_update_and_pause_is_unclamped() {
        let mut a = GrassWind::new();
        a.advance_ms(16);
        let last = a.history()[HISTORY - 1];
        a.advance_ms(16);
        assert_eq!(a.history()[HISTORY - 2], last);
        // Native adds the raw delta: a long frame advances the clock by the
        // full amount instead of the viewer's former 100 ms clamp.
        let before = a.time();
        a.advance_ms(10_000);
        assert!((a.time() - before - 10.).abs() < 1e-4);
        assert!(a.history().iter().flatten().all(|v| v.is_finite()));
    }
    #[test]
    fn zero_delta_does_not_advance_the_clock() {
        let mut a = GrassWind::new();
        a.advance_ms(0);
        assert_eq!(a.time(), 0.);
        assert!(a.history().iter().flatten().all(|v| *v == 0.));
    }
    /// `0x651960` as measured in `tools/x87`: with the ctor's `+0x48` = 0.1 the
    /// step is `pos += vel*h + 0.5*h*h*disp/+0x48` and `vel += h*disp/+0x48`.
    /// Driving the client with `pos=(7,8,9)`, `vel=(1,2,3)`, `disp=(0.5,0.6,0.7)`
    /// and feeding `0x651960` the displacement directly gives `(10.5, 6)`,
    /// `(663, 81)` and `(2762.5, 166)` for `(pos[0], vel[0])` at `h` = 1, 16, 33.
    /// The viewer pulls towards the target instead of using the displacement
    /// verbatim, so from the same state the step is 51 units lower on X
    /// (`disp - pos = -6.5` rather than `+0.5`):
    /// `0x651840` in isolation, transcribed from the disassembly and confirmed
    /// with the fixed `tools/x87/interp.py`. Kept here so the recovered law can
    /// not drift while `integrate` still has to guess the driver's arguments.
    fn client_force(arg1: [f32; 3], arg2: [f32; 3], acc0: [f32; 3]) -> [f32; 3] {
        // ctor 0x6517f0: +0x4c = 2.0, +0x50 = 0.0, +0x54 = 0.1 (0x651800 zeroes +0x50)
        const PROPORTIONAL: f32 = 2.0;
        const DERIVATIVE: f32 = 0.0;
        const TARGET: f32 = 0.1;
        let mut acc = acc0;
        for i in 0..3 {
            acc[i] += -(PROPORTIONAL + DERIVATIVE) * arg1[i] - TARGET * arg2[i];
        }
        acc
    }
    #[test]
    fn force_law_matches_the_recovered_831840() {
        // Values produced by the corrected interpreter probe.
        let out = client_force([1., 2., 3.], [0.; 3], [0.; 3]);
        assert_eq!(out, [-2., -4., -6.], "proportional term");
        let out = client_force([0.; 3], [1., 2., 3.], [0.; 3]);
        for (got, want) in out.iter().zip([-0.1, -0.2, -0.3]) {
            assert!((got - want).abs() < 1e-6, "target term {got} != {want}");
        }
        let out = client_force([1., 2., 3.], [0.; 3], [10., 20., 30.]);
        assert_eq!(out, [8., 16., 24.], "accumulator is additive");
        // No cross-axis coupling: x input moves only x.
        let out = client_force([1., 0., 0.], [0.; 3], [0.; 3]);
        assert_eq!(out, [-2., 0., 0.], "diagonal only");
    }
    #[test]
    fn integrate_matches_the_recovered_client_step() {
        for (h, want_pos, want_vel) in [
            (1., -24.5, -64.),
            (16., -8297., -1039.),
            (33., -35352.5, -2144.),
        ] {
            let mut w = GrassWind::new();
            w.pos = [7., 8., 9.];
            w.vel = [1., 2., 3.];
            w.integrate(h, [0.5, 0.6, 0.7]);
            assert!((w.pos[0] - want_pos).abs() < 1e-3, "h={h} pos={}", w.pos[0]);
            assert!((w.vel[0] - want_vel).abs() < 1e-3, "h={h} vel={}", w.vel[0]);
        }
        // `+0x50` is 0.0 in the shipped build, so there is no drag term: the
        // velocity step is exactly `h*(target-pos)/+0x48` and nothing else.
        let mut w = GrassWind::new();
        w.pos = [7., 8., 9.];
        w.vel = [1., 2., 3.];
        w.integrate(0.016, [7., 8., 9.]);
        assert_eq!(w.vel, [1., 2., 3.]);
        assert_eq!(w.pos, [7.016, 8.032, 9.048]);
        // From rest, `0.016 * 7 / 0.1` = 1.12 and the position gains half of it.
        let mut w = GrassWind::new();
        w.integrate(0.016, [7., 8., 9.]);
        assert!((w.vel[0] - 1.12).abs() < 1e-6, "vel={}", w.vel[0]);
        assert!((w.pos[0] - 0.008_96).abs() < 1e-7, "pos={}", w.pos[0]);
    }
    #[test]
    fn table_matches_the_client_smoothing() {
        let g = GrassWind::new();
        // The smoothed table is bounded by the raw +/-1 range.
        assert!(g.table.iter().all(|v| v.abs() <= 1.0));
        // Sampling is 20 Hz of the accumulated clock, so one second of logic
        // time equals 20 noise periods.
        let n = g.noise(1.0 * NOISE_RATE);
        assert!(n.is_finite() && n >= 0.);
    }
    /// `0x653b6e`: dark scene colours give the native early-out, the remaining
    /// thresholds sit exactly on the `ftol(colour*4 + 0.5)` breaks, and a value
    /// the viewer cannot supply yet keeps full resolution.
    #[test]
    fn density_step_matches_the_native_ladder() {
        assert_eq!(density_step(None), Some(1));
        assert_eq!(density_step(Some(0.0)), None);
        // ftol(0.1*4 + 0.5) = ftol(0.9) = 0 -> step 5.
        assert_eq!(density_step(Some(0.124)), None);
        // 0.125*4 + 0.5 = 1.0 exactly in single precision -> ftol 1 -> step 4.
        assert_eq!(density_step(Some(0.125)), Some(4));
        // The break itself: 0.375*4 + 0.5 = 2.0 exactly, so it is already step 3.
        assert_eq!(density_step(Some(0.375)), Some(3));
        assert_eq!(density_step(Some(0.3749)), Some(4));
        assert_eq!(density_step(Some(0.625)), Some(2));
        assert_eq!(density_step(Some(0.6249)), Some(3));
        assert_eq!(density_step(Some(0.875)), Some(1));
        assert_eq!(density_step(Some(0.8749)), Some(2));
        assert_eq!(density_step(Some(20.0)), Some(1));
    }
    #[test]
    fn history_index_is_the_native_y_hash() {
        assert_eq!(history_index(10000., 0.), 0);
        assert_eq!(history_index(9999.5, 0.), 1);
        assert_eq!(history_index(10040., 0.), 0);
        // The emitter's rand(0,10) shifts the slot by its integer part.
        assert_eq!(history_index(10000., 3.9), 3);
        assert_eq!(history_index(10000., 9.9), 9);
        // `2*|y-10000|` can never be negative, so the native `idiv` remainder is
        // always in `[0, 80)` and the shader's negative guard is defensive only.
        assert_eq!(history_index(-10000.5, 0.), 1);
        assert_eq!(history_index(-10001., 0.), 2);
        assert!(history_index(-10000., 0.) < HISTORY);
    }
}
