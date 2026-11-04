//! Terrain block residency and per-view visibility (`0x77e250`).
//!
//! The client walks the camera block window every frame (`0x77ecf0`, see
//! [`crate::terrain_lod`]). For each slot it skips blocks whose data is not
//! loaded (`block+0x18`), asks the view whether the block's bounds
//! (`block+0x54`, `block+0x60`) are visible, stores the answer in `block+0x74`,
//! builds the full-resolution mesh on entry (`0x7c1140`) and counts the visible
//! blocks in `terrain+0x324`; the level rule `0x77e360` then coarsens the mesh
//! already built. Blocks that leave the window are dropped by the loader.
//!
//! This module keeps the same lifecycle for the viewer:
//!
//! * a block enters the resident set when the window first covers it, and its
//!   mesh is built exactly once at the level the rule picked;
//! * it is re-meshed only when its level changes;
//! * it is evicted when the window moves off it, freeing its geometry;
//! * the submitted set is the resident set filtered per view by the same
//!   bounds test the client stores in `block+0x74`.
//!
//! Malformed or missing block data is reported as an error naming the block.

use crate::gpu::Vertex;
use glam::{Mat4, Vec3};

/// One resident block: the geometry built for its level plus the bounds the
/// per-view visibility test needs.
pub struct Block {
    level: u8,
    cases: [u8; 4],
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    /// World-space `[x0, y0, z0, x1, y1, z1]` of the block surface.
    bounds: [f32; 6],
}

/// Result of one residency pass, the viewer's equivalent of the block walk in
/// `0x77e250` and its visible-block counter at `terrain+0x324`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Census {
    /// Blocks holding geometry after the pass.
    pub resident: usize,
    /// Blocks that entered this pass (their mesh was built).
    pub loaded: usize,
    /// Blocks the window moved off, whose geometry was released.
    pub evicted: usize,
    /// Resident blocks re-meshed because their level changed.
    pub remeshed: usize,
    /// Triangles held by the resident blocks.
    pub triangles: u32,
}

/// One resident block inside the assembled terrain buffers.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    /// Block coordinates, kept for diagnostics and the tests.
    #[allow(dead_code)]
    pub block: (usize, usize),
    /// Index-buffer range of this block's triangles.
    pub start: u32,
    pub count: u32,
    pub bounds: [f32; 6],
}

/// Resident block geometry, owned across rebuilds so that entering blocks are
/// the only ones meshed and evicted blocks release their vertices.
#[derive(Default)]
pub struct Residency {
    blocks: std::collections::BTreeMap<(usize, usize), Block>,
}

impl Residency {
    /// Resident block count; the draw path uses the assembled buffers, this is
    /// the standalone census used by tests.
    #[allow(dead_code)]
    pub fn resident(&self) -> usize {
        self.blocks.len()
    }

    pub fn triangles(&self) -> u32 {
        self.blocks
            .values()
            .map(|b| (b.indices.len() / 3) as u32)
            .sum()
    }

    /// Apply one window to the resident set: evict blocks outside it, build the
    /// blocks that entered and re-mesh the ones whose level changed. `level_of`
    /// returns `0` for a block the rule does not draw.
    pub fn update<L, B>(
        &mut self,
        window: &crate::terrain_lod::Window,
        level_of: L,
        mut build: B,
    ) -> Result<Census, String>
    where
        L: Fn(usize, usize) -> u8,
        B: FnMut(usize, usize, u8) -> Result<(Vec<Vertex>, Vec<u32>, [f32; 6]), String>,
    {
        self.update_stitched(window, |x, y| (level_of(x, y), [0; 4]), build)
    }

    /// Stitching is geometry: a neighbor can change our boundary without
    /// changing our own resolution. Include all four cases in the cache key.
    pub fn update_stitched<L, B>(
        &mut self,
        window: &crate::terrain_lod::Window,
        state_of: L,
        mut build: B,
    ) -> Result<Census, String>
    where
        L: Fn(usize, usize) -> (u8, [u8; 4]),
        B: FnMut(usize, usize, u8) -> Result<(Vec<Vertex>, Vec<u32>, [f32; 6]), String>,
    {
        let mut census = Census::default();
        // Eviction first: the client's loader drops the slots the window left.
        let before = self.blocks.len();
        self.blocks.retain(|&(x, y), _| window.contains(x, y));
        census.evicted = before - self.blocks.len();
        for y in window.y0..window.y1 {
            for x in window.x0..window.x1 {
                let (level, cases) = state_of(x, y);
                if level == 0 {
                    // No mesh for this block (`block+0x70 == 0`).
                    self.blocks.remove(&(x, y));
                    continue;
                }
                let stale = match self.blocks.get(&(x, y)) {
                    Some(block) => block.level != level || block.cases != cases,
                    None => true,
                };
                if !stale {
                    continue;
                }
                let (vertices, indices, bounds) = build(x, y, level)
                    .map_err(|e| format!("terrain block ({x},{y}) level {level}: {e}"))?;
                if self.blocks.contains_key(&(x, y)) {
                    census.remeshed += 1;
                } else {
                    census.loaded += 1;
                }
                self.blocks.insert(
                    (x, y),
                    Block {
                        level,
                        cases,
                        vertices,
                        indices,
                        bounds,
                    },
                );
            }
        }
        census.resident = self.blocks.len();
        census.triangles = self.triangles();
        Ok(census)
    }

    /// Append the resident blocks of `window` to the terrain buffers in window
    /// order and report the index range each one occupies.
    pub fn append(
        &self,
        window: &crate::terrain_lod::Window,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) -> Vec<Span> {
        let mut spans = Vec::new();
        for y in window.y0..window.y1 {
            for x in window.x0..window.x1 {
                let Some(block) = self.blocks.get(&(x, y)) else {
                    continue;
                };
                if block.indices.is_empty() {
                    continue;
                }
                let base = vertices.len() as u32;
                let start = indices.len() as u32;
                vertices.extend_from_slice(&block.vertices);
                indices.extend(block.indices.iter().map(|i| base + *i));
                spans.push(Span {
                    block: (x, y),
                    start,
                    count: block.indices.len() as u32,
                    bounds: block.bounds,
                });
            }
        }
        spans
    }
}

/// Conservative bounding-box/frustum test, the viewer's stand-in for the view
/// call that fills `block+0x74`: a box is culled only when all eight corners
/// fall outside the same clip plane, so nothing that could be on screen is
/// dropped.
pub fn in_frustum(matrix: Mat4, bounds: [f32; 6]) -> bool {
    let mut corners = [Vec3::ZERO; 8];
    for (i, corner) in corners.iter_mut().enumerate() {
        *corner = Vec3::new(
            if i & 1 == 0 { bounds[0] } else { bounds[3] },
            if i & 2 == 0 { bounds[1] } else { bounds[4] },
            if i & 4 == 0 { bounds[2] } else { bounds[5] },
        );
    }
    let clip = corners.map(|c| matrix * c.extend(1.));
    !(clip.iter().all(|c| c.x < -c.w)
        || clip.iter().all(|c| c.x > c.w)
        || clip.iter().all(|c| c.y < -c.w)
        || clip.iter().all(|c| c.y > c.w)
        || clip.iter().all(|c| c.z < 0.)
        || clip.iter().all(|c| c.z > c.w))
}

/// Merge the visible blocks of one view into contiguous index ranges, so a
/// frame is submitted as a handful of draw calls instead of one per block.
pub fn visible_runs(spans: &[Span], matrix: Mat4) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for span in spans {
        if span.count == 0 || !in_frustum(matrix, span.bounds) {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.0 + last.1 == span.start => last.1 += span.count,
            _ => runs.push((span.start, span.count)),
        }
    }
    runs
}

/// Visible block count for one view (`terrain+0x324`). The draw path submits
/// `visible_runs`; this stays as the standalone census.
#[allow(dead_code)]
pub fn visible_blocks(spans: &[Span], matrix: Mat4) -> usize {
    spans
        .iter()
        .filter(|s| s.count != 0 && in_frustum(matrix, s.bounds))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain_lod::Window;

    fn vertex(x: f32) -> Vertex {
        Vertex {
            pos: [x, 0., 0.],
            normal: [0., 1., 0.],
            uv: [0.; 2],
            color: [1.; 4],
            kind: 0.,
            diffuse: 0xffff_ffff,
            night_diffuse: 0xffff_ffff,
        }
    }

    /// A two-cell-wide block whose bounds stay in front of the camera.
    fn block(x: f32) -> (Vec<Vertex>, Vec<u32>, [f32; 6]) {
        (
            vec![vertex(x), vertex(x + 1.)],
            vec![0, 1, 0],
            [x, 0., 0., x + 1., 0., 1.],
        )
    }

    fn window(x0: usize, y0: usize, x1: usize, y1: usize) -> Window {
        Window { x0, y0, x1, y1 }
    }

    #[test]
    fn blocks_enter_with_the_window_and_are_evicted_when_it_leaves() {
        let mut residency = Residency::default();
        let first = window(0, 0, 2, 2);
        let census = residency
            .update(&first, |_, _| 3, |x, y, _| Ok(block(x as f32)))
            .expect("builds");
        assert_eq!(
            census,
            Census {
                resident: 4,
                loaded: 4,
                evicted: 0,
                remeshed: 0,
                triangles: 4
            }
        );
        // Slide one block east: the west column leaves, the east column enters.
        let second = window(1, 0, 3, 2);
        let census = residency
            .update(&second, |_, _| 3, |x, y, _| Ok(block(x as f32)))
            .expect("builds");
        assert_eq!(census.loaded, 2);
        assert_eq!(census.evicted, 2);
        assert_eq!(census.remeshed, 0);
        assert_eq!(residency.resident(), 4);
        // Only the blocks the window still covers are drawn.
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let spans = residency.append(&second, &mut vertices, &mut indices);
        assert_eq!(spans.len(), 4);
        assert!(spans.iter().all(|s| s.block.0 >= 1));
        assert_eq!(indices.len(), 12);
    }

    #[test]
    fn a_level_change_re_meshes_only_that_block() {
        let mut residency = Residency::default();
        let win = window(0, 0, 2, 1);
        residency
            .update(&win, |_, _| 3, |x, y, _| Ok(block(x as f32)))
            .expect("builds");
        let census = residency
            .update(
                &win,
                |x, _| if x == 1 { 2 } else { 3 },
                |x, y, level| {
                    assert_eq!(level, 2, "only block 1 changed level");
                    Ok(block(x as f32 + 0.5))
                },
            )
            .expect("builds");
        assert_eq!(census.remeshed, 1);
        assert_eq!(census.loaded, 0);
        assert_eq!(census.evicted, 0);
        assert_eq!(census.resident, 2);
    }

    #[test]
    fn a_block_that_cannot_be_built_is_named() {
        let mut residency = Residency::default();
        let err = residency
            .update(
                &window(0, 0, 2, 1),
                |_, _| 3,
                |x, y, _| {
                    if (x, y) == (1, 0) {
                        Err("truncated height data".into())
                    } else {
                        Ok(block(x as f32))
                    }
                },
            )
            .expect_err("must fail");
        assert!(err.contains("(1,0)"), "{err}");
        assert!(err.contains("truncated height data"), "{err}");
    }

    #[test]
    fn a_level_zero_block_is_not_built() {
        let mut residency = Residency::default();
        let census = residency
            .update(
                &window(0, 0, 3, 1),
                |x, _| if x == 1 { 0 } else { 3 },
                |x, _, _| Ok(block(x as f32)),
            )
            .expect("builds");
        assert_eq!(census.resident, 2);
        assert_eq!(census.loaded, 2);
    }

    #[test]
    fn frustum_test_keeps_what_is_on_screen_and_drops_what_is_behind() {
        let identity = Mat4::IDENTITY;
        // In front of the camera, inside the unit cube.
        assert!(in_frustum(identity, [-0.5, -0.5, 0.25, 0.5, 0.5, 0.75]));
        // Entirely behind the near plane.
        assert!(!in_frustum(identity, [-0.5, -0.5, -2., 0.5, 0.5, -1.]));
        // Entirely to the right of the view.
        assert!(!in_frustum(identity, [2., -0.5, 0.25, 3., 0.5, 0.75]));
        // Straddling the right plane is still drawn.
        assert!(in_frustum(identity, [0.5, -0.5, 0.25, 1.5, 0.5, 0.75]));
    }

    #[test]
    fn visible_runs_merge_neighbours_and_skip_the_culled_ones() {
        let spans = vec![
            Span {
                block: (0, 0),
                start: 0,
                count: 6,
                bounds: [-0.5, -0.5, 0.25, 0.5, 0.5, 0.75],
            },
            Span {
                block: (1, 0),
                start: 6,
                count: 6,
                bounds: [-0.5, -0.5, 0.25, 0.5, 0.5, 0.75],
            },
            Span {
                block: (2, 0),
                start: 12,
                count: 6,
                // Behind the camera: this block is not submitted.
                bounds: [-0.5, -0.5, -2., 0.5, 0.5, -1.],
            },
            Span {
                block: (3, 0),
                start: 18,
                count: 6,
                bounds: [-0.5, -0.5, 0.25, 0.5, 0.5, 0.75],
            },
        ];
        assert_eq!(
            visible_runs(&spans, Mat4::IDENTITY),
            vec![(0, 12), (18, 6)],
            "contiguous visible blocks share one draw call"
        );
        assert_eq!(visible_blocks(&spans, Mat4::IDENTITY), 3);
    }
}
