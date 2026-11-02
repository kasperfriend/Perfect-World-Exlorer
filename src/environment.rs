//! Version-12 world EC-BSD box-area environment records, as loaded by
//! elementclient 0x41da70 / 0x48a510. No auto-family scene constants here.
use crate::scene::Scene;
#[path = "client_region_map.rs"]
mod client_region_map;
use client_region_map::ActiveBoxMap;
use std::{collections::BTreeMap, fs, path::Path};

/// Client world day weight (1 = day, 0 = night).
/// `4b60e0` produces the complementary night factor at object +0x70;
/// `41ec86`, `42b4fe`, and `42bd6e` pass it to fog, BMD and terrain.
/// Hour is the world-clock time, normalized by 24 before the client curve.
pub fn daylight(hour: f32) -> f32 {
    let t = hour.rem_euclid(24.) / 24.;
    let night = if t < 0.125 {
        1.
    } else if t < f32::from_bits(0x3e955555) {
        1. - (t - 0.125) * 6.
    } else if (t as f64) < 0.75 {
        0.
    } else if t < 0.875 {
        (t - 0.75) * 8.
    } else {
        1.
    };
    1. - night.clamp(0., 1.)
}

#[derive(Clone, Copy, Debug)]
pub struct Fog {
    pub start: f32,
    pub end: f32,
    pub density: f32,
    pub color: [f32; 3],
}
impl Fog {
    fn blend(a: Self, b: Self, t: f32) -> Self {
        let mix = |x: f32, y: f32| x + (y - x) * t;
        Self {
            start: mix(a.start, b.start),
            end: mix(a.end, b.end),
            density: mix(a.density, b.density),
            color: std::array::from_fn(|i| mix(a.color[i], b.color[i])),
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct FogEndpoints {
    air_day: Fog,
    air_night: Fog,
    water_day: Fog,
    water_night: Fog,
    ambient_day: [f32; 3],
    ambient_night: [f32; 3],
    diffuse_day: [f32; 3],
    diffuse_night: [f32; 3],
    sun_direction: [f32; 3],
}
impl FogEndpoints {
    fn blend(a: Self, b: Self, t: f32) -> Self {
        Self {
            air_day: Fog::blend(a.air_day, b.air_day, t),
            air_night: Fog::blend(a.air_night, b.air_night, t),
            water_day: Fog::blend(a.water_day, b.water_day, t),
            water_night: Fog::blend(a.water_night, b.water_night, t),
            ambient_day: mix3(a.ambient_day, b.ambient_day, t),
            ambient_night: mix3(a.ambient_night, b.ambient_night, t),
            diffuse_day: mix3(a.diffuse_day, b.diffuse_day, t),
            diffuse_night: mix3(a.diffuse_night, b.diffuse_night, t),
            sun_direction: b.sun_direction,
        }
    }
    fn at_hour(self, hour: f32) -> (Fog, Fog) {
        let day = daylight(hour);
        (
            Fog::blend(self.air_night, self.air_day, day),
            Fog::blend(self.water_night, self.water_day, day),
        )
    }
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
#[derive(Clone, Copy, Debug)]
pub struct RegionalLight {
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub toward_sun: [f32; 3],
}
/// Client-like linear transitions between authored region endpoints. Each new
/// box starts from the current in-flight values, with duration in EC-BSD.
#[derive(Clone, Debug)]
pub struct FogTransition {
    key: Option<usize>,
    from: FogEndpoints,
    target: FogEndpoints,
    elapsed: f32,
    duration: f32,
}
impl FogTransition {
    pub fn new(env: &Environments, p: [f32; 3]) -> Self {
        let (key, target, duration) = env.endpoints(p);
        Self {
            key,
            from: target,
            target,
            elapsed: duration,
            duration,
        }
    }
    fn current(&self) -> FogEndpoints {
        let t = if self.duration <= 0. {
            1.
        } else {
            (self.elapsed / self.duration).clamp(0., 1.)
        };
        FogEndpoints::blend(self.from, self.target, t)
    }
    pub fn lighting(&self, hour: f32) -> Option<RegionalLight> {
        self.key?;
        let e = self.current();
        let d = daylight(hour);
        Some(RegionalLight {
            ambient: mix3(e.ambient_night, e.ambient_day, d),
            diffuse: mix3(e.diffuse_night, e.diffuse_day, d),
            toward_sun: e.sun_direction.map(|x| -x),
        })
    }
    pub fn update(&mut self, env: &Environments, p: [f32; 3], hour: f32, dt: f32) -> (Fog, Fog) {
        self.update_endpoints(env.endpoints(p), hour, dt)
    }
    pub fn update_streamed(
        &mut self,
        env: &Environments,
        active: &mut RegionStreaming,
        p: [f32; 3],
        hour: f32,
        dt: f32,
    ) -> (Fog, Fog) {
        self.update_endpoints(active.endpoints(env, p), hour, dt)
    }
    pub fn new_streamed(env: &Environments, active: &mut RegionStreaming, p: [f32; 3]) -> Self {
        let (key, target, duration) = active.endpoints(env, p);
        Self {
            key,
            from: target,
            target,
            elapsed: duration,
            duration,
        }
    }

    fn update_endpoints(
        &mut self,
        (key, target, duration): (Option<usize>, FogEndpoints, f32),
        hour: f32,
        dt: f32,
    ) -> (Fog, Fog) {
        // The original region update returns without selecting a fallback if
        // the camera is outside every active box (0x41ec65). Do not snap to a
        // synthetic region when flying across a gap.
        if key.is_none() && self.key.is_some() {
            return self.current().at_hour(hour);
        }
        if key != self.key {
            self.from = self.current();
            self.target = target;
            self.key = key;
            self.elapsed = 0.;
            self.duration = duration;
        }
        self.elapsed = (self.elapsed + dt.max(0.)).min(self.duration.max(0.));
        self.current().at_hour(hour)
    }
}
impl Default for Fog {
    fn default() -> Self {
        Self {
            start: 10.,
            end: 100.,
            density: 0.5,
            color: [24. / 255., 100. / 255., 124. / 255.],
        }
    }
}
#[derive(Clone, Debug)]
struct BoxArea {
    global: bool,
    transition_ms: u32,
    center: [f32; 3],
    extents: [f32; 3],
    axes: [[f32; 3]; 3],
    day: Fog,
    night: Fog,
    air_day: Fog,
    air_night: Fog,
    ambient_day: [f32; 3],
    ambient_night: [f32; 3],
    diffuse_day: [f32; 3],
    diffuse_night: [f32; 3],
    sun_direction: [f32; 3],
}
#[derive(Clone, Default)]
pub struct Environments {
    areas: Vec<BoxArea>,
    id_to_index: BTreeMap<u32, usize>,
    streaming: Option<RegionLayout>,
}
/// Block references retained for runtime regional environment selection.
#[derive(Clone)]
struct RegionLayout {
    width: f32,
    length: f32,
    block_size: f32,
    rows: usize,
    cols: usize,
    boxes: Vec<Vec<u32>>,
}
pub struct RegionStreaming {
    layout: RegionLayout,
    map: ActiveBoxMap,
    active: Vec<bool>,
    rect: Option<(usize, usize, usize, usize)>,
}
impl RegionStreaming {
    fn new(layout: RegionLayout) -> Self {
        let active = vec![false; layout.boxes.len()];
        Self {
            layout,
            active,
            map: ActiveBoxMap::default(),
            rect: None,
        }
    }
    fn endpoints(&mut self, env: &Environments, p: [f32; 3]) -> (Option<usize>, FogEndpoints, f32) {
        self.update(p);
        let index = self
            .map
            .select(|id| {
                env.id_to_index.get(&id).and_then(|&i| {
                    let area = &env.areas[i];
                    area.contains(p).then_some(area.global)
                })
            })
            .and_then(|id| env.id_to_index.get(&id).copied());
        env.endpoints_for(index)
    }
    fn update(&mut self, p: [f32; 3]) {
        let l = &self.layout;
        if !p.iter().all(|x| x.is_finite()) || l.block_size <= 0. {
            return;
        }
        // 0x41c480 uses ceil(500 / block size), inclusive center and
        // exclusive upper bounds. At edges it shifts the full rectangle in.
        let radius = (500. / l.block_size).ceil() as isize;
        let span = 2 * radius + 1;
        let col = ((p[0] + l.width * 0.5) / l.block_size) as isize;
        let row = -(((p[2] - l.length * 0.5) / l.block_size) as isize);
        fn bounds(center: isize, span: isize, limit: usize) -> (usize, usize) {
            let width = span.min(limit as isize);
            let start = (center - span / 2).clamp(0, limit as isize - width);
            (start as usize, (start + width) as usize)
        }
        let (x0, x1) = bounds(col, span, l.cols);
        let (y0, y1) = bounds(row, span, l.rows);
        let rect = (x0, x1, y0, y1);
        if self.rect == Some(rect) {
            return;
        }
        self.rect = Some(rect);
        // Retain overlapping blocks and release departing references. The
        // asynchronous client's exact completion order remains unknown.
        for idx in 0..self.active.len() {
            let row = idx / l.cols;
            let col = idx % l.cols;
            if self.active[idx] && !(row >= y0 && row < y1 && col >= x0 && col < x1) {
                for &id in &l.boxes[idx] {
                    self.map.unload(id);
                }
                self.active[idx] = false;
            }
        }
        for row in y0..y1 {
            for col in x0..x1 {
                let idx = row * l.cols + col;
                if !self.active[idx] {
                    for &id in &l.boxes[idx] {
                        self.map.load(id);
                    }
                    self.active[idx] = true;
                }
            }
        }
    }
}
fn word(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .ok_or("truncated box area")?
            .try_into()
            .unwrap(),
    ))
}
fn float(b: &[u8], p: usize) -> Result<f32, String> {
    let f = f32::from_bits(word(b, p)?);
    if !f.is_finite() {
        return Err("invalid box-area float".into());
    }
    Ok(f)
}
fn vec3(b: &[u8], p: usize) -> Result<[f32; 3], String> {
    Ok([float(b, p)?, float(b, p + 4)?, float(b, p + 8)?])
}
fn rgb(b: &[u8], p: usize) -> Result<[f32; 3], String> {
    let c = word(b, p)?;
    Ok([
        ((c >> 16) & 255) as f32 / 255.,
        ((c >> 8) & 255) as f32 / 255.,
        (c & 255) as f32 / 255.,
    ])
}
fn scaled_rgb(b: &[u8], p: usize, scale: usize) -> Result<[f32; 3], String> {
    let k = float(b, scale)?;
    Ok(rgb(b, p)?.map(|x| x * k))
}
fn fog(b: &[u8], p: usize) -> Result<Fog, String> {
    let c = word(b, p + 12)?;
    Ok(Fog {
        start: float(b, p)?,
        end: float(b, p + 4)?,
        density: float(b, p + 8)?,
        color: [
            ((c >> 16) & 255) as f32 / 255.,
            ((c >> 8) & 255) as f32 / 255.,
            (c & 255) as f32 / 255.,
        ],
    })
}
impl BoxArea {
    fn parse(b: &[u8], p: usize) -> Result<Self, String> {
        let bytes = b
            .get(p..p.checked_add(0xe0).ok_or("box offset overflow")?)
            .ok_or("truncated EC-BSD box area")?;
        let global = word(bytes, 0)? != 0;
        let area = Self {
            global,
            transition_ms: word(bytes, 0x9c)?,
            center: vec3(bytes, 8)?,
            extents: if global {
                [1000.; 3]
            } else {
                vec3(bytes, 0x14)?
            },
            axes: [vec3(bytes, 0x20)?, vec3(bytes, 0x2c)?, vec3(bytes, 0x38)?],
            day: fog(bytes, 0x54)?,
            night: fog(bytes, 0xd0)?,
            air_day: fog(bytes, 0x44)?,
            air_night: fog(bytes, 0x64)?,
            ambient_day: rgb(bytes, 0x80)?,
            ambient_night: rgb(bytes, 0x88)?,
            diffuse_day: scaled_rgb(bytes, 0x84, 0xb8)?,
            diffuse_night: scaled_rgb(bytes, 0x8c, 0xbc)?,
            sun_direction: vec3(bytes, 0x74)?,
        };
        if area.extents.iter().any(|v| *v < 0.) {
            return Err("invalid box-area fog or bounds".into());
        }
        Ok(area)
    }
    fn contains(&self, p: [f32; 3]) -> bool {
        let d = [
            p[0] - self.center[0],
            p[1] - self.center[1],
            p[2] - self.center[2],
        ];
        (0..3).all(|i| {
            let a = self.axes[i];
            (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]).abs() <= self.extents[i]
        })
    }
    fn distance2(&self, p: [f32; 3]) -> f32 {
        (0..3).map(|i| (p[i] - self.center[i]).powi(2)).sum()
    }
}
impl Environments {
    pub fn load(scene: &Scene, dir: &Path) -> Result<Self, String> {
        let b = fs::read(dir.join("world.ecbsd")).map_err(|e| e.to_string())?;
        let mut refs = BTreeMap::new();
        for block in &scene.blocks {
            for r in &block.boxes {
                refs.insert(r.export_id, r.offset as usize);
            }
        }
        let mut areas = Vec::with_capacity(refs.len());
        let mut id_to_index = BTreeMap::new();
        for (id, p) in refs {
            id_to_index.insert(id, areas.len());
            areas.push(BoxArea::parse(&b, p).map_err(|e| format!("environment box {id}: {e}"))?);
        }
        let mut boxes = vec![Vec::new(); scene.rows * scene.cols];
        for block in &scene.blocks {
            let i = block.row as usize * scene.cols + block.col as usize;
            if i >= boxes.len() {
                return Err("invalid region block coordinates".into());
            }
            boxes[i] = block.boxes.iter().map(|r| r.export_id).collect();
        }
        let streaming = Some(RegionLayout {
            width: scene.width,
            length: scene.length,
            block_size: scene.block_size,
            rows: scene.rows,
            cols: scene.cols,
            boxes,
        });
        eprintln!("Loaded {} client environment boxes", areas.len());
        Ok(Self {
            areas,
            id_to_index,
            streaming,
        })
    }
    pub fn streaming(&self) -> Option<RegionStreaming> {
        self.streaming.clone().map(RegionStreaming::new)
    }
    pub fn underwater(&self, p: [f32; 3], hour: f32) -> Fog {
        self.sample(p, hour, true)
    }
    pub fn atmosphere(&self, p: [f32; 3], hour: f32) -> Fog {
        self.sample(p, hour, false)
    }
    fn endpoints(&self, p: [f32; 3]) -> (Option<usize>, FogEndpoints, f32) {
        // Selection order is still a heuristic pending the streaming-order trace.
        let a = self
            .areas
            .iter()
            .enumerate()
            .filter(|(_, a)| a.contains(p))
            .min_by(|(_, a), (_, b)| {
                a.global
                    .cmp(&b.global)
                    .then_with(|| a.distance2(p).total_cmp(&b.distance2(p)))
            });
        self.endpoints_for(a.map(|(i, _)| i))
    }
    fn endpoints_for(&self, index: Option<usize>) -> (Option<usize>, FogEndpoints, f32) {
        if let Some(i) = index {
            let a = &self.areas[i];
            return (
                Some(i),
                FogEndpoints {
                    air_day: a.air_day,
                    air_night: a.air_night,
                    water_day: a.day,
                    water_night: a.night,
                    ambient_day: a.ambient_day,
                    ambient_night: a.ambient_night,
                    diffuse_day: a.diffuse_day,
                    diffuse_night: a.diffuse_night,
                    sun_direction: a.sun_direction,
                },
                a.transition_ms as f32 / 1000.,
            );
        }
        let air = Fog {
            start: 60.,
            end: 400.,
            density: 1.,
            color: [70. / 255., 77. / 255., 91. / 255.],
        };
        let water = Fog::default();
        (
            None,
            FogEndpoints {
                air_day: air,
                air_night: air,
                water_day: water,
                water_night: water,
                ambient_day: [0.; 3],
                ambient_night: [0.; 3],
                diffuse_day: [0.; 3],
                diffuse_night: [0.; 3],
                sun_direction: [0., -1., 0.],
            },
            0.,
        )
    }
    fn sample(&self, p: [f32; 3], hour: f32, water: bool) -> Fog {
        let (air, water_fog) = self.endpoints(p).1.at_hour(hour);
        if water {
            water_fog
        } else {
            air
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_v12_box_fog_and_priority() {
        let mut b = vec![0u8; 224];
        fn put(b: &mut [u8], p: usize, v: f32) {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes())
        }
        for i in 0..3 {
            put(&mut b, 0x14 + 4 * i, 50.);
            put(&mut b, 0x20 + 16 * i, 1.);
        }
        for (p, c) in [
            (0x54, 0xff18647cu32),
            (0xd0, 0xff242448),
            (0x44, 0xff123456),
            (0x64, 0xff654321),
        ] {
            put(&mut b, p, 10.);
            put(&mut b, p + 4, 100.);
            put(&mut b, p + 8, 0.5);
            b[p + 12..p + 16].copy_from_slice(&c.to_le_bytes());
        }
        for (p, c) in [
            (0x80, 0xff59738eu32),
            (0x84, 0xff4b586bu32),
            (0x88, 0xff6c858eu32),
            (0x8c, 0xff383d52u32),
        ] {
            b[p..p + 4].copy_from_slice(&c.to_le_bytes());
        }
        put(&mut b, 0xb8, 1.);
        put(&mut b, 0xbc, 0.5);
        put(&mut b, 0x74, 0.1524305);
        put(&mut b, 0x78, -0.619779);
        put(&mut b, 0x7c, 0.76983047);
        let a = BoxArea::parse(&b, 0).unwrap();
        assert!((a.ambient_day[0] - 89. / 255.).abs() < 1e-6);
        assert!((a.diffuse_night[1] - 0.5 * 61. / 255.).abs() < 1e-6);
        assert!((a.sun_direction[1] + 0.619779).abs() < 1e-6);
        assert!(a.contains([49., 0., 0.]));
        assert!(!a.contains([51., 0., 0.]));
        let e = Environments {
            areas: vec![a],
            ..Default::default()
        };
        let d = e.underwater([0.; 3], 12.);
        let n = e.underwater([0.; 3], 0.);
        assert_eq!(d.start, 10.);
        assert!((d.color[1] - 100. / 255.).abs() < 1e-6);
        assert!((n.color[1] - 36. / 255.).abs() < 1e-6);
        let air = e.atmosphere([0.; 3], 12.);
        assert!((air.color[0] - 18. / 255.).abs() < 1e-6);
        let mut global = e.areas[0].clone();
        global.global = true;
        global.day.start = 80.;
        let mut local = e.areas[0].clone();
        local.day.start = 7.;
        let overlap = Environments {
            areas: vec![global, local],
            ..Default::default()
        };
        assert_eq!(overlap.underwater([0.; 3], 12.).start, 7.);
        // A new region's duration governs a linear transition, including a
        // second crossing before the first transition completes.
        let mut second = overlap.areas[1].clone();
        second.center = [200., 0., 0.];
        second.day.start = 27.;
        second.night.start = 47.;
        second.ambient_day = [1., 0.5, 0.25];
        second.diffuse_day = [0.4, 0.3, 0.2];
        second.transition_ms = 5000;
        let mut env = Environments {
            areas: vec![overlap.areas[1].clone(), second],
            ..Default::default()
        };
        let mut transition = FogTransition::new(&env, [0.; 3]);
        let light = transition.lighting(5.).unwrap();
        assert!((light.ambient[0] - (89. + 108.) / 510.).abs() < 1e-6);
        assert!((light.toward_sun[1] - 0.619779).abs() < 1e-6);
        let (_, w) = transition.update(&env, [200., 0., 0.], 12., 2.5);
        assert!((w.start - 17.).abs() < 1e-5);
        let mid = transition.lighting(12.).unwrap();
        assert!((mid.ambient[0] - (89. / 255. + 1.) * 0.5).abs() < 1e-6);
        assert!((mid.diffuse[0] - (75. / 255. + 0.4) * 0.5).abs() < 1e-6);
        let (_, outside) = transition.update(&env, [500., 0., 0.], 12., 10.);
        assert!((outside.start - 17.).abs() < 1e-5);
        let (_, night) = transition.update(&env, [200., 0., 0.], 0., 0.);
        assert!((night.start - 28.5).abs() < 1e-5);
        env.areas[0].transition_ms = 5000;
        let (_, reversing) = transition.update(&env, [0., 0., 0.], 12., 2.5);
        assert!((reversing.start - 12.).abs() < 1e-5);
        let (_, done) = transition.update(&env, [0., 0., 0.], 12., 2.5);
        assert!((done.start - 7.).abs() < 1e-5);
        transition.update(&env, [200., 0., 0.], 12., 5.);
        env.areas[0].transition_ms = 0;
        let (_, instant) = transition.update(&env, [0.; 3], 12., 0.);
        assert!((instant.start - 7.).abs() < 1e-5);
    }
    #[test]
    fn streamed_region_selection_tracks_blocks_references_and_transitions() {
        let fog = Fog::default();
        let area = BoxArea {
            global: false,
            transition_ms: 5000,
            center: [0.; 3],
            extents: [10000.; 3],
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            day: fog,
            night: fog,
            air_day: fog,
            air_night: fog,
            ambient_day: [1.; 3],
            ambient_night: [0.; 3],
            diffuse_day: [1.; 3],
            diffuse_night: [0.; 3],
            sun_direction: [0., -1., 0.],
        };
        let mut global = area.clone();
        global.global = true;
        global.day.start = 90.;
        let mut local = area.clone();
        local.day.start = 10.;
        let mut second = area;
        second.day.start = 30.;
        let mut ids = BTreeMap::new();
        ids.insert(1, 0);
        ids.insert(54, 1);
        ids.insert(2, 2);
        let layout = RegionLayout {
            width: 4000.,
            length: 500.,
            block_size: 500.,
            rows: 1,
            cols: 8,
            boxes: vec![
                vec![1],
                vec![1, 54],
                vec![1, 54],
                vec![1, 54, 2],
                vec![1, 2],
                vec![1],
                vec![1],
                vec![1],
            ],
        };
        let env = Environments {
            areas: vec![global, local, second],
            id_to_index: ids,
            streaming: Some(layout),
        };
        let mut active = env.streaming().unwrap();
        let p = [-1700., 0., 0.];
        let mut transition = FogTransition::new_streamed(&env, &mut active, p);
        assert_eq!(
            transition
                .update_streamed(&env, &mut active, p, 12., 0.)
                .1
                .start,
            10.
        );
        // Retained block refs prevent unloading an area when one block leaves.
        let mid = [-100., 0., 0.];
        let water = transition
            .update_streamed(&env, &mut active, mid, 12., 2.5)
            .1;
        assert!((water.start - 20.).abs() < 1e-5);
        let water = transition
            .update_streamed(&env, &mut active, mid, 12., 2.5)
            .1;
        assert_eq!(water.start, 30.);
        let end = [1900., 0., 0.];
        let water = transition
            .update_streamed(&env, &mut active, end, 12., 5.)
            .1;
        assert_eq!(water.start, 90.);
    }
    #[test]
    fn client_clock_fog_and_baked_lighting_weight() {
        for (hour, want) in [
            (-3., 0.),
            (0., 0.),
            (3., 0.),
            (4., 0.25),
            (5., 0.5),
            (6., 0.75),
            (7., 1.),
            (12., 1.),
            (18., 1.),
            (19., 2. / 3.),
            (20., 1. / 3.),
            (21., 0.),
            (24., 0.),
            (28., 0.25),
        ] {
            assert!(
                (daylight(hour) - want).abs() < 0.00001,
                "hour {hour}: {} != {want}",
                daylight(hour)
            );
        }
        for h in [3., 7., 18., 21.] {
            assert!((daylight(h - 0.0001) - daylight(h + 0.0001)).abs() < 0.0001);
        }
    }
    #[test]
    fn truncated_rejected() {
        assert!(BoxArea::parse(&[0; 30], 0).is_err())
    }
}
