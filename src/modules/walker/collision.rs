//! Native building and SpeedTree collision for Walker.
//!
//! `CELBrushBuilding` stores `CCDBrush` convex hulls after BMD geometry.
//! `CELTree::InitCDObjects` stores the same brushes in version-nine TBH files,
//! after a serialized convex-hull payload. The sweep below follows
//! `CHBasedCD::ClipBrush`: expand each plane by the player's AABB, track the
//! latest entering and earliest leaving fractions, and retain the entering
//! plane for sliding. Terrain is deliberately not represented here; Walker's
//! permanent terrain/support pass remains independent of this object option.

use crate::{bmd::CollisionBrush, pck, scene};
use glam::Vec3;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
};

const CELL_SIZE: f32 = 60.0;
const DIST_EPSILON: f32 = 0.03125;
const CHDATA_SKIP_COLLISION: u32 = 0x00000004;
const MAX_CELLS_PER_QUERY: usize = 4096;
const MAX_SLIDE_ITERATIONS: usize = 4;

type Cell = (i32, i32);

fn cell(value: f32) -> i32 {
    (value / CELL_SIZE).floor() as i32
}

fn cell_count(min: Vec3, max: Vec3) -> Option<(i32, i32, i32, i32, usize)> {
    let x0 = cell(min.x);
    let x1 = cell(max.x);
    let z0 = cell(min.z);
    let z1 = cell(max.z);
    let width = i64::from(x1) - i64::from(x0) + 1;
    let depth = i64::from(z1) - i64::from(z0) + 1;
    if width <= 0 || depth <= 0 {
        return None;
    }
    let count = width.checked_mul(depth)? as usize;
    Some((x0, x1, z0, z1, count))
}

fn bounds(center: Vec3, extents: Vec3) -> (Vec3, Vec3) {
    (center - extents, center + extents)
}

fn overlaps(a_center: Vec3, a_extents: Vec3, b_center: Vec3, b_extents: Vec3) -> bool {
    (a_center.x - b_center.x).abs() <= a_extents.x + b_extents.x
        && (a_center.y - b_center.y).abs() <= a_extents.y + b_extents.y
        && (a_center.z - b_center.z).abs() <= a_extents.z + b_extents.z
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    pub fraction: f32,
    pub normal: Vec3,
    pub start_solid: bool,
    pub all_solid: bool,
}

/// Exact plane sweep used by native `CCDBrush::Trace` for a non-point AABB.
pub fn trace_brush(
    brush: &CollisionBrush,
    start: Vec3,
    delta: Vec3,
    extents: Vec3,
) -> Option<Trace> {
    let end = start + delta;
    let mut enter = -1.0f32;
    let mut leave = 1.0f32;
    let mut clip = None;
    let mut start_out = false;
    let mut get_out = false;

    for side in &brush.sides {
        let normal = Vec3::from_array(side.normal);
        let offset = Vec3::new(
            if normal.x < 0.0 {
                extents.x
            } else {
                -extents.x
            },
            if normal.y < 0.0 {
                extents.y
            } else {
                -extents.y
            },
            if normal.z < 0.0 {
                extents.z
            } else {
                -extents.z
            },
        );
        let distance = side.distance - offset.dot(normal);
        let d1 = start.dot(normal) - distance;
        let d2 = end.dot(normal) - distance;
        get_out |= d2 > 0.0;
        start_out |= d1 > 0.0;

        if d1 > 0.0 && d2 >= d1 {
            return None;
        }
        if d1 <= 0.0 && d2 <= 0.0 {
            continue;
        }
        if d1 > d2 {
            let fraction = ((d1 - DIST_EPSILON) / (d1 - d2)).max(0.0);
            if fraction > enter {
                enter = fraction;
                clip = Some(normal);
            }
        } else {
            let fraction = ((d1 + DIST_EPSILON) / (d1 - d2)).min(1.0);
            leave = leave.min(fraction);
        }
    }

    if !start_out {
        return Some(Trace {
            fraction: 0.0,
            normal: Vec3::ZERO,
            start_solid: true,
            all_solid: !get_out,
        });
    }
    if enter < leave && enter > -1.0 && enter < 1.0 {
        return Some(Trace {
            fraction: enter.max(0.0),
            normal: clip.unwrap_or(Vec3::ZERO),
            start_solid: false,
            all_solid: false,
        });
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedMove {
    pub feet: Vec3,
    pub blocked: bool,
    pub normal: Vec3,
}

#[derive(Debug)]
struct TreeInstance {
    tree_type: usize,
    position: Vec3,
    center: Vec3,
    extents: Vec3,
}

#[derive(Debug, Default)]
struct CollisionWorld {
    buildings: Vec<CollisionBrush>,
    building_grid: HashMap<Cell, Vec<usize>>,
    large_buildings: Vec<usize>,
    tree_types: Vec<Option<Vec<CollisionBrush>>>,
    trees: Vec<TreeInstance>,
    tree_grid: HashMap<Cell, Vec<usize>>,
    large_trees: Vec<usize>,
}

impl CollisionWorld {
    fn add_buildings(&mut self, brushes: Vec<CollisionBrush>) {
        for brush in brushes {
            let index = self.buildings.len();
            let center = Vec3::from_array(brush.center);
            let extents = Vec3::from_array(brush.extents);
            let (min, max) = bounds(center, extents);
            self.buildings.push(brush);
            if let Some((x0, x1, z0, z1, count)) = cell_count(min, max) {
                if count <= MAX_CELLS_PER_QUERY {
                    for x in x0..=x1 {
                        for z in z0..=z1 {
                            self.building_grid.entry((x, z)).or_default().push(index);
                        }
                    }
                } else {
                    self.large_buildings.push(index);
                }
            }
        }
    }

    fn add_tree_type(&mut self, tree_type: usize, brushes: Vec<CollisionBrush>) {
        if self.tree_types.len() <= tree_type {
            self.tree_types.resize_with(tree_type + 1, || None);
        }
        self.tree_types[tree_type] = Some(brushes);
    }

    fn add_tree(&mut self, tree_type: usize, position: Vec3) {
        let Some(brushes) = self
            .tree_types
            .get(tree_type)
            .and_then(Option::as_ref)
            .filter(|brushes| !brushes.is_empty())
        else {
            return;
        };
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for brush in brushes {
            let center = Vec3::from_array(brush.center);
            let extents = Vec3::from_array(brush.extents);
            min = min.min(center - extents);
            max = max.max(center + extents);
        }
        let center = (min + max) * 0.5 + position;
        let extents = (max - min) * 0.5;
        let index = self.trees.len();
        let (world_min, world_max) = bounds(center, extents);
        self.trees.push(TreeInstance {
            tree_type,
            position,
            center,
            extents,
        });
        if let Some((x0, x1, z0, z1, count)) = cell_count(world_min, world_max) {
            if count <= MAX_CELLS_PER_QUERY {
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        self.tree_grid.entry((x, z)).or_default().push(index);
                    }
                }
            } else {
                self.large_trees.push(index);
            }
        }
    }

    fn trace(&self, start: Vec3, delta: Vec3, extents: Vec3) -> Option<Trace> {
        let end = start + delta;
        let min = start.min(end) - extents;
        let max = start.max(end) + extents;
        let query_center = (min + max) * 0.5;
        let query_extents = (max - min) * 0.5;
        let mut building_ids = self.large_buildings.clone();
        let mut tree_ids = self.large_trees.clone();
        let mut cover_all = true;
        if let Some((x0, x1, z0, z1, count)) = cell_count(min, max) {
            if count <= MAX_CELLS_PER_QUERY {
                cover_all = false;
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        if let Some(ids) = self.building_grid.get(&(x, z)) {
                            building_ids.extend_from_slice(ids);
                        }
                        if let Some(ids) = self.tree_grid.get(&(x, z)) {
                            tree_ids.extend_from_slice(ids);
                        }
                    }
                }
            }
        }
        if cover_all {
            building_ids.extend(0..self.buildings.len());
            tree_ids.extend(0..self.trees.len());
        }
        building_ids.sort_unstable();
        building_ids.dedup();
        tree_ids.sort_unstable();
        tree_ids.dedup();

        let mut best: Option<Trace> = None;
        let mut retain = |candidate: Trace| {
            if best.is_none_or(|old| candidate.fraction < old.fraction) {
                best = Some(candidate);
            }
        };
        for index in building_ids {
            let brush = &self.buildings[index];
            if brush.flags & CHDATA_SKIP_COLLISION != 0 {
                continue;
            }
            let center = Vec3::from_array(brush.center);
            let brush_extents = Vec3::from_array(brush.extents);
            if !overlaps(query_center, query_extents, center, brush_extents) {
                continue;
            }
            if let Some(hit) = trace_brush(brush, start, delta, extents) {
                retain(hit);
            }
        }
        for index in tree_ids {
            let tree = &self.trees[index];
            if !overlaps(query_center, query_extents, tree.center, tree.extents) {
                continue;
            }
            let Some(brushes) = self.tree_types[tree.tree_type].as_ref() else {
                continue;
            };
            // Native CELTree subtracts the instance position before tracing
            // its shared brushes. Tree hull flags are not filtered there.
            let local_start = start - tree.position;
            for brush in brushes {
                if let Some(hit) = trace_brush(brush, local_start, delta, extents) {
                    retain(hit);
                }
            }
        }
        best
    }

    fn resolve(&self, old_feet: Vec3, desired_feet: Vec3, extents: Vec3) -> ResolvedMove {
        let mut center = old_feet + Vec3::Y * extents.y;
        let mut remaining = desired_feet - old_feet;
        let mut blocked = false;
        let mut last_normal = Vec3::ZERO;
        for _ in 0..MAX_SLIDE_ITERATIONS {
            if remaining.length_squared() < 1e-12 {
                break;
            }
            let Some(hit) = self.trace(center, remaining, extents) else {
                center += remaining;
                break;
            };
            blocked = true;
            if hit.start_solid {
                // Match the native air-move escape behavior: a sweep that
                // begins inside a hull must not permanently trap a player who
                // spawned or loaded inside scenery. Subsequent sweeps from
                // outside are stopped normally.
                center += remaining;
                break;
            }
            center += remaining * hit.fraction;
            last_normal = hit.normal;
            remaining *= 1.0 - hit.fraction;
            let into_plane = remaining.dot(hit.normal);
            if into_plane < 0.0 {
                remaining -= hit.normal * into_plane;
            }
        }
        ResolvedMove {
            feet: center - Vec3::Y * extents.y,
            blocked,
            normal: last_normal,
        }
    }
}

#[derive(Default, Debug)]
struct TreeReport {
    loaded_types: usize,
    missing: usize,
    failed: usize,
    errors: Vec<String>,
}

fn read_u32(data: &[u8], p: &mut usize) -> Result<u32, String> {
    let end = p.checked_add(4).ok_or("TBH offset overflow")?;
    let value = u32::from_le_bytes(
        data.get(*p..end)
            .ok_or("truncated TBH")?
            .try_into()
            .unwrap(),
    );
    *p = end;
    Ok(value)
}

fn skip_hull_payload(data: &[u8], p: &mut usize) -> Result<(), String> {
    const V02: &[u8] = b"V0.2======Convex_Hull_Data_Start=========";
    const V01: &[u8] = b"V0.1======Convex_Hull_Data_Start=========";
    const V00: &[u8] = b"==========Convex_Hull_Data_Start=========";
    let end = p.checked_add(V02.len()).ok_or("TBH header overflow")?;
    let header = data.get(*p..end).ok_or("truncated TBH hull header")?;
    *p = end;
    if header == V02 || header == V01 {
        let _flags = read_u32(data, p)?;
    } else if header != V00 {
        return Err("unknown TBH convex-hull header".into());
    }
    let size = read_u32(data, p)? as usize;
    if size > 64 * 1024 * 1024 {
        return Err(format!("TBH hull payload is too large: {size}"));
    }
    *p = p.checked_add(size).ok_or("TBH payload overflow")?;
    if *p > data.len() {
        return Err("truncated TBH hull payload".into());
    }
    Ok(())
}

fn parse_tbh(data: &[u8]) -> Result<Vec<CollisionBrush>, String> {
    let mut p = 0;
    let version = read_u32(data, &mut p)?;
    if version != 9 {
        return Err(format!(
            "unsupported TBH version {version}; native requires 9"
        ));
    }
    let count = read_u32(data, &mut p)? as usize;
    if count > 4096 {
        return Err(format!("TBH collision-object limit exceeded: {count}"));
    }
    let mut brushes = Vec::with_capacity(count);
    for _ in 0..count {
        skip_hull_payload(data, &mut p)?;
        brushes.push(CollisionBrush::read(data, &mut p)?);
    }
    if p != data.len() {
        return Err(format!("{} trailing TBH bytes", data.len() - p));
    }
    Ok(brushes)
}

fn tree_brushes(world: &mut CollisionWorld, element: &PathBuf, scene: &scene::Scene) -> TreeReport {
    let mut report = TreeReport::default();
    let result = (|| -> Result<(File, HashMap<String, pck::Entry>), String> {
        let mut file = File::open(element.join("trees.pck")).map_err(|e| e.to_string())?;
        let entries = pck::directory(&mut file)?
            .into_iter()
            .map(|entry| (entry.name.to_lowercase(), entry))
            .collect();
        Ok((file, entries))
    })();
    let (mut file, entries) = match result {
        Ok(value) => value,
        Err(error) => {
            report.failed = scene.tree_types.len();
            report.errors.push(format!("trees.pck: {error}"));
            return report;
        }
    };
    for (index, tree) in scene.tree_types.iter().enumerate() {
        let key = tree.tree.to_lowercase().replace('\\', "/");
        let Some(stem) = key.strip_suffix(".spt") else {
            report.failed += 1;
            report
                .errors
                .push(format!("{}: tree path is not SPT", tree.tree));
            continue;
        };
        let path = format!("{stem}.tbh");
        let Some(entry) = entries.get(&path) else {
            // CELTree::InitCDObjects treats a missing TBH as non-fatal.
            report.missing += 1;
            continue;
        };
        let result = pck::read_entry(&mut file, entry).and_then(|data| parse_tbh(&data));
        match result {
            Ok(brushes) => {
                world.add_tree_type(index, brushes);
                report.loaded_types += 1;
            }
            Err(error) => {
                report.failed += 1;
                if report.errors.len() < 4 {
                    report.errors.push(format!("{path}: {error}"));
                }
            }
        }
    }
    report
}

enum BuildingMessage {
    Batch {
        processed: usize,
        brushes: Vec<CollisionBrush>,
    },
    Done {
        failed: usize,
    },
    Fatal(String),
}

fn building_worker(element: PathBuf, paths: Vec<String>, tx: SyncSender<BuildingMessage>) {
    let result = (|| -> Result<(), String> {
        let mut file = File::open(element.join("litmodels.pck")).map_err(|e| e.to_string())?;
        let entries: HashMap<_, _> = pck::directory(&mut file)?
            .into_iter()
            .map(|entry| (entry.name.to_lowercase(), entry))
            .collect();
        let mut failed = 0;
        for start in (0..paths.len()).step_by(32) {
            let end = (start + 32).min(paths.len());
            let mut brushes = Vec::new();
            for path in &paths[start..end] {
                let key = format!("litmodels/world/{path}");
                let result = entries
                    .get(&key)
                    .ok_or_else(|| format!("missing model {key}"))
                    .and_then(|entry| pck::read_entry(&mut file, entry))
                    .and_then(|data| {
                        crate::bmd::Model::parse(&data).map_err(|error| format!("{key}: {error}"))
                    });
                match result {
                    Ok(model) => brushes.extend(model.collision),
                    Err(error) => {
                        failed += 1;
                        eprintln!("Walker object collision: {error}");
                    }
                }
            }
            if tx
                .send(BuildingMessage::Batch {
                    processed: end,
                    brushes,
                })
                .is_err()
            {
                return Ok(());
            }
        }
        let _ = tx.send(BuildingMessage::Done { failed });
        Ok(())
    })();
    if let Err(error) = result {
        let _ = tx.send(BuildingMessage::Fatal(error));
    }
}

/// Walker-owned object collision and its independent, lightweight BMD loader.
/// It does not depend on the whole-world texture/geometry preload setting.
pub struct Collision {
    element: Option<PathBuf>,
    building_paths: Vec<String>,
    receiver: Option<Receiver<BuildingMessage>>,
    world: CollisionWorld,
    tree_report: TreeReport,
    building_processed: usize,
    building_failed: usize,
    building_done: bool,
    fatal_error: Option<String>,
}

impl Default for Collision {
    fn default() -> Self {
        Self {
            element: None,
            building_paths: Vec::new(),
            receiver: None,
            world: CollisionWorld::default(),
            tree_report: TreeReport::default(),
            building_processed: 0,
            building_failed: 0,
            building_done: false,
            fatal_error: None,
        }
    }
}

impl Collision {
    pub fn new(element: PathBuf, scene: &scene::Scene, ornaments: &[scene::Ornament]) -> Self {
        let mut world = CollisionWorld::default();
        let tree_report = tree_brushes(&mut world, &element, scene);
        for block in &scene.blocks {
            for tree in &block.trees {
                world.add_tree(tree.tree_type as usize, Vec3::from_array(tree.position));
            }
        }
        let mut seen = HashSet::new();
        let building_paths = ornaments
            .iter()
            .map(|ornament| ornament.path.to_lowercase().replace('\\', "/"))
            .filter(|path| seen.insert(path.clone()))
            .collect();
        Self {
            element: Some(element),
            building_paths,
            world,
            tree_report,
            ..Self::default()
        }
    }

    pub fn ensure_loading(&mut self) {
        if self.receiver.is_some()
            || self.building_done
            || self.building_paths.is_empty()
            || self.fatal_error.is_some()
        {
            return;
        }
        let Some(element) = self.element.clone() else {
            return;
        };
        let (tx, rx) = mpsc::sync_channel(2);
        let paths = self.building_paths.clone();
        if std::thread::Builder::new()
            .name("walker-object-collision".into())
            .spawn(move || building_worker(element, paths, tx))
            .is_ok()
        {
            self.receiver = Some(rx);
        } else {
            self.fatal_error = Some("could not start object-collision loader".into());
        }
    }

    pub fn poll(&mut self) {
        let mut finished = false;
        loop {
            let message = match self.receiver.as_ref() {
                Some(receiver) => receiver.try_recv(),
                None => break,
            };
            match message {
                Ok(BuildingMessage::Batch { processed, brushes }) => {
                    self.building_processed = processed;
                    self.world.add_buildings(brushes);
                }
                Ok(BuildingMessage::Done { failed }) => {
                    self.building_failed = failed;
                    self.building_done = true;
                    finished = true;
                    break;
                }
                Ok(BuildingMessage::Fatal(error)) => {
                    self.fatal_error = Some(error);
                    self.building_done = true;
                    finished = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.building_done = true;
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.receiver = None;
        }
    }

    pub fn resolve(&self, old_feet: Vec3, desired_feet: Vec3, extents: Vec3) -> ResolvedMove {
        self.world.resolve(old_feet, desired_feet, extents)
    }

    pub fn status(&self) -> String {
        let loading = if self.building_paths.is_empty() {
            "no BMD placements".to_owned()
        } else if self.building_done {
            format!("{} BMD assets", self.building_processed)
        } else if self.receiver.is_some() {
            format!(
                "loading BMD {}/{}",
                self.building_processed,
                self.building_paths.len()
            )
        } else {
            "BMD loading starts with Walker".to_owned()
        };
        format!(
            "{} building hulls • {} tree instances ({}/{} tree hull types) • {} • terrain always on",
            self.world.buildings.len(),
            self.world.trees.len(),
            self.tree_report.loaded_types,
            self.tree_report.loaded_types + self.tree_report.missing + self.tree_report.failed,
            loading
        )
    }

    pub fn diagnostic(&self) -> Option<String> {
        if let Some(error) = &self.fatal_error {
            return Some(format!("Object collision loader: {error}"));
        }
        let mut details = Vec::new();
        if self.building_failed > 0 {
            details.push(format!("{} BMD assets failed", self.building_failed));
        }
        if self.tree_report.failed > 0 {
            details.push(format!(
                "{} tree hull types failed",
                self.tree_report.failed
            ));
        }
        if self.tree_report.missing > 0 {
            details.push(format!(
                "{} tree types have no TBH (native non-fatal path)",
                self.tree_report.missing
            ));
        }
        details.extend(self.tree_report.errors.iter().take(2).cloned());
        if details.is_empty() {
            None
        } else {
            Some(details.join(" • "))
        }
    }

    #[cfg(test)]
    fn from_parts(buildings: Vec<CollisionBrush>) -> Self {
        let mut world = CollisionWorld::default();
        world.add_buildings(buildings);
        Self {
            world,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bmd::BrushSide;

    fn cube() -> CollisionBrush {
        let sides = [
            ([1.0, 0.0, 0.0], 1.0),
            ([-1.0, 0.0, 0.0], 1.0),
            ([0.0, 1.0, 0.0], 1.0),
            ([0.0, -1.0, 0.0], 1.0),
            ([0.0, 0.0, 1.0], 1.0),
            ([0.0, 0.0, -1.0], 1.0),
        ]
        .into_iter()
        .map(|(normal, distance)| BrushSide {
            normal,
            distance,
            bevel: false,
        })
        .collect();
        CollisionBrush {
            center: [0.0; 3],
            extents: [1.0; 3],
            flags: 0,
            sides,
        }
    }

    #[test]
    fn native_sweep_expands_planes_by_player_aabb() {
        let hit = trace_brush(
            &cube(),
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
            Vec3::splat(0.3),
        )
        .unwrap();
        assert!(!hit.start_solid);
        assert_eq!(hit.normal, Vec3::NEG_X);
        let expected = (1.7 - DIST_EPSILON) / 6.0;
        assert!((hit.fraction - expected).abs() < 1e-6);
    }

    #[test]
    fn object_collision_stops_walls_and_slides_along_them() {
        let collision = Collision::from_parts(vec![cube()]);
        let extents = Vec3::new(0.3, 0.9, 0.3);
        let old = Vec3::new(-3.0, 1.0, 0.0);
        let stopped = collision.resolve(old, Vec3::new(3.0, 1.0, 0.0), extents);
        assert!(stopped.blocked);
        assert!(stopped.feet.x <= -1.29, "{:?}", stopped);
        assert!((stopped.feet.z - old.z).abs() < 1e-6);

        let slide = collision.resolve(old, Vec3::new(3.0, 1.0, 2.0), extents);
        assert!(slide.blocked);
        assert!(slide.feet.x <= -1.29, "{:?}", slide);
        assert!(slide.feet.z > 1.0, "wall sliding was lost: {slide:?}");
    }

    #[test]
    fn skipped_building_hulls_do_not_block_but_tree_hulls_use_native_flags() {
        let mut skipped = cube();
        skipped.flags = CHDATA_SKIP_COLLISION;
        let collision = Collision::from_parts(vec![skipped]);
        let result = collision.resolve(
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::splat(0.3),
        );
        assert!(!result.blocked);
        assert_eq!(result.feet, Vec3::new(3.0, 0.0, 0.0));
    }

    #[test]
    fn tbh_reader_skips_native_hull_payload_and_reads_shared_brush() {
        let brush = cube();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(b"V0.2======Convex_Hull_Data_Start=========");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"abc");
        for value in brush.center.into_iter().chain(brush.extents) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32; 6] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&brush.flags.to_le_bytes());
        bytes.extend_from_slice(&(brush.sides.len() as u32).to_le_bytes());
        for side in brush.sides {
            for value in side.normal {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&side.distance.to_le_bytes());
            bytes.push(u8::from(side.bevel));
        }
        let parsed = parse_tbh(&bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sides.len(), 6);
        assert_eq!(parsed[0].center, [0.0; 3]);
    }

    #[test]
    fn start_solid_can_escape_instead_of_trapping_the_player() {
        let collision = Collision::from_parts(vec![cube()]);
        let result = collision.resolve(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), Vec3::splat(0.3));
        assert!(result.blocked);
        assert_eq!(result.feet, Vec3::new(3.0, 0.0, 0.0));
    }
}
