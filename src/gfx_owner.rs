//! Scene/model effect owners that actually produce runtime scene lights.
//!
//! Recovered 1.4.2 behaviour reproduced here:
//! * `CECOrnamentMan::LoadEffect` 0x5b33f3..0x5b34b0 reads the 60-byte ECWLD
//!   effect record and copies position/direction/up, scale/speed, valid-time,
//!   alpha and attenuation into the GFX node; 0x5b3700..0x5b37b9 builds the
//!   normalized placement matrix through 0x788960, applies the authored scale
//!   and play speed and installs the parent.
//! * 0x5b18a1..0x5b18d4 rejects day-only and night-only effects at exactly 0.5.
//! * 0x5b1a00 onward ticks existing owners, checks their time eligibility and
//!   loads at most ONE new root per frame, returning immediately after a load.
//! * 0x70ba00 advances the light keypoint clock before the visibility/priority
//!   publication gate, so hidden or suppressed lights still progress.
//! * 0x70bd20 updates duplicate publications in place; 0x70b130 removes a
//!   pending publication when the owner is destroyed; 0x425bf8..0x425c2c
//!   clears publication membership after the dynamic cells are built.
//!
//! Explicitly NOT implemented: the rest of the GFX graph (particles, sprites,
//! trails, animated textures), hook/attachment resolution inside a skeleton and
//! the visual effect's alpha/attenuation fade. Those remain in
//! Findings/Unsupported-client-paths.md; they do not fake light producers.

#![allow(dead_code)]

use crate::{
    gfx_light::Emitter,
    scene_lights::{DynamicLight, LightPublications},
};

/// A root GFX asset attached to a world owner (ECWLD effect or ECM model event).
#[derive(Clone, Debug)]
pub struct Request {
    /// Caller-stable owner identity; removed/unregistered through this value.
    pub identity: u64,
    /// Normalized archive path of the root GFX file (`gfx/...`).
    pub path: String,
    /// Column-major placement with normalized basis and authored scale.
    pub transform: [f32; 16],
    /// Authored play speed; the owner clock is scaled by it once per tick.
    pub speed: f32,
    /// Manager priority handed to each light element's publication gate.
    pub priority: i32,
    /// Owner-local activation delay in milliseconds.
    pub start_ms: u32,
    /// Owner lifetime after `start_ms`; `None` runs until removal.
    pub span_ms: Option<u32>,
    /// Optional packed outer color; alpha is the intensity factor.
    pub outer: Option<[f32; 4]>,
    /// Region/time-of-day visibility. Gates publication, never progression.
    pub active: bool,
    /// Diagnostic origin, e.g. `world.ecwld (3,4) export 17`.
    pub origin: String,
}

impl Request {
    fn validate(&self) -> Result<(), String> {
        if self.path.is_empty() {
            return Err("GFX owner has an empty root path".into());
        }
        if self.transform.iter().any(|v| !v.is_finite()) {
            return Err(format!("{}: nonfinite owner placement", self.origin));
        }
        if !self.speed.is_finite() || self.speed < 0. {
            return Err(format!("{}: invalid owner play speed", self.origin));
        }
        if self.span_ms == Some(0) {
            return Err(format!("{}: zero owner lifetime", self.origin));
        }
        if let Some(outer) = self.outer {
            if outer.iter().any(|v| !v.is_finite() || *v < 0.) {
                return Err(format!("{}: invalid owner outer color", self.origin));
            }
        }
        Ok(())
    }
}

/// Column-major placement from a world placement basis, then the authored
/// scale. 0x788960 normalizes forward and up separately before their cross
/// product; `up` must not be orthogonalized silently.
pub fn placement_transform(
    position: [f32; 3],
    direction: [f32; 3],
    up: [f32; 3],
    scale: f32,
) -> Result<[f32; 16], String> {
    if position
        .iter()
        .chain(direction.iter())
        .chain(up.iter())
        .any(|v| !v.is_finite())
        || !scale.is_finite()
    {
        return Err("nonfinite effect placement".into());
    }
    let length = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let normalized = |v: [f32; 3]| -> Result<[f32; 3], String> {
        let l = length(v);
        if !(l > 1e-6) || !l.is_finite() {
            return Err("degenerate effect placement axis".into());
        }
        Ok([v[0] / l, v[1] / l, v[2] / l])
    };
    let forward = normalized(direction)?;
    let up = normalized(up)?;
    let right = [
        up[1] * forward[2] - up[2] * forward[1],
        up[2] * forward[0] - up[0] * forward[2],
        up[0] * forward[1] - up[1] * forward[0],
    ];
    let right = normalized(right).map_err(|_| "parallel effect placement axes".to_string())?;
    // Column-major: right, up, forward scaled by the authored scale, then the
    // translation column.
    Ok([
        right[0] * scale,
        right[1] * scale,
        right[2] * scale,
        0.,
        up[0] * scale,
        up[1] * scale,
        up[2] * scale,
        0.,
        forward[0] * scale,
        forward[1] * scale,
        forward[2] * scale,
        0.,
        position[0],
        position[1],
        position[2],
        1.,
    ])
}

/// Normalize an ECWLD/ECM GFX reference into an archive path. Mirrors the
/// normalized-PCK lookup used for models so `scene\\x.gfx` and `gfx/scene/x.gfx`
/// name the same root.
pub fn root_path(reference: &str) -> String {
    let path = reference.replace('\\', "/").to_lowercase();
    if path.starts_with("gfx/") {
        path
    } else {
        format!("gfx/{path}")
    }
}

enum State {
    Pending,
    Live(Vec<Emitter>),
    Failed,
}

struct Owner {
    serial: u64,
    request: Request,
    elapsed: u32,
    state: State,
}

/// Publication identity: owner serial in the high half, element index in the
/// low half. Two owners can never collide, and destruction removes exactly the
/// elements that owner published.
fn element_identity(serial: u64, index: usize) -> u64 {
    (serial << 32) | (index as u64 & 0xffff_ffff)
}

/// Counters for the viewer's status line and for tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub live: usize,
    pub pending: usize,
    pub failed_owners: usize,
    pub removed: usize,
    pub loads: usize,
    pub emitters: usize,
    /// Number of recorded failure strings, including rejected roots.
    pub failures: usize,
}

pub struct Manager {
    owners: Vec<Owner>,
    failures: Vec<String>,
    next_serial: u64,
    removed: usize,
    loads: usize,
    read: Option<Box<dyn FnMut(&str) -> Result<String, String>>>,
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    pub fn new() -> Self {
        Self {
            owners: Vec::new(),
            failures: Vec::new(),
            next_serial: 1,
            removed: 0,
            loads: 0,
            read: None,
        }
    }

    /// Text reader for GFX roots. The caller owns archive I/O and GBK decoding;
    /// this module never guesses an encoding or a container format.
    pub fn set_reader(&mut self, read: impl FnMut(&str) -> Result<String, String> + 'static) {
        self.read = Some(Box::new(read));
    }

    pub fn with_reader(read: impl FnMut(&str) -> Result<String, String> + 'static) -> Self {
        let mut manager = Self::new();
        manager.set_reader(read);
        manager
    }

    /// Register an owner. It stays pending until the lazy loader reaches it.
    pub fn insert(&mut self, request: Request) -> Result<(), String> {
        request.validate()?;
        if self
            .owners
            .iter()
            .any(|owner| owner.request.identity == request.identity)
        {
            return Err(format!(
                "duplicate GFX owner identity {}",
                request.identity
            ));
        }
        let serial = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1);
        self.owners.push(Owner {
            serial,
            request,
            elapsed: 0,
            state: State::Pending,
        });
        Ok(())
    }

    /// Region/time visibility. Returns false when the identity is not owned.
    pub fn set_active(&mut self, identity: u64, active: bool) -> bool {
        match self.owners.iter_mut().find(|o| o.request.identity == identity) {
            Some(owner) => {
                owner.request.active = active;
                true
            }
            None => false,
        }
    }

    pub fn active(&self, identity: u64) -> Option<bool> {
        self.owners
            .iter()
            .find(|o| o.request.identity == identity)
            .map(|o| o.request.active)
    }

    /// Destroy an owner before or after it published. Its pending publication
    /// membership is removed, so a light can never outlive its producer.
    pub fn remove(&mut self, identity: u64, publications: &mut LightPublications) -> bool {
        let Some(index) = self
            .owners
            .iter()
            .position(|o| o.request.identity == identity)
        else {
            return false;
        };
        self.unregister(&self.owners[index], publications);
        self.owners.remove(index);
        self.removed += 1;
        true
    }

    /// Region unload: drop every owner and every publication it owns.
    pub fn clear(&mut self, publications: &mut LightPublications) {
        for owner in &self.owners {
            self.unregister(owner, publications);
        }
        self.removed += self.owners.len();
        self.owners.clear();
    }

    fn unregister(&self, owner: &Owner, publications: &mut LightPublications) {
        let count = match &owner.state {
            State::Live(emitters) => emitters.len(),
            State::Pending | State::Failed => 0,
        };
        for index in 0..count {
            publications.unregister(element_identity(owner.serial, index));
        }
    }

    /// Advance the owner lifecycle and publish this frame's lights.
    ///
    /// Order follows 0x5b1a00: existing owners tick and publish first, then at
    /// most one pending root is loaded and the update returns immediately.
    pub fn update(&mut self, delta_ms: u32, publications: &mut LightPublications) {
        let mut finished = Vec::new();
        let mut failed = Vec::new();
        for owner in &mut self.owners {
            if !matches!(owner.state, State::Live(_)) {
                continue;
            }
            match tick_owner(owner, delta_ms, publications) {
                Ok(true) => {}
                Ok(false) => finished.push(owner.request.identity),
                Err(e) => failed.push((owner.request.identity, e)),
            }
        }
        for identity in finished {
            if let Some(index) = self
                .owners
                .iter()
                .position(|o| o.request.identity == identity)
            {
                self.unregister(&self.owners[index], publications);
                self.owners.remove(index);
                self.removed += 1;
            }
        }
        for (identity, error) in failed {
            if let Some(index) = self
                .owners
                .iter()
                .position(|o| o.request.identity == identity)
            {
                self.unregister(&self.owners[index], publications);
                let origin = self.owners[index].request.origin.clone();
                self.owners[index].state = State::Failed;
                self.failures.push(format!("{origin}: {error}"));
            }
        }
        self.load_one();
    }

    /// Lazily load exactly one pending root; 0x5b1a00 returns after a load.
    fn load_one(&mut self) {
        let Some(index) = self
            .owners
            .iter()
            .position(|o| matches!(o.state, State::Pending))
        else {
            return;
        };
        let path = self.owners[index].request.path.clone();
        let origin = self.owners[index].request.origin.clone();
        let result = match &mut self.read {
            Some(read) => read(&path).map_err(|e| format!("{path}: {e}")),
            None => Err(format!("{path}: no GFX root reader attached")),
        };
        self.loads += 1;
        match result.and_then(|text| Self::decode(&text)) {
            Ok(emitters) => self.owners[index].state = State::Live(emitters),
            Err(e) => {
                self.owners[index].state = State::Failed;
                self.failures.push(format!("{origin}: {e}"));
            }
        }
    }

    fn decode(text: &str) -> Result<Vec<Emitter>, String> {
        if !text.starts_with("MOXT") {
            return Err("binary GFX root is not implemented for light publication".into());
        }
        let definitions = crate::gfx_light::parse(text)?;
        Ok(definitions.into_iter().map(Emitter::new).collect())
    }

    pub fn status(&self) -> Status {
        let mut status = Status {
            removed: self.removed,
            loads: self.loads,
            failures: self.failures.len(),
            ..Default::default()
        };
        for owner in &self.owners {
            match &owner.state {
                State::Pending => status.pending += 1,
                State::Failed => status.failed_owners += 1,
                State::Live(emitters) => {
                    status.live += 1;
                    status.emitters += emitters.len();
                }
            }
        }
        status
    }

    pub fn failures(&self) -> &[String] {
        &self.failures
    }

    /// Owner origins in insertion order, for diagnostics.
    pub fn origins(&self) -> Vec<&str> {
        self.owners.iter().map(|o| o.request.origin.as_str()).collect()
    }
}

/// Advance one live owner by an already owner-scaled delta.
/// Returns `Ok(false)` when the owner's lifetime ended.
fn tick_owner(
    owner: &mut Owner,
    delta_ms: u32,
    publications: &mut LightPublications,
) -> Result<bool, String> {
    let scaled = delta_ms as f32 * owner.request.speed;
    if !scaled.is_finite() || scaled < 0. || scaled >= u32::MAX as f32 {
        return Err("owner clock overflow".into());
    }
    owner.elapsed = owner.elapsed.saturating_add(scaled as u32);
    let start = owner.request.start_ms;
    if let Some(span) = owner.request.span_ms {
        if owner.elapsed >= start.saturating_add(span) {
            return Ok(false);
        }
    }
    let visible = owner.request.active && owner.elapsed >= start;
    let priority = owner.request.priority;
    let transform = owner.request.transform;
    let outer = owner.request.outer;
    let serial = owner.serial;
    let State::Live(emitters) = &mut owner.state else {
        return Ok(true);
    };
    for (index, emitter) in emitters.iter_mut().enumerate() {
        let light: Option<DynamicLight> =
            emitter.tick(scaled as u32, visible, priority, transform, outer)?;
        if let Some(light) = light {
            publications.publish(element_identity(serial, index), light);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_lights::StaticGrid;

    fn root(path: &str, text: &str) -> impl FnMut(&str) -> Result<String, String> + 'static {
        let path = path.to_string();
        let text = text.to_string();
        move |requested: &str| {
            if requested == path {
                Ok(text.clone())
            } else {
                Err("missing GFX root".into())
            }
        }
    }

    fn light_file() -> String {
        "MOXTVersion: 37\nGFXELEMENTCOUNT: 1\nGFXELEMENTID: 130\nRepeatCount: 1\nRepeatDelay: 0\nBindEle: \nDummyEle: \nIsDummy: 0\nMatchGrnd: 0\nLightType: 1\nPriority: 4\nRange: 10\nStartTime: 0\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\nTimeSpan: -1\nPosition: 1, 2, 3\nColor: -1\nScale: 1\nDirection: 0, 0, 0, 1\nRad_2D: 0\nCtrlMethodCount: 0\n".into()
    }

    fn request(identity: u64, active: bool) -> Request {
        Request {
            identity,
            path: "gfx/scene/lamp.gfx".into(),
            transform: placement_transform([0.; 3], [0., 0., 1.], [0., 1., 0.], 1.).unwrap(),
            speed: 1.,
            priority: 4,
            start_ms: 0,
            span_ms: None,
            outer: None,
            active,
            origin: format!("test owner {identity}"),
        }
    }

    fn grid() -> StaticGrid {
        let mut grid = StaticGrid::default();
        grid.update(&crate::scene_lights::SceneLights::default(), [0.; 3]);
        grid
    }

    #[test]
    fn placement_transform_normalizes_basis_and_applies_authored_scale() {
        let m = placement_transform([10., 2., -3.], [0., 0., 2.], [0., 2., 2.], 3.).unwrap();
        assert_eq!(&m[12..16], &[10., 2., -3., 1.]);
        assert!((m[0] - 3.).abs() < 1e-6);
        let up_length = (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt();
        assert!((up_length - 3.).abs() < 1e-6);
        assert_eq!(&m[8..12], &[0., 0., 3., 0.]);
        assert!(placement_transform([0.; 3], [0., 0., 1.], [0., 0., 1.], 1.).is_err());
        assert!(placement_transform([0.; 3], [0., 0., 1.], [0., 1., 0.], f32::NAN).is_err());
        assert_eq!(root_path("scene\\lamp.gfx"), "gfx/scene/lamp.gfx");
        assert_eq!(root_path("GFX/Scene/Lamp.GFX"), "gfx/scene/lamp.gfx");
    }

    #[test]
    fn owner_publishes_only_between_start_and_death_then_unregisters() {
        let mut manager = Manager::with_reader(root("gfx/scene/lamp.gfx", &light_file()));
        let mut r = request(1, true);
        r.start_ms = 100;
        r.span_ms = Some(500);
        manager.insert(r).unwrap();
        let mut publications = LightPublications::default();
        let mut grid = grid();
        // Frame 1 loads the single pending root and returns without ticking.
        manager.update(50, &mut publications);
        assert_eq!(manager.status().live, 1);
        assert_eq!(manager.status().loads, 1);
        publications.consume_into(&mut grid);
        assert!(grid.query([1., 2., 3.], 1.).is_none());
        // Before StartTime the clock advances but nothing is published.
        manager.update(99, &mut publications);
        publications.consume_into(&mut grid);
        assert!(grid.query([1., 2., 3.], 1.).is_none());
        manager.update(1, &mut publications);
        assert!(publications.len() == 1);
        publications.consume_into(&mut grid);
        assert!(grid.query([1., 2., 3.], 1.).is_some());
        // Death removes the pending publication in the same frame.
        manager.update(500, &mut publications);
        publications.consume_into(&mut grid);
        assert_eq!(manager.status().live, 0);
        assert_eq!(manager.status().removed, 1);
        assert!(grid.query([1., 2., 3.], 1.).is_none());
        manager.update(500, &mut publications);
        assert!(publications.len() == 0);
    }

    #[test]
    fn hidden_owner_keeps_progressing_and_priority_only_gates_publication() {
        let mut manager = Manager::with_reader(root("gfx/scene/lamp.gfx", &light_file()));
        manager.insert(request(1, false)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(0, &mut publications); // lazy load
        manager.update(7000, &mut publications);
        assert_eq!(publications.len(), 0, "hidden owners publish nothing");
        manager.set_active(1, true);
        manager.update(0, &mut publications);
        assert_eq!(publications.len(), 1, "infinite key is still live at 7s");
        publications.consume_into(&mut grid());
        // A lower manager priority suppresses the same live key.
        let mut r = request(2, true);
        r.priority = 3;
        manager.insert(r).unwrap();
        manager.update(0, &mut publications);
        manager.update(0, &mut publications);
        assert_eq!(publications.len(), 1);
        publications.consume_into(&mut grid());
        assert_eq!(manager.status().failures, 0);
    }

    #[test]
    fn one_root_is_loaded_per_update_and_insertion_order_is_preserved() {
        let text = light_file();
        let mut manager = Manager::with_reader(move |_| Ok(text.clone()));
        manager.insert(request(1, true)).unwrap();
        manager.insert(request(2, true)).unwrap();
        manager.insert(request(3, true)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(0, &mut publications);
        assert_eq!(manager.status().live, 1);
        assert_eq!(manager.status().pending, 2);
        manager.update(0, &mut publications);
        assert_eq!(manager.status().live, 2);
        manager.update(0, &mut publications);
        assert_eq!(manager.status().live, 3);
        // The load frames themselves publish nothing for the newly loaded root;
        // already-live owners keep publishing on every update.
        assert_eq!(publications.len(), 2);
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 3);
        assert_eq!(
            manager.origins(),
            ["test owner 1", "test owner 2", "test owner 3"]
        );
    }

    #[test]
    fn duplicate_identity_and_invalid_requests_are_rejected() {
        let mut manager = Manager::new();
        manager.insert(request(1, true)).unwrap();
        assert!(manager.insert(request(1, true)).is_err());
        let mut bad = request(2, true);
        bad.path.clear();
        assert!(manager.insert(bad).is_err());
        let mut bad = request(3, true);
        bad.transform[0] = f32::INFINITY;
        assert!(manager.insert(bad).is_err());
        let mut bad = request(4, true);
        bad.speed = -1.;
        assert!(manager.insert(bad).is_err());
        let mut bad = request(5, true);
        bad.span_ms = Some(0);
        assert!(manager.insert(bad).is_err());
        let mut bad = request(6, true);
        bad.outer = Some([0., 0., 0., -1.]);
        assert!(manager.insert(bad).is_err());
        assert_eq!(manager.status().live + manager.status().pending, 1);
    }

    #[test]
    fn missing_root_fails_explicitly_without_blocking_later_owners() {
        let text = light_file();
        let mut manager = Manager::with_reader(move |path| {
            if path == "gfx/scene/lamp.gfx" {
                Ok(text.clone())
            } else {
                Err("missing archive entry".into())
            }
        });
        let mut first = request(1, true);
        first.path = "gfx/scene/broken.gfx".into();
        manager.insert(first).unwrap();
        manager.insert(request(2, true)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(16, &mut publications);
        assert_eq!(manager.status().failed_owners, 1);
        assert!(manager.failures()[0].contains("gfx/scene/broken.gfx"));
        manager.update(16, &mut publications);
        assert_eq!(manager.status().live, 1);
        assert_eq!(publications.len(), 0, "a load frame does not tick");
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 1);
    }

    #[test]
    fn binary_roots_are_refused_rather_than_parsed_as_lights() {
        let mut manager = Manager::with_reader(|_| Ok("MOXB".to_string()));
        manager.insert(request(1, true)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(16, &mut publications);
        assert_eq!(manager.status().failed_owners, 1);
        assert!(manager.failures()[0].contains("binary GFX"));
        // No reader attached is also explicit, not a silent no-op.
        let mut manager = Manager::new();
        manager.insert(request(9, true)).unwrap();
        manager.update(16, &mut publications);
        assert!(manager.failures()[0].contains("no GFX root reader"));
    }

    #[test]
    fn removal_and_region_unload_cannot_leave_stale_publications() {
        let text = light_file();
        let mut manager = Manager::with_reader(move |_| Ok(text.clone()));
        manager.insert(request(1, true)).unwrap();
        manager.insert(request(2, true)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(0, &mut publications);
        manager.update(0, &mut publications);
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 2);
        // Destroy one producer before consumption.
        assert!(manager.remove(1, &mut publications));
        assert_eq!(publications.len(), 1);
        assert!(!manager.remove(1, &mut publications));
        publications.consume_into(&mut grid());
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 1, "the live owner republishes");
        // Region unload drops everything, and the same identity may be reused.
        manager.clear(&mut publications);
        assert_eq!(publications.len(), 0);
        assert_eq!(manager.status().live, 0);
        manager.insert(request(2, true)).unwrap();
        manager.insert(request(1, true)).unwrap();
        manager.update(0, &mut publications);
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 1, "one root loads per update");
        manager.update(16, &mut publications);
        assert_eq!(publications.len(), 2);
    }

    #[test]
    fn publication_identities_are_unique_per_owner_and_element() {
        let two_lights = light_file().replace(
            "GFXELEMENTCOUNT: 1",
            "GFXELEMENTCOUNT: 2",
        ).replace(
            "CtrlMethodCount: 0\n",
            "CtrlMethodCount: 0\nGFXELEMENTID: 130\nRepeatCount: 1\nRepeatDelay: 0\nBindEle: \nDummyEle: \nIsDummy: 0\nMatchGrnd: 0\nLightType: 1\nPriority: 4\nRange: 10\nStartTime: 0\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\nTimeSpan: -1\nPosition: 0, 4, 0\nColor: -1\nScale: 1\nDirection: 0, 0, 0, 1\nRad_2D: 0\nCtrlMethodCount: 0\n",
        );
        let mut manager = Manager::with_reader(move |_| Ok(two_lights.clone()));
        manager.insert(request(1, true)).unwrap();
        let mut publications = LightPublications::default();
        manager.update(0, &mut publications);
        manager.update(0, &mut publications);
        assert_eq!(publications.len(), 2);
        manager.remove(1, &mut publications);
        assert_eq!(publications.len(), 0, "removal clears every element");
    }

    #[test]
    fn owner_speed_scales_the_clock_and_overflow_is_explicit() {
        let mut manager = Manager::with_reader(root("gfx/scene/lamp.gfx", &light_file()));
        let mut r = request(1, true);
        r.speed = 2.;
        r.start_ms = 1000;
        manager.insert(r).unwrap();
        let mut publications = LightPublications::default();
        manager.update(0, &mut publications);
        manager.update(500, &mut publications);
        assert_eq!(publications.len(), 1, "500ms * 2 reaches StartTime 1000");
        let mut r = request(2, true);
        r.speed = f32::MAX;
        manager.insert(r).unwrap();
        manager.update(0, &mut publications);
        manager.update(1000, &mut publications);
        assert!(manager
            .failures()
            .iter()
            .any(|f| f.contains("owner clock overflow")));
    }
}
