//! Native sky resource swapping and its transition machine (P5.07 / P5.08).
//!
//! Basis: `elementclient.exe`, MD5 `5f02895688fa580506e869156340a664`, PE i386,
//! image base `0x400000`. Every address, offset and branch below was read from
//! that image with `objdump -d`; nothing here is inferred from the viewer.
//!
//! # Object layout (`A3DSkySphere`)
//!
//! | offset | meaning |
//! |---|---|
//! | `+0x0c` | owning scene pointer; zero means "not initialised yet" |
//! | `+0x4cc`, `+0x504` | live interpolation blocks (7 and 16 dwords) |
//! | `+0x4e8`, `+0x544` | incoming interpolation blocks |
//! | `+0x5a4`, `+0x5a8`, `+0x5ac` | current picture handles |
//! | `+0x5ba`, `+0x6be`, `+0x7c2` | the three name blocks (current, and two pending) |
//! | `+0x8c8`, `+0x8cc`, `+0x8d0` | pending picture handles |
//! | `+0x5b0` | bound |
//! | `+0x5b4` | counter |
//! | `+0x5b8` | direction byte: `0` counts **up**, `1` counts **down** |
//! | `+0x5b9` | "textures still loading" byte |
//!
//! # Entry points
//!
//! **`0x79ac30(t0,t1,t2,matrix)` — the guard.** Returns `false` when a swap is
//! already running (`+0x5b4 != 0`, `0x79ac3a`) or loading (`+0x5b9 != 0`,
//! `0x79ac48`). Otherwise it formats the three requested names, returns `true`
//! when any current handle is null (`0x79ac9e`..`0x79ad70`) or when the live
//! interpolation block differs from the caller's matrix
//! (`0x79acd6`..`0x79ad13`, four float compares against `+0x4cc`), and returns
//! `false` at `0x79ad7d` when every requested name already equals the current
//! handle. It never installs anything: the caller uses the answer to decide
//! whether the state really changes.
//!
//! **`0x79aeb0(t0,t1,t2,matrix,count)` — the swap.** Returns immediately (with
//! `true`) when the object has no scene (`0x79aebc`) or is still loading
//! (`0x79aec7`). It sets direction `1` (`0x79aeed`), then:
//!
//! * requested names equal the **current** handles (`0x79af24`..`0x79af99`) ->
//!   copy the interpolation block into `+0x4cc`, set direction **0**
//!   (`0x79afb4`), return. Counter untouched: a no-op re-request of the live
//!   set cannot start a fade.
//! * requested names equal the **pending** handles (`0x79afc9`..`0x79b035`) ->
//!   copy the block into `+0x4e8`, set direction **1** (`0x79b050`), return.
//! * otherwise (`0x79b065`) it copies the incoming block into `+0x4e8`, stores
//!   the three names, sets `+0x5b9 = 1`, masks a non-positive count to zero
//!   (`setle`/`dec`/`and` at `0x79b0d0`..`0x79b0dd`), writes that value to both
//!   `+0x5b0` and `+0x5b4` (`0x79b0e1`, `0x79b0e7`), and either schedules the
//!   loader `0x79ad90` on a timer (`0x79b11f`) or, for a zero count, runs it
//!   inline and commits straight away (`0x79b0f0`..`0x79b10b`).
//!
//! **`0x79a040` — the tick.** Returns at `0x79a0d5` when `+0x5b4 == 0`
//! (`0x79a069`) or `+0x5b9 != 0` (`0x79a06d`). Both interpolation blocks are
//! advanced first (`0x79a05e`, `0x79a087`, through `0x79a0e0`). Then, with
//! direction `0`: `counter += 33` and commit once `counter >= bound`
//! (`0x79a0a9`..`0x79a0c2`); with direction `1`: `counter -= 33` and commit only
//! when the result is negative (`0x79a096`..`0x79a0a5`). `0x79a0c6` zeroes the
//! counter on both commit paths before calling `0x79b180`.
//!
//! **`0x79b150(delta)` — hurry.** Inert without a counter (`0x79b156`),
//! otherwise `counter -= delta`, clamped up to `1` when the result reaches zero
//! or below (`0x79b15a`..`0x79b16f`), so the next tick commits.
//!
//! **`0x79b180` — commit.** Pending handles into `+0x5a4/+0x5a8/+0x5ac`, the
//! incoming blocks over the live ones, pending slots cleared.
//!
//! # Driver `0x4203f4..0x420807`
//!
//! Returns while `+0x5b4 != 0` (`0x420404`), then loads the phase scalar at
//! `config+0x70` (`0x420415`) and the three names of two triples:
//! `[ebp+0x100/+0x104/+0x108]` (day) and `[ebp+0x10c/+0x110/+0x114]` (night):
//!
//! | phase | byte at `0x4204db` | action |
//! |---|---|---|
//! | `0` | any | `0x79ac30` then `0x79aeb0(count = 0)` with the day triple (`0x420430`) |
//! | `1` | any | same with the night triple (`0x420477`) |
//! | `0` after `0.8` check (`0x8d62dc`) | `0` | day triple, `count = ftol(clock/2 * phase * 1.25)` (`0x42054e`) |
//! | `>= 0.5` (`0x8d5894`) | `0` | night triple, `count = ftol(clock/2 * (1-phase) * 2)` (`0x420666`) |
//! | `0.8..1` | `0` | nothing (`0x420738`) |
//! | `< 0.5` | `0` | nothing (`0x4207aa`) |
//! | otherwise | `!= 0` | pre-swap the night triple uncounted, then the day/night triple with the clock count (`0x4204f5`, `0x42060d`) |
//!
//! Every counted branch then blocks on `+0x5b9` with 10 ms sleeps
//! (`0x4205c0`..`0x4205e8`, `0x4206dc`..`0x420704`) and finally calls
//! `0x79b150(hurry)` with `clock/2 - count` (`0x4205ea`..`0x420603`,
//! `0x420706`..`0x42071f`).
//!
//! `0x4b6250` returns `0x249f00` (2 400 000) or `0x1b7740` (1 800 000),
//! switching on the float at `config+0x74` (`fcomps 0x8d58a0`, i.e. against
//! zero): a 40- or 30-minute game day expressed in milliseconds.
//!
//! # Why the count is a duration (the cadence that used to be missing)
//!
//! The driver divides that day length straight into the counter, and `0x79a040`
//! moves the counter by a flat 33 per call, so the two units only line up if the
//! tick runs once per rendered frame and 33 is a millisecond step. That is what
//! the object says: `0x8f14bc` holds `1/30` (`0x3d088889`), the factor
//! `0x79a0e0` applies to both interpolation blocks, i.e. exactly one Voodoo-3
//! era frame. And `0x79a040` is reached *only* from its own vtable slot
//! `0x8f14b0` (vtable base `0x8f1490`), so it is the object's owned per-frame
//! update — there is no other caller and no config-sourced cadence (the binary
//! has no frame-rate string at all).
//!
//! So: one tick = one frame = 33 ms of fade, and a transition lasts `count`
//! milliseconds. With `count = clock/2 * phase * 1.25` on the day set and
//! `clock/2 * (1 - phase) * 2` on the night set, the counted bands
//! (`0.5..0.8` on the day branch, `0.5..1` on the night one) give a supremum of
//! half a game day: 900 000 ms of fade on the 30-minute setting, i.e. 15 game
//! minutes and **900 seconds of real time**. (The remaining half of the day is
//! covered by the uncounted endpoint switch at `phase <= 0` / `phase >= 1`, not
//! by a longer fade.)
//!
//! The fades are therefore minutes of wall time, not milliseconds, so
//! [`Machine::step`] has to be driven by elapsed time rather than called once per
//! rendered frame: [`Machine::step_ms`] is what the viewer uses, and it replaces
//! the stand-in clock constant this module used to carry.
//!
//! # Deliberately not invented
//!
//! * What the scalar at `config+0x70` and the byte at `0x4204db` physically are
//!   in the client. The viewer feeds both from its model day/night state
//!   ([`drive`] takes them as parameters).
//! * The per-slice colour weights inside the interpolation blocks.
//!   [`Machine::blend`] is the viewer's linear reconstruction.
//! * `TransSky()` (`0x98c0f8`), the debug entry point that logs a failed load.
//!   The viewer has no console and simply keeps the current set.

/// Slice step of the native counter (`add $0xffffffdf`, `0x79a09c`;
/// `add $0x21`, `0x79a0b5`).
pub const SLICE_MS: u32 = 33;
/// `0x8f14bc`: one slice in seconds, the factor `0x79a0e0` applies to both
/// interpolation blocks. Read back as `1/30` it is the frame interval, which is
/// what makes the counter a millisecond counter (see [`TICKS_PER_SECOND`]).
#[cfg(test)]
pub const SLICE_SECONDS: f32 = f32::from_bits(0x3d088889);
/// `0x8d6808`: the `1.25` factor in the day count (`0x42056a`).
pub const DAY_COUNT_SCALE: f32 = 1.25;
/// `0x8d62dc` / `0x8d5894`: the two phase thresholds of the byte-`0` branches.
pub const DAY_LATE_PHASE: f32 = 0.8;
pub const NIGHT_ONSET_PHASE: f32 = 0.5;

/// `0x4b6250`: the length of a game day in milliseconds, 40-minute setting
/// (`config+0x74 != 0`).
pub const DAY_MS_LONG: i32 = 0x249f00;
/// `0x4b6250`: the same for the 30-minute setting (`config+0x74 == 0`).
pub const DAY_MS_SHORT: i32 = 0x1b7740;
/// The clock value the viewer feeds to [`drive`]: the 30-minute day, the one the
/// recovered default configuration selects. This is the real unit again —
/// a count from [`slice_count`] is a number of milliseconds, not a stand-in.
pub const DAY_CLOCK_MS: i32 = DAY_MS_SHORT;
/// Ticks per second of wall time: the frame interval `0x8f14bc` holds is `1/30` s.
pub const TICKS_PER_SECOND: u64 = 30;
/// Upper bound on the ticks one [`Machine::step_ms`] call may take, so a stalled
/// frame cannot replay a whole fade synchronously.
pub const MAX_TICKS_PER_STEP: u64 = 4096;

/// The clock in the unit `0x4b6250` returns, from a game-day length in minutes.
///
/// The client has exactly two settings (`+0x74` against zero), so this snaps to
/// the nearer of them rather than pretending a day length is continuous.
pub fn clock_for_minutes(minutes: f32) -> i32 {
    if minutes >= 35. {
        DAY_MS_LONG
    } else {
        DAY_MS_SHORT
    }
}

/// One authored three-image sky set. The viewer's loader holds the three
/// `configs/autofamilyconfigs/sky.lst` rows in file order:
/// `0 = 晴天 (clear)`, `1 = 阴天01 (overcast)`, `2 = 夕阳 (sunset)`.
/// `autoscene.cfg` names `sky_id_day = 0` and `sky_id_night = 1` for this
/// build, so the night swap is to the overcast set, not to a painted night
/// panorama. The set paths are configuration data (`Textures\Sky\%s` at
/// `0x98c0e8`), not a filename pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triple(pub u8);

impl Triple {
    /// Day set (`sky_id_day = 0`, clear).
    pub const DAY: Triple = Triple(0);
    /// Night set (`sky_id_night = 1`, overcast). The client darkens it with the
    /// regional light record; it is not a separate painted night texture.
    pub const NIGHT: Triple = Triple(1);
    pub const ALL: [Triple; 3] = [Triple(0), Triple(1), Triple(2)];

    pub fn valid(self) -> bool {
        (self.0 as usize) < Self::ALL.len()
    }
    /// Texture-set folder for the viewer's own loader, in `sky.lst` order.
    pub fn folder(self) -> &'static str {
        ["clear", "阴天01", "夕阳"][self.0 as usize % Self::ALL.len()]
    }
}

/// A swap request in the same units `0x79aeb0` receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub to: Triple,
    /// Native count. `0` is the immediate-commit branch.
    pub slices: u32,
}

impl Request {
    pub fn now(to: Triple) -> Self {
        Self { to, slices: 0 }
    }
    pub fn over(to: Triple, slices: u32) -> Self {
        Self { to, slices }
    }
    /// A fade of `ms`, on the native slice grid and off the exact-multiple
    /// stall (see [`Machine::on_commit_boundary`]).
    pub fn duration_ms(to: Triple, ms: u32) -> Self {
        Self {
            to,
            slices: ms / SLICE_MS * SLICE_MS + 1,
        }
    }
}

/// Why a [`Machine::request`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Unknown set index; the loader would have no name to load.
    InvalidSet,
    /// `+0x5b4 != 0` or `+0x5b9 != 0` (`0x79ac3a`, `0x79ac48`).
    Busy,
    /// Requested set is already current (`0x79ad7d`).
    AlreadyCurrent,
}

/// The native sky-swap state machine. One instance per sky object; step it
/// exactly once per frame from the single per-frame snapshot, never from a
/// reflection, refraction, clipped or map view.
#[derive(Clone, Debug)]
pub struct Machine {
    /// Which loaded set stands in for the client's day triple
    /// (`[+0x100/+0x104/+0x108]`) and night triple (`[+0x10c/+0x110/+0x114]`).
    ///
    /// The client resolves both from `Textures\Sky\%s` and the `sky_id_day` /
    /// `sky_id_night` keys of `configs.pck`, which re-author per region. The
    /// viewer now reads those keys through `angelica_pck` (the shipped
    /// `configs.pck` is an Angelica package, not the retail v2 container) and
    /// installs the `sky.lst` rows they name. [`Machine::author`] is the single
    /// place a region mapping lands.
    day: Triple,
    night: Triple,
    current: Triple,
    pending: Triple,
    bound: u32,
    counter: u32,
    /// `+0x5b8`: `true` counts down (decrement 33, commit when negative),
    /// `false` counts up (increment 33, commit at `>= bound`).
    count_down: bool,
    /// `+0x5b9`.
    waiting: bool,
    commits: u32,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new(Triple::DAY)
    }
}

impl Machine {
    pub fn new(current: Triple) -> Self {
        Self {
            day: Triple::DAY,
            night: Triple::NIGHT,
            current,
            pending: current,
            bound: 0,
            counter: 0,
            // Constructor `0x799743` leaves `+0x5b8 = 1`.
            count_down: true,
            waiting: false,
            commits: 0,
        }
    }

    /// Install the region-authored day/night pairing. Native only replaces the
    /// names it will swap to, never the ones already installed, so this refuses
    /// while a swap runs.
    pub fn author(&mut self, day: Triple, night: Triple) -> bool {
        if self.running() || self.waiting || !day.valid() || !night.valid() || day == night {
            return false;
        }
        self.day = day;
        self.night = night;
        true
    }

    pub fn day_set(&self) -> Triple {
        self.day
    }
    pub fn night_set(&self) -> Triple {
        self.night
    }

    pub fn current(&self) -> Triple {
        self.current
    }
    pub fn pending(&self) -> Triple {
        self.pending
    }
    /// `+0x5b4 != 0`, the guard at `0x79ac3a`, `0x420404` and `0x79a069`.
    pub fn running(&self) -> bool {
        self.counter != 0
    }
    pub fn waiting(&self) -> bool {
        self.waiting
    }
    pub fn counter(&self) -> u32 {
        self.counter
    }
    pub fn bound(&self) -> u32 {
        self.bound
    }
    pub fn counts_down(&self) -> bool {
        self.count_down
    }
    pub fn commits(&self) -> u32 {
        self.commits
    }

    /// Whether a count-down tick would commit on the next call.
    ///
    /// `0x79a09c` subtracts 33 and `0x79a0a5` branches on the sign flag, so the
    /// commit happens exactly when `counter < 33`. A counter that reaches zero
    /// exactly is indistinguishable from "no transition" at `0x79a069` and the
    /// swap then never commits - a real native stall that
    /// [`Request::duration_ms`] avoids and [`Self::step`] keeps observable
    /// instead of papering over.
    pub fn on_commit_boundary(&self) -> bool {
        self.counter != 0 && self.count_down && self.counter < SLICE_MS
    }

    /// Finish the outstanding load. `0x79ad90` clears `+0x5b9` on success
    /// (`0x79ae96`) and leaves the wait pending on failure (`0x79ae86`); the
    /// viewer's sets are resident, so it resolves on the next frame.
    pub fn resources_ready(&mut self) -> bool {
        let was = self.waiting;
        self.waiting = false;
        was
    }

    /// `0x79aeb0`. Returns the refusal reason instead of the native `true`
    /// no-op, so a caller cannot mistake "busy" for "swapped".
    pub fn request(&mut self, request: Request) -> Result<(), Refused> {
        if !request.to.valid() {
            return Err(Refused::InvalidSet);
        }
        if self.running() || self.waiting {
            return Err(Refused::Busy);
        }
        if request.to == self.current {
            // Requested names equal the current handles (0x79af24..0x79af99):
            // targets are refreshed, direction becomes 0, no fade starts.
            self.pending = request.to;
            self.count_down = false;
            return Err(Refused::AlreadyCurrent);
        }
        self.pending = request.to;
        // The fallback path keeps the direction written at 0x79aeed.
        self.count_down = true;
        // 0x79b0d0: loading; 0x79b0d9: a non-positive count masks to zero.
        self.waiting = true;
        self.bound = request.slices;
        self.counter = request.slices;
        if request.slices == 0 {
            // 0x79b0f0: load inline, then commit (0x79b10b).
            self.waiting = false;
            self.commit();
        }
        Ok(())
    }

    /// `0x79a040`. Call once per frame; returns `true` when the swap committed.
    pub fn step(&mut self) -> bool {
        // 0x79a069 / 0x79a06d: no counter, or still loading.
        if self.counter == 0 || self.waiting {
            return false;
        }
        if self.count_down {
            if self.counter < SLICE_MS {
                // 0x79a096: subtract, then commit on the sign flag.
                self.commit();
                return true;
            }
            self.counter -= SLICE_MS;
            // Landing exactly on zero stalls natively; left visible.
            false
        } else {
            // 0x79a0a9: add, compare against the bound.
            let next = self.counter.saturating_add(SLICE_MS);
            self.counter = next;
            if next >= self.bound {
                self.commit();
                return true;
            }
            false
        }
    }

    /// The same tick, driven by elapsed wall time instead of by a frame count.
    ///
    /// One native tick is one frame and `0x8f14bc` says a frame is `1/30` s, so a
    /// second of real time is 30 ticks. `residual` carries the sub-tick remainder
    /// in **milli-thirty-seconds** (`0..999`), so the accounting is exact integer
    /// arithmetic: thirty ticks are exactly 1000 units. A float accumulator would
    /// floor `1000 / (1000/30)` to 29 and lose a tick per second, which over a
    /// several-minute fade is a visible lag behind the game clock. Returns how
    /// many ticks were taken.
    ///
    /// This exists because a native fade is 15–100 % of a game day: replaying it
    /// once per rendered frame would make the duration depend on the frame rate.
    pub fn step_ms(&mut self, elapsed_ms: u32, residual: &mut u64) -> u32 {
        if elapsed_ms == 0 {
            return 0;
        }
        *residual += elapsed_ms as u64 * TICKS_PER_SECOND;
        let ticks = *residual / 1000;
        *residual %= 1000;
        // A fade never needs more ticks than its own counter, and a long stall (a
        // loading screen, a debugger break) must not spin here for minutes; the
        // excess is dropped, and `0x79b16f`-like clamping is not needed because a
        // commit clears the counter.
        let ticks = ticks.min(MAX_TICKS_PER_STEP) as u32;
        for _ in 0..ticks {
            if self.step() {
                break;
            }
        }
        ticks
    }

    /// `0x79b150(delta)`.
    pub fn hurry(&mut self, delta: u32) {
        if self.counter == 0 {
            return; // 0x79b156
        }
        self.counter = match self.counter.checked_sub(delta) {
            Some(left) if left > 0 => left,
            _ => 1, // 0x79b168: clamp so the next tick commits
        };
    }

    /// `0x79b180`.
    pub fn commit(&mut self) {
        self.current = self.pending;
        self.counter = 0;
        self.bound = 0;
        self.waiting = false;
        self.commits = self.commits.saturating_add(1);
    }

    /// Fade weight, `0..=1`. Reconstruction of the interpolation blocks.
    pub fn blend(&self) -> f32 {
        if self.bound == 0 || self.counter == 0 {
            return 0.;
        }
        1. - self.counter.min(self.bound) as f32 / self.bound as f32
    }
}

/// What the sky shader draws: `(current set, incoming set, weight)`. Without a
/// running swap both indices are the committed set and the weight is zero.
pub fn frame_sets(machine: &Machine) -> (i32, i32, f32) {
    let base = machine.current.0 as i32;
    if machine.running() && !machine.waiting {
        (base, machine.pending.0 as i32, machine.blend())
    } else {
        (base, base, 0.)
    }
}

/// What a view draws, given the frame's published sets.
///
/// `0x7cca50` (refraction) and the reflection target are drawn from the same
/// sky object with a mirrored camera, so they read exactly the snapshot's
/// values; the orthographic map passes never draw the sky at all. This helper
/// exists so a call site cannot quietly recompute the fade for one view, and it
/// never touches the machine.
///
/// `secondary` is `scenery_visibility::Pass::is_secondary()`; it is passed as a
/// value so this module stays free of the renderer's pass enum and can be
/// compiled and tested on its own (`rustc --test src/sky_transition.rs`).
pub fn view_sets(frame: (i32, i32, f32), secondary: bool) -> (i32, i32, f32) {
    let _ = secondary;
    frame
}

/// One step of the native selection driver `0x4203f4..0x420807`.
///
/// `phase` is the scalar at `config+0x70`, `day` the byte read at `0x4204db`,
/// `clock` what `0x4b6250` returns (a game-day length in ms, [`DAY_CLOCK_MS`] by
/// default). Returns the request that was
/// issued, if any, for diagnostics.
pub fn drive(machine: &mut Machine, phase: f32, day: bool, clock: i32) -> Option<Request> {
    // The driver's own guard: nothing is re-issued while a swap runs.
    if !phase.is_finite() || machine.running() || machine.waiting {
        return None;
    }
    let phase = phase.clamp(0., 1.);
    let request = if phase <= 0. {
        // 0x42042e / 0x420430: day triple, uncounted.
        Request::now(machine.day)
    } else if phase >= 1. {
        // 0x420486 / 0x420488: night triple, uncounted.
        Request::now(machine.night)
    } else {
        // 0x4204db takes this branch when the day/night byte is zero; the
        // non-zero branch reaches the same two counted swaps after an uncounted
        // pre-swap, so only the counted part changes the visible result.
        let counted = if day {
            // 0x420738: the late-day band is a no-op.
            if phase >= DAY_LATE_PHASE {
                return None;
            }
            (machine.day, slice_count(clock, phase))
        } else if phase >= NIGHT_ONSET_PHASE {
            (machine.night, slice_count_night(clock, phase))
        } else {
            // 0x4207aa: below the night onset nothing happens.
            return None;
        };
        if counted.1 == 0 {
            Request::now(counted.0)
        } else {
            Request::over(counted.0, counted.1)
        }
    };
    if machine.request(request).is_err() {
        return None;
    }
    // 0x79ad90 runs before the driver returns; the caller then blocks on
    // `+0x5b9`. The viewer's sets are already resident, so the wait resolves
    // here instead of in a 10 ms sleep loop.
    machine.resources_ready();
    // 0x420600 / 0x42071f: shorten by the slices the clock already covered.
    let covered = (clock / 2).max(0) as u32;
    machine.hurry(covered.saturating_sub(request.slices));
    Some(request)
}

/// `0x42054e..0x420570`: `ftol((clock / 2) * phase * 1.25)`, with the native
/// integer halving (`0x420559`..`0x42055c`) and truncating `_ftol`.
pub fn slice_count(clock: i32, phase: f32) -> u32 {
    scaled(clock, phase * DAY_COUNT_SCALE)
}

/// `0x420666..0x42068c`: `ftol((clock / 2) * (1 - phase) * 2)`.
pub fn slice_count_night(clock: i32, phase: f32) -> u32 {
    scaled(clock, (1. - phase) * 2.)
}

fn scaled(clock: i32, factor: f32) -> u32 {
    if !factor.is_finite() {
        return 0;
    }
    let value = (clock / 2) as f32 * factor;
    if !value.is_finite() || value <= 0. {
        return 0;
    }
    if value >= u32::MAX as f32 {
        return u32::MAX;
    }
    value as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uncounted_swap_commits_at_once_and_leaves_no_counter() {
        let mut m = Machine::new(Triple::DAY);
        assert_eq!(m.request(Request::now(Triple::NIGHT)), Ok(()));
        assert_eq!(m.current(), Triple::NIGHT);
        assert!(!m.running());
        assert!(!m.waiting());
        assert_eq!(frame_sets(&m), (2, 2, 0.));
        assert_eq!(m.commits(), 1);
        // The same-set guard (0x79ad7d) refuses a repeat and starts no fade.
        assert_eq!(
            m.request(Request::now(Triple::NIGHT)),
            Err(Refused::AlreadyCurrent)
        );
        assert!(!m.step());
        assert_eq!(m.commits(), 1);
        assert!(
            !m.counts_down(),
            "0x79afb4 sets direction 0 on a same-set hit"
        );
    }

    #[test]
    fn a_counted_swap_waits_for_the_loader_then_counts_down_in_33_units() {
        let mut m = Machine::new(Triple::DAY);
        // 200 is not a multiple of 33, so the count-down commit is reachable.
        assert_eq!(m.request(Request::over(Triple::NIGHT, 200)), Ok(()));
        assert!(m.running() && m.waiting() && m.counts_down());
        assert_eq!(m.counter(), 200);
        assert_eq!(m.bound(), 200);
        // Nothing moves while +0x5b9 is set (0x79a06d).
        for _ in 0..8 {
            assert!(!m.step());
        }
        assert_eq!(m.counter(), 200);
        assert_eq!(frame_sets(&m), (0, 0, 0.), "no fade until the load lands");
        m.resources_ready();
        assert_eq!(frame_sets(&m), (0, 2, 0.));
        assert!(!m.step());
        assert_eq!(m.counter(), 167);
        assert_eq!(m.blend(), 1. - 167. / 200.);
        for _ in 0..10 {
            if m.step() {
                break;
            }
        }
        assert_eq!(m.current(), Triple::NIGHT);
        assert!(!m.running());
        assert_eq!(frame_sets(&m), (2, 2, 0.));
    }

    #[test]
    fn the_count_down_rule_commits_only_when_the_step_goes_negative() {
        let mut m = Machine::new(Triple::DAY);
        m.request(Request::over(Triple(1), 40)).unwrap();
        m.resources_ready();
        assert!(!m.step());
        assert_eq!(m.counter(), 7, "40 - 33");
        assert!(m.step(), "7 - 33 is negative, so it commits");
        assert_eq!(m.current(), Triple(1));
        // The stall: a counter of exactly 33 lands on zero, and the zero counter
        // is then read as "no transition" forever.
        let mut stall = Machine::new(Triple::DAY);
        stall.request(Request::over(Triple(1), 33)).unwrap();
        stall.resources_ready();
        assert!(!stall.step());
        assert_eq!(stall.counter(), 0);
        for _ in 0..100 {
            assert!(!stall.step());
        }
        assert_eq!(stall.current(), Triple::DAY, "native stall, not a commit");
        // duration_ms can never produce that value.
        for ms in [0, 1, 32, 33, 66, 99, 660, 4000, u32::MAX] {
            let r = Request::duration_ms(Triple(1), ms);
            assert_ne!(r.slices, 0);
            assert_ne!(r.slices % SLICE_MS, 0, "ms {ms} -> {}", r.slices);
        }
    }

    #[test]
    fn the_count_up_path_commits_once_it_reaches_the_bound() {
        // Direction 0 only arises from the same-set branch, which does not touch
        // the counter; drive it directly to pin the arithmetic.
        let mut m = Machine::new(Triple::DAY);
        m.pending = Triple(1);
        m.count_down = false;
        m.bound = 100;
        m.counter = 1;
        assert!(!m.step());
        assert_eq!(m.counter(), 34);
        assert!(!m.step());
        assert_eq!(m.counter(), 67);
        assert!(m.step());
        assert_eq!(m.current(), Triple(1));
        assert_eq!(m.commits(), 1);
    }

    #[test]
    fn a_running_swap_is_never_retargeted() {
        let mut m = Machine::new(Triple::DAY);
        assert!(m.request(Request::over(Triple(1), 100)).is_ok());
        assert_eq!(
            m.request(Request::now(Triple::NIGHT)),
            Err(Refused::Busy),
            "0x79ac48 guards the load"
        );
        assert_eq!(m.pending(), Triple(1));
        m.resources_ready();
        assert_eq!(
            m.request(Request::now(Triple::NIGHT)),
            Err(Refused::Busy),
            "0x79ac3a guards the counter"
        );
        m.hurry(1000);
        assert!(m.step());
        assert_eq!(m.current(), Triple(1));
        assert!(m.request(Request::now(Triple::NIGHT)).is_ok());
        assert_eq!(m.current(), Triple::NIGHT);
    }

    #[test]
    fn hurry_shortens_an_in_flight_fade_and_clamps_to_one_slice() {
        let mut m = Machine::new(Triple::DAY);
        m.request(Request::over(Triple::NIGHT, 1000)).unwrap();
        m.resources_ready();
        m.step();
        assert_eq!(m.counter(), 967);
        m.hurry(900);
        assert_eq!(m.counter(), 67);
        assert!(!m.on_commit_boundary());
        m.hurry(40);
        assert_eq!(m.counter(), 27);
        assert!(m.on_commit_boundary());
        assert!(m.step());
        assert_eq!(m.current(), Triple::NIGHT);
        // Hurrying a settled machine is inert (0x79b156).
        let before = m.commits();
        m.hurry(500);
        assert_eq!(m.commits(), before);
        assert_eq!(m.counter(), 0);
        // An oversized hurry clamps to one slice (0x79b168).
        let mut c = Machine::new(Triple::DAY);
        c.request(Request::over(Triple(1), 500)).unwrap();
        c.resources_ready();
        c.hurry(100_000);
        assert_eq!(c.counter(), 1);
        assert!(c.step());
    }

    #[test]
    fn invalid_and_non_finite_inputs_are_rejected() {
        let mut m = Machine::new(Triple::DAY);
        assert_eq!(m.request(Request::now(Triple(9))), Err(Refused::InvalidSet));
        assert_eq!(m.current(), Triple::DAY);
        assert!(!m.running());
        assert_eq!(slice_count(1000, f32::NAN), 0);
        assert_eq!(slice_count(1000, -1.), 0);
        assert_eq!(slice_count_night(1000, f32::INFINITY), 0);
        assert_eq!(slice_count(i32::MIN, 1.), 0, "negative clock clamp");
        // A finite product above u32::MAX clamps instead of wrapping, and an
        // infinite one is refused outright.
        assert_eq!(scaled(1000, 1e9), u32::MAX);
        assert_eq!(scaled(1000, f32::MAX), 0);
        for t in Triple::ALL {
            assert!(t.valid());
            assert!(!t.folder().is_empty());
        }
        assert!(!Triple(3).valid());
    }

    #[test]
    fn slice_counts_follow_the_recovered_expressions() {
        // (clock/2) * phase * 1.25, integer halving, truncating conversion.
        assert_eq!(slice_count(1000, 1.), 625);
        assert_eq!(slice_count(1000, 0.5), 312);
        assert_eq!(slice_count(999, 1.), 623); // 999/2 = 499
        assert_eq!(slice_count(0, 1.), 0);
        assert_eq!(slice_count(-1000, 1.), 0);
        assert_eq!(slice_count(2_400_000, 0.5), 750_000);
        assert_eq!(slice_count(1_800_000, 1.), 1_125_000);
        // (clock/2) * (1 - phase) * 2, truncated.
        assert_eq!(slice_count_night(1000, 0.), 1000);
        assert_eq!(slice_count_night(1000, 0.75), 250);
        assert_eq!(slice_count_night(1001, 0.5), 500);
        assert_eq!(slice_count_night(1_800_000, 0.5), 900_000);
        for clock in [0, 1, 2, 3599, 86_400, 2_400_000, i32::MAX] {
            for phase in [0., 0.25, 0.5, 0.75, 1.] {
                let _ = slice_count(clock, phase);
                let _ = slice_count_night(clock, phase);
            }
        }
    }

    #[test]
    fn the_driver_follows_the_native_ladder() {
        // Phase 0 and 1 install a whole set, uncounted.
        let mut m = Machine::new(Triple::NIGHT);
        assert!(drive(&mut m, 0., true, 1000).is_some());
        assert_eq!(m.current(), Triple::DAY);
        assert!(!m.running());
        let mut m = Machine::new(Triple::DAY);
        assert!(drive(&mut m, 1., false, 1000).is_some());
        assert_eq!(m.current(), Triple::NIGHT);
        assert!(!m.running());
        // Mid phase, day byte set, below 0.8: counted swap of the day set.
        let mut m = Machine::new(Triple::NIGHT);
        let r = drive(&mut m, 0.5, true, 1000).unwrap();
        assert_eq!(r, Request::over(Triple::DAY, 312));
        assert_eq!(m.pending(), Triple::DAY);
        assert_eq!(m.bound(), 312);
        assert!(m.running() && !m.waiting(), "the viewer never sleeps");
        // 0.8..1 on the day branch is a no-op (0x420738).
        let mut m = Machine::new(Triple(1));
        assert!(drive(&mut m, 0.9, true, 1000).is_none());
        assert!(!m.running());
        assert_eq!(m.current(), Triple(1));
        // The night branch needs 0.5 or more (0x4207aa).
        let mut m = Machine::new(Triple(1));
        assert!(drive(&mut m, 0.4, false, 1000).is_none());
        assert!(!m.running());
        assert!(drive(&mut m, 0.75, false, 1000).is_some());
        assert_eq!(m.pending(), Triple::NIGHT);
        assert_eq!(m.bound(), 250, "1000/2 * (1 - 0.75) * 2");
        // A running swap makes the driver inert (0x420404).
        assert!(drive(&mut m, 0., false, 1000).is_none());
        assert_eq!(m.pending(), Triple::NIGHT);
        // Hostile phase values install nothing.
        let mut m = Machine::new(Triple(1));
        assert!(drive(&mut m, f32::NAN, true, 1000).is_none());
        assert!(!m.running());
        assert_eq!(m.current(), Triple(1));
    }

    #[test]
    fn the_driver_hurries_by_the_slices_the_clock_already_covered() {
        // `0x420600` is `clock/2 - count`, subtracted from the counter, so the
        // remaining counter is `2*count - clock/2`, clamped up to one slice.
        let mut m = Machine::new(Triple::NIGHT);
        drive(&mut m, 0.5, true, 1000);
        assert_eq!(m.bound(), 312);
        assert_eq!(m.counter(), 124, "624 - 500");
        // A clock far past the fade collapses to a single slice and commits on
        // the next tick.
        let mut m = Machine::new(Triple::NIGHT);
        drive(&mut m, 0.1, true, 10_000);
        assert_eq!(m.bound(), 625);
        assert_eq!(m.counter(), 1);
        assert!(m.step());
        assert_eq!(m.current(), Triple::DAY);
        // A short clock shortens the fade only a little.
        let mut m = Machine::new(Triple::NIGHT);
        drive(&mut m, 0.5, true, 40);
        assert_eq!(m.bound(), 12);
        assert_eq!(m.counter(), 4);
        let mut m = Machine::new(Triple::NIGHT);
        drive(&mut m, 0.5, true, 624);
        assert_eq!(m.counter(), 78);
        // A real game-day clock leaves hundreds of thousands of slices.
        let mut m = Machine::new(Triple::NIGHT);
        drive(&mut m, 0.5, true, 1_800_000);
        assert_eq!(m.bound(), 562_500);
        assert_eq!(m.counter(), 225_000);
        // The real clock: a 30-minute day, so the day-set count at phase 0.5 is
        // 900 000 * 0.5 * 1.25 = 562 500 ms of fade, and the driver's hurry takes
        // the counter down to 225 000 ms.
        let mut m = Machine::new(Triple::NIGHT);
        let r = drive(&mut m, 0.5, true, DAY_CLOCK_MS).unwrap();
        assert_eq!(r.slices, 562_500, "900000 * 0.5 * 1.25");
        assert_eq!(m.counter(), 225_000, "2*562500 - 900000");
        assert!(m.running());
    }

    #[test]
    fn the_clock_settings_are_the_two_the_client_ships() {
        // `0x4b6250` has exactly two answers, selected by `config+0x74`.
        assert_eq!(DAY_CLOCK_MS, DAY_MS_SHORT);
        assert_eq!(clock_for_minutes(30.), DAY_MS_SHORT);
        assert_eq!(clock_for_minutes(40.), DAY_MS_LONG);
        assert_eq!(clock_for_minutes(35.), DAY_MS_LONG, "snaps to the nearer");
        assert_eq!(clock_for_minutes(0.), DAY_MS_SHORT);
        // A day-set fade at full phase is 62.5 % of the day, a night-set fade at
        // phase 0 is a whole day: both are minutes, not milliseconds.
        assert_eq!(slice_count(DAY_MS_LONG, 1.) * 100 / DAY_MS_LONG as u32, 62);
        assert_eq!(
            slice_count_night(DAY_MS_LONG, 0.),
            DAY_MS_LONG as u32 / 2 * 2
        );
    }

    #[test]
    fn elapsed_time_advances_the_fade_in_thirtieths_of_a_second() {
        // `0x8f14bc` is `1/30`, so a second of wall time is 30 ticks, exactly:
        // the residual is milli-thirty-seconds, not a float, because a float
        // accumulator floors `1000 / (1000/30)` to 29 and loses a tick per
        // second.
        assert_eq!(TICKS_PER_SECOND, 30);
        assert!(
            (SLICE_SECONDS - 1. / 30.).abs() < 1e-7,
            "0x8f14bc must be the 1/30 s frame interval, got {SLICE_SECONDS}"
        );
        let mut m = Machine::new(Triple::DAY);
        m.request(Request::over(Triple::NIGHT, 600_000)).unwrap();
        m.resources_ready();
        let mut residual = 0u64;
        assert_eq!(m.step_ms(1000, &mut residual), 30);
        assert_eq!(residual, 0);
        assert_eq!(m.counter(), 600_000 - 30 * SLICE_MS);
        assert_eq!(m.step_ms(2000, &mut residual), 60);
        assert_eq!(residual, 0);
        // Sub-tick frames accumulate and cross the tick boundary on time.
        let mut residual = 0u64;
        let mut ticks = 0;
        for _ in 0..30 {
            ticks += m.step_ms(1, &mut residual);
        }
        assert_eq!(ticks, 0, "30 ms is less than one 33.3 ms tick");
        assert_eq!(residual, 900);
        for _ in 0..4 {
            ticks += m.step_ms(1, &mut residual);
        }
        assert_eq!(ticks, 1, "34 ms is one tick");
        assert_eq!(residual, 20);
        // A zero-length frame can never advance the state.
        assert_eq!(m.step_ms(0, &mut residual), 0);
        assert_eq!(residual, 20);
    }

    #[test]
    fn a_fade_duration_does_not_depend_on_the_frame_size() {
        // The native fade is minutes long, so replaying it once per rendered
        // frame would tie its duration to the frame rate. The same elapsed time
        // must commit in the same number of ticks whatever the frame size is.
        let run = |frame_ms: u32| {
            let mut m = Machine::new(Triple::DAY);
            m.request(Request::over(Triple::NIGHT, 60_000)).unwrap();
            m.resources_ready();
            let mut residual = 0u64;
            let mut elapsed = 0u32;
            while elapsed < 200_000 && m.running() {
                let taken = m.step_ms(frame_ms, &mut residual);
                assert!(u64::from(taken) <= MAX_TICKS_PER_STEP);
                elapsed = elapsed.saturating_add(frame_ms);
            }
            (elapsed, m.commits(), m.current())
        };
        let (fast_ms, fast_commits, fast_set) = run(7);
        let (slow_ms, slow_commits, slow_set) = run(250);
        assert_eq!((fast_commits, fast_set), (1, Triple::NIGHT));
        assert_eq!((slow_commits, slow_set), (1, Triple::NIGHT));
        // Both land on the same tick count; only the frame quantisation of the
        // *last* frame differs, which is one frame, not one fade.
        let fast_ticks = fast_ms as f32 * TICKS_PER_SECOND as f32 / 1000.;
        let slow_ticks = slow_ms as f32 * TICKS_PER_SECOND as f32 / 1000.;
        assert!(
            (fast_ticks - slow_ticks).abs() <= 1.0 + TICKS_PER_SECOND as f32 * 250. / 1000.,
            "fade lasted {fast_ms} ms at 7 ms frames and {slow_ms} ms at 250 ms frames"
        );
        // And the duration is the counted fade, not the frame size: 60 000 ms of
        // counter is 1 818 ticks, i.e. about 60.6 s.
        assert!(
            (59_000..62_000).contains(&fast_ms),
            "60 000 ms of fade took {fast_ms} ms"
        );
    }

    #[test]
    fn the_whole_model_day_sweeps_through_the_ladder_on_the_real_clock() {
        // One minute per model hour, as the renderer does it, with the real
        // game-day clock and the real tick. A fade legitimately spans several
        // minutes of model time, so the ladder is only obliged to act at a
        // minute it finds the machine idle; whatever it does must settle on the
        // set that hour names.
        // `night_factor`/`daylight` are transcribed here rather than imported so
        // this module still compiles standalone (`rustc --test src/sky_transition.rs`,
        // which is how CI runs it); `environment.rs` is the definition of record.
        fn night_factor(hour: f32) -> f32 {
            let t = hour.rem_euclid(24.) / 24.;
            if t < 0.125 {
                1.
            } else if t < f32::from_bits(0x3e955555) {
                1. - (t - 0.125) * 6.
            } else if (t as f64) < 0.75 {
                0.
            } else if t < 0.875 {
                (t - 0.75) * 8.
            } else {
                1.
            }
        }
        let clock = DAY_CLOCK_MS;
        let mut m = Machine::new(Triple::DAY);
        let mut residual = 0u64;
        let (mut swaps, mut repeated, mut busy) = (0u32, 0u32, 0u32);
        let mut longest = 0;
        for minute in 0..(24 * 60) {
            let hour = minute as f32 / 60.;
            let phase = night_factor(hour);
            assert!((0.0..=1.0).contains(&phase), "{hour}h: phase {phase}");
            let day = 1. - phase >= 0.5;
            // The ladder's target for this hour: `None` for a silent band.
            let target = if phase <= 0. {
                Some(Triple::DAY)
            } else if phase >= 1. {
                Some(Triple::NIGHT)
            } else if day {
                (phase < DAY_LATE_PHASE).then_some(Triple::DAY)
            } else {
                (phase >= NIGHT_ONSET_PHASE).then_some(Triple::NIGHT)
            };
            let issued = drive(&mut m, phase, day, clock);
            match target {
                Some(t) if m.running() || m.waiting() || issued.is_some() => {
                    // Either this minute issued the swap, or an earlier one is
                    // still in flight and the driver correctly left it alone.
                    if let Some(r) = issued {
                        assert_eq!(r.to, t, "{hour}h issued the wrong set");
                    }
                    let settled = if m.running() || m.waiting() {
                        m.pending()
                    } else {
                        m.current()
                    };
                    assert_eq!(settled, t, "{hour}h is not heading to its set");
                    if issued.is_some() {
                        swaps += 1;
                        longest = longest.max(m.bound());
                    } else {
                        busy += 1;
                    }
                }
                Some(t) => {
                    // Idle and nothing issued: only the same-set guard (0x79ad7d)
                    // can refuse that, so the machine must already stand on it.
                    assert_eq!(m.current(), t, "{hour}h refused while not on target");
                    repeated += 1;
                }
                None => {
                    assert!(issued.is_none(), "{hour}h issued inside a silent band");
                }
            }
            if m.running() && m.counts_down() {
                assert!(m.on_commit_boundary() || m.counter() >= SLICE_MS);
            }
            // A minute of wall time is far longer than one frame.
            m.step_ms(60_000, &mut residual);
        }
        assert!(swaps > 0, "the sweep must issue swaps");
        assert_eq!(
            swaps + busy + repeated,
            24 * 60,
            "every minute is accounted for"
        );
        // The counted bands' supremum is half a game day; the rest of the day is
        // the uncounted endpoint switch. With one sample per model minute the
        // longest observed fade sits just under that ceiling.
        let half_day = slice_count_night(clock, 0.5);
        assert_eq!(half_day, clock as u32 / 2);
        assert!(
            longest <= half_day && longest * 100 >= half_day * 98,
            "longest fade {longest} should approach half a day ({half_day});              {swaps} swaps, {busy} minutes spent mid-fade, {repeated} repeats"
        );
    }

    #[test]
    fn both_silent_bands_still_suppress_a_swap_when_the_byte_disagrees() {
        // The bands are in the ladder (`0x420738`, `0x4207aa`) even though the
        // viewer's byte makes them unreachable from the clock; this keeps them
        // from being silently deleted by a future refactor.
        let mut m = Machine::new(Triple::NIGHT);
        assert!(
            drive(&mut m, 0.9, true, DAY_CLOCK_MS).is_none(),
            "0.8..1 day"
        );
        assert!(
            drive(&mut m, 0.1, false, DAY_CLOCK_MS).is_none(),
            "< 0.5 night"
        );
        assert!(!m.running() && !m.waiting());
        assert_eq!(m.current(), Triple::NIGHT);
        // Just outside each band the same inputs do issue, so the guard is the
        // threshold itself and not something earlier in the ladder.
        assert!(drive(&mut m, 0.799, true, DAY_CLOCK_MS).is_some());
        // The night branch targets the night set, so the machine must not
        // already stand on it: otherwise `0x79ad7d` refuses and the refusal would
        // look like the band again.
        let mut m = Machine::new(Triple::DAY);
        assert!(drive(&mut m, 0.50_1, false, DAY_CLOCK_MS).is_some());
    }

    #[test]
    fn frame_sets_are_the_committed_set_without_a_running_swap() {
        let mut m = Machine::new(Triple(1));
        assert_eq!(frame_sets(&m), (1, 1, 0.));
        m.request(Request::over(Triple::NIGHT, 200)).unwrap();
        assert_eq!(frame_sets(&m), (1, 1, 0.), "loading is not a fade");
        m.resources_ready();
        assert_eq!(frame_sets(&m), (1, 2, 0.));
        for _ in 0..100 {
            if m.step() {
                break;
            }
        }
        assert_eq!(frame_sets(&m), (2, 2, 0.));
    }

    #[test]
    fn an_authored_day_night_pair_drives_the_swaps() {
        let mut m = Machine::new(Triple(1));
        assert!(m.author(Triple(1), Triple(0)));
        assert_eq!((m.day_set(), m.night_set()), (Triple(1), Triple(0)));
        drive(&mut m, 0., true, 1000);
        assert_eq!(m.current(), Triple(1), "phase 0 selects the day set");
        drive(&mut m, 1., false, 1000);
        assert_eq!(m.current(), Triple(0), "phase 1 selects the night set");
        // Both must be known sets, and a swap of the same set in both roles is
        // refused rather than producing a silent no-op every clock edge.
        assert!(!m.author(Triple(9), Triple(0)));
        assert!(!m.author(Triple(1), Triple(1)));
        assert!(!m.author(Triple(0), Triple(9)));
        // Re-authoring is refused while a swap is running.
        m.request(Request::over(Triple(1), 500)).unwrap();
        assert!(!m.author(Triple(0), Triple(1)));
    }

    #[test]
    fn a_secondary_view_reads_the_same_sets_and_never_steps() {
        let mut m = Machine::new(Triple::DAY);
        m.request(Request::over(Triple::NIGHT, 990)).unwrap();
        m.resources_ready();
        m.step();
        let frame = frame_sets(&m);
        assert_eq!(frame.0, 0);
        assert_eq!(frame.1, 2);
        assert!(frame.2 > 0.);
        let counter = m.counter();
        for secondary in [false, false, true, true, true, true] {
            assert_eq!(
                view_sets(frame, secondary),
                frame,
                "every view sees one snapshot"
            );
        }
        assert_eq!(m.counter(), counter, "reading a view must not tick");
        // And the machine's own view of the frame is what got published.
        assert_eq!(frame, frame_sets(&m));
        assert_eq!(counter, m.counter(), "no view advanced the machine");
    }
}
