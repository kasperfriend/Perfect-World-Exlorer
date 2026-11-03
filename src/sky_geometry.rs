//! Native sky-sphere geometry, panorama projection and join rules (P5.01).
//!
//! Every constant here was recovered from `elementclient.exe` and is checked
//! against an execution of the original code, never inferred from the texture
//! artwork:
//!
//! * `A3DSkySphere::Init` 0x79a330 builds the horizon curtain into the vertex
//!   stream at `this+0x584` (70 vertices, 288 indices).
//! * `A3DSkySphere::Init` 0x79a810 builds the polar cap into the stream at
//!   `this+0x588` (145 vertices, 792 indices; both streams share the index
//!   buffer at `this+0x5a0`).
//! * constructor 0x799610 stores band count `+0x4c0` = 6, angular step
//!   `+0x4c4` = 0.30543262 rad and radius `+0x4c8` = 50.0.
//! * `A3DSky::Draw` 0x799b40 draws the whole curtain twice, once with the front
//!   texture `+0x5a4` (`5\01.bmp`) and once with the back texture `+0x5a8`
//!   (`5\02.bmp`) under a mirrored world matrix; 0x799a40 draws the cap with
//!   `+0x54` (`5\Roof.bmp`).
//! * config keys `sky_image01/02/03` are `5\Roof.bmp`, `5\01.bmp`, `5\02.bmp`.
//!
//! `tools/emulate-sky-sphere.py` runs both builders under Unicorn and
//! `tools/verify-sky-geometry.py` diffs the resulting buffers against
//! [`curtain`] and [`cap`] below: 70/288 and 145/792, 215 vertices and 1080
//! indices in total, worst component delta 7.6e-6 (x87 versus f64 sine and
//! cosine).
//!
//! Joins are hard, not faded. The curtain covers one 180 degree half of the
//! azimuth range and the second draw mirrors `z`, so the meridian at
//! `z = 0` is a texture switch between `01.bmp` and `02.bmp`; inside one draw
//! the two 7-column islands meet at azimuth 90 degrees where the `v` run
//! reverses (0.05 -> 0.5 versus 0.95 -> 0.5). Both joins are exact in
//! position, because the client duplicates the shared column instead of
//! interpolating across it.
//!
//! Only the meshes and the [`project`] reference formulas live here; the
//! renderer mirrors [`project`] in WGSL and the unit tests are what keep the
//! two in step, so unused-in-app items are expected.
#![allow(dead_code)]

/// Band count from the constructor (`this+0x4c0`, 6).
pub const BANDS: usize = 6;
/// Sphere radius in world units (`this+0x4c8`, 50.0).
pub const RADIUS: f32 = 50.0;
/// Angular step of the cap in radians (`this+0x4c4`, 0x3e9c61aa).
pub const ANGULAR_STEP: f32 = 0.305_432_62;
/// Curtain top row height; first entry of the row table at 0x79a4af.
pub const CURTAIN_TOP_Y: f32 = ROW_Y[0];
/// Cap outer ring height, `R * cos(step)`.
pub const CAP_EDGE_Y: f32 = 47.685_848;

/// Row heights of the curtain: `39.27 * [1, 0.5, 0.167, 0, -0.27]`.
pub const ROW_Y: [f32; 5] = [
    39.270_000_5, // rim row, alpha 0
    19.635_000_2,
    6.558_089_7,
    0.0, // horizon
    -10.602_9,
];
/// Texture `v` per row for the frontier island (azimuth 90..180 degrees).
pub const ROW_V_FRONT: [f32; 5] = [0.05, 0.275, 0.424_85, 0.5, 0.5];
/// Texture `v` per row for the back island (azimuth 0..90 degrees).
pub const ROW_V_BACK: [f32; 5] = [0.95, 0.725, 0.575_15, 0.5, 0.5];

/// `0x00ffffff` on the curtain's top row: the skirt fades out at its rim.
pub const CURTAIN_TOP_DIFFUSE: u32 = 0x00ff_ffff;
/// `0xffffffff` on every other row and on the whole cap.
pub const OPAQUE_DIFFUSE: u32 = 0xffff_ffff;

/// One native vertex: position, packed diffuse, texture coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub diffuse: u32,
    pub uv: [f32; 2],
}

/// A recovered vertex/index buffer pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

impl Mesh {
    /// Number of triangles, `indices.len() / 3` for a triangle list.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Which texture of the current sky phase a sample belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// `5\01.bmp`, drawn for the half space the world matrix does not mirror.
    Front,
    /// `5\02.bmp`, drawn mirrored to cover the opposite half space.
    Back,
    /// `5\Roof.bmp`, the polar cap disc.
    Roof,
}

impl Layer {
    /// Index inside a three-texture phase set, matching the load order
    /// `[01.bmp, 02.bmp, roof.bmp]`.
    pub fn slot(self) -> u32 {
        match self {
            Layer::Front => 0,
            Layer::Back => 1,
            Layer::Roof => 2,
        }
    }
}

/// Which recovered surface produced a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Planar disc inside the cap's 17.5 degree cone.
    Cap,
    /// Horizon skirt, `y <= R * pi / 4`.
    Curtain,
    /// Band between the cap rim (72.5 degrees) and the curtain rim (51.75
    /// degrees). The client's two buffers leave this band empty; its own
    /// rim vertex alpha is zero, so the background shows through there.
    CapEdgeGap,
}

/// A texture lookup for one view direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub layer: Layer,
    pub uv: [f32; 2],
    /// Interpolated vertex alpha at this direction; the curtain fades from 1
    /// at `y = 19.635` to 0 at its rim, the cap is always opaque.
    pub alpha: f32,
    pub region: Region,
}

/// Horizon curtain: `A3DSkySphere::Init` 0x79a330 (stream `this+0x584`).
pub fn curtain() -> Mesh {
    let mut vertices = Vec::with_capacity(2 * (BANDS + 1) * ROW_Y.len());
    for (row, height) in ROW_Y.iter().enumerate() {
        let y = *height;
        for half in 0..2 {
            let base = if half == 0 {
                std::f32::consts::FRAC_PI_2
            } else {
                0.0
            };
            for column in 0..=BANDS {
                let angle = base + std::f32::consts::FRAC_PI_2 * column as f32 / BANDS as f32;
                let (sin, cos) = angle.sin_cos();
                let last = row + 1 == ROW_Y.len();
                let v = if last {
                    0.5
                } else if half == 0 {
                    ROW_V_FRONT[row]
                } else {
                    ROW_V_BACK[row]
                };
                vertices.push(Vertex {
                    position: [RADIUS * cos, y, RADIUS * sin],
                    diffuse: if row == 0 {
                        CURTAIN_TOP_DIFFUSE
                    } else {
                        OPAQUE_DIFFUSE
                    },
                    uv: [1.0 - column as f32 / BANDS as f32, v],
                });
            }
        }
    }
    let span = 2 * (BANDS + 1);
    let mut indices = Vec::with_capacity(2 * BANDS * (ROW_Y.len() - 1) * 6);
    for row in 0..ROW_Y.len() - 1 {
        for half in 0..2 {
            for column in 0..BANDS {
                let v0 = (row * span + half * (BANDS + 1) + column) as u16;
                let v1 = v0 + 1;
                let v2 = v0 + span as u16;
                indices.extend_from_slice(&[v0, v2, v2 + 1, v0, v2 + 1, v1]);
            }
        }
    }
    Mesh { vertices, indices }
}

/// Polar cap: `A3DSkySphere::Init` 0x79a810 (stream `this+0x588`).
pub fn cap() -> Mesh {
    let sectors = 4 * BANDS;
    let disc = 2.0 * RADIUS * ANGULAR_STEP.sin();
    let mut vertices = Vec::with_capacity(1 + BANDS * sectors);
    vertices.push(Vertex {
        position: [0.0, RADIUS, 0.0],
        diffuse: OPAQUE_DIFFUSE,
        uv: [0.5, 0.5],
    });
    for ring in 1..=BANDS {
        let theta = ANGULAR_STEP * ring as f32 / BANDS as f32;
        let (sin_t, cos_t) = theta.sin_cos();
        for sector in 0..sectors {
            let phi = std::f32::consts::TAU * sector as f32 / sectors as f32;
            let (sin_p, cos_p) = phi.sin_cos();
            let x = RADIUS * sin_t * cos_p;
            let z = RADIUS * sin_t * sin_p;
            vertices.push(Vertex {
                position: [x, RADIUS * cos_t, z],
                diffuse: OPAQUE_DIFFUSE,
                uv: [x / disc + 0.5, z / disc + 0.5],
            });
        }
    }
    let mut indices = Vec::with_capacity(sectors * (3 + 6 * (BANDS - 1)));
    for sector in 0..sectors {
        let next = (sector + 1) % sectors;
        indices.extend_from_slice(&[0, 1 + sector as u16, 1 + next as u16]);
    }
    for ring in 1..BANDS {
        let inner = 1 + (ring - 1) * sectors;
        let outer = 1 + ring * sectors;
        for sector in 0..sectors {
            let next = (sector + 1) % sectors;
            let a = (inner + sector) as u16;
            let b = (inner + next) as u16;
            let c = (outer + sector) as u16;
            let d = (outer + next) as u16;
            indices.extend_from_slice(&[a, c, d, a, d, b]);
        }
    }
    Mesh { vertices, indices }
}

/// Interpolate one of the two `v` columns of the curtain at height `y`.
///
/// The client stores per-vertex coordinates only, so this reproduces the
/// linear interpolation the rasteriser performs between two rows. Outside the
/// rows it clamps to the nearest rim, which is what a draw limited to the
/// recorded geometry does.
pub fn curtain_v(y: f32, front: bool) -> f32 {
    let table = if front { ROW_V_FRONT } else { ROW_V_BACK };
    if y >= ROW_Y[0] {
        return table[0];
    }
    for i in 0..ROW_Y.len() - 1 {
        if y >= ROW_Y[i + 1] {
            let t = (y - ROW_Y[i]) / (ROW_Y[i + 1] - ROW_Y[i]);
            return table[i] + (table[i + 1] - table[i]) * t;
        }
    }
    table[ROW_Y.len() - 1]
}

/// Vertex alpha at height `y` on the curtain; 0 on the top row.
pub fn curtain_alpha(y: f32) -> f32 {
    if y >= ROW_Y[0] {
        return 0.0;
    }
    if y <= ROW_Y[1] {
        return 1.0;
    }
    (ROW_Y[0] - y) / (ROW_Y[0] - ROW_Y[1])
}

/// Project a unit view direction onto the recovered sky textures.
///
/// Returns `None` only for a direction that is not finite. The `CapEdgeGap`
/// band is reported instead of being blended away, so callers can show the
/// uncovered band rather than hide it.
pub fn project(direction: [f32; 3]) -> Option<Sample> {
    let len = (direction[0] * direction[0]
        + direction[1] * direction[1]
        + direction[2] * direction[2])
        .sqrt();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let d = [direction[0] / len, direction[1] / len, direction[2] / len];
    let y = d[1] * RADIUS;
    if y >= CAP_EDGE_Y {
        // Planar disc: the cap's ring radius is `2 * R * sin(step)`.
        let disc = 2.0 * RADIUS * ANGULAR_STEP.sin();
        return Some(Sample {
            layer: Layer::Roof,
            uv: [0.5 + d[0] * RADIUS / disc, 0.5 + d[2] * RADIUS / disc],
            alpha: 1.0,
            region: Region::Cap,
        });
    }
    // The second draw mirrors z; both draws use the same vertex buffer, so the
    // mirrored half shares the island split at azimuth 90 degrees.
    let (layer, z) = if d[2] >= 0.0 {
        (Layer::Front, d[2])
    } else {
        (Layer::Back, -d[2])
    };
    let azimuth = z.atan2(d[0]).rem_euclid(std::f32::consts::PI);
    let quarter = std::f32::consts::FRAC_PI_2;
    let (front_island, u) = if azimuth >= quarter {
        (true, 2.0 - 2.0 * azimuth / std::f32::consts::PI)
    } else {
        (false, 1.0 - 2.0 * azimuth / std::f32::consts::PI)
    };
    let region = if y > CURTAIN_TOP_Y {
        Region::CapEdgeGap
    } else {
        Region::Curtain
    };
    let clamped = y.min(CURTAIN_TOP_Y);
    Some(Sample {
        layer,
        uv: [u, curtain_v(clamped, front_island)],
        alpha: curtain_alpha(clamped),
        region,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn curtain_matches_the_emulated_native_buffer() {
        let mesh = curtain();
        assert_eq!(mesh.vertices.len(), 70);
        assert_eq!(mesh.indices.len(), 288);
        assert_eq!(mesh.triangles(), 96);
        // Golden vertices from tools/emulate-sky-sphere.py (Unicorn run of
        // A3DSkySphere::Init 0x79a330).
        let cases = [
            (0, [-0.000_002_2, 39.27, 50.0], 0x00ff_ffff, [1.0, 0.05]),
            (1, [-12.940_955, 39.27, 48.296_291], 0x00ff_ffff, [0.833_333_3, 0.05]),
            (6, [-50.0, 39.27, -0.000_004_4], 0x00ff_ffff, [0.0, 0.05]),
            (7, [50.0, 39.27, 0.0], 0x00ff_ffff, [1.0, 0.95]),
            (13, [-0.000_002_2, 39.27, 50.0], 0x00ff_ffff, [0.0, 0.95]),
            (14, [-0.000_002_2, 19.635, 50.0], 0xffff_ffff, [1.0, 0.275]),
            (27, [-0.000_002_2, 19.635, 50.0], 0xffff_ffff, [0.0, 0.725]),
            (41, [-0.000_002_2, 6.558_09, 50.0], 0xffff_ffff, [0.0, 0.575_15]),
            (55, [-0.000_002_2, 0.0, 50.0], 0xffff_ffff, [0.0, 0.5]),
            (69, [-0.000_002_2, -10.602_9, 50.0], 0xffff_ffff, [0.0, 0.5]),
        ];
        for (index, position, diffuse, uv) in cases {
            let v = mesh.vertices[index];
            for k in 0..3 {
                assert!(
                    close(v.position[k], position[k], 1e-3),
                    "vertex {index} axis {k}: {:?}",
                    v.position
                );
            }
            assert_eq!(v.diffuse, diffuse, "vertex {index} diffuse");
            assert!(close(v.uv[0], uv[0], 1e-6), "vertex {index} u {:?}", v.uv);
            assert!(close(v.uv[1], uv[1], 1e-6), "vertex {index} v {:?}", v.uv);
        }
    }

    #[test]
    fn cap_matches_the_emulated_native_buffer() {
        let mesh = cap();
        assert_eq!(mesh.vertices.len(), 145);
        assert_eq!(mesh.indices.len(), 792);
        assert_eq!(mesh.triangles(), 264);
        let cases = [
            (0, [0.0, 50.0, 0.0], [0.5, 0.5]),
            (1, [2.544_170_4, 49.935_230, 0.0], [0.584_606_6, 0.5]),
            (2, [2.457_48, 49.935_230, 0.658_479_8], [0.581_723_7, 0.521_897_8]),
            (13, [-2.544_170_4, 49.935_230, -0.000_000_2], [0.415_393_4, 0.5]),
            (144, [14.522_973, 47.685_848, -3.891_416_3], [0.982_962_8, 0.370_590_6]),
        ];
        for (index, position, uv) in cases {
            let v = mesh.vertices[index];
            for k in 0..3 {
                assert!(close(v.position[k], position[k], 1e-4), "cap vertex {index}");
            }
            assert_eq!(v.diffuse, OPAQUE_DIFFUSE);
            assert!(close(v.uv[0], uv[0], 1e-5), "cap u {index}");
            assert!(close(v.uv[1], uv[1], 1e-5), "cap v {index}");
        }
        // Every cap triangle keeps the recovered order: the signed area of the
        // x/z projection is positive for all 264 of them, so the cap faces the
        // camera when seen from inside the sphere.
        for triangle in mesh.indices.chunks(3) {
            let (a, b, c) = (
                mesh.vertices[triangle[0] as usize],
                mesh.vertices[triangle[1] as usize],
                mesh.vertices[triangle[2] as usize],
            );
            let area = (b.position[0] - a.position[0]) * (c.position[2] - a.position[2])
                - (c.position[0] - a.position[0]) * (b.position[2] - a.position[2]);
            assert!(area > 0.0, "cap triangle winding at {triangle:?}");
        }
    }

    #[test]
    fn curtain_faces_the_camera_inside_the_sphere() {
        let mesh = curtain();
        for triangle in mesh.indices.chunks(3) {
            let a = mesh.vertices[triangle[0] as usize].position;
            let b = mesh.vertices[triangle[1] as usize].position;
            let c = mesh.vertices[triangle[2] as usize].position;
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let normal = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let middle = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let outward = normal[0] * middle[0] + normal[1] * middle[1] + normal[2] * middle[2];
            assert!(outward < 0.0, "curtain triangle {triangle:?} faces outward");
        }
    }

    #[test]
    fn combination_covers_both_rims_without_shared_vertices() {
        let (curtain, cap) = (curtain(), cap());
        assert_eq!(curtain.vertices.len() + cap.vertices.len(), 215);
        assert_eq!(curtain.indices.len() + cap.indices.len(), 1080);
        assert!(
            ROW_Y[0] < CAP_EDGE_Y,
            "the native buffers leave the band between 51.75 and 72.5 degrees open"
        );
    }

    #[test]
    fn island_join_is_a_duplicated_column_with_a_reversed_texture_run() {
        let mesh = curtain();
        let span = 2 * (BANDS + 1);
        for row in 0..ROW_Y.len() {
            // Azimuth 90 degrees is stored twice: once as the first column of
            // the front island (u = 1) and once as the last column of the back
            // island (u = 0). Same position, opposite end of the same run, so
            // the client never interpolates across the seam.
            let front_first = mesh.vertices[row * span];
            let back_last = mesh.vertices[row * span + span - 1];
            for axis in [0, 1, 2] {
                assert!(
                    close(front_first.position[axis], back_last.position[axis], 1e-4),
                    "row {row} axis {axis}"
                );
            }
            assert_eq!(front_first.uv[0], 1.0);
            assert_eq!(back_last.uv[0], 0.0);
            assert_ne!(front_first.uv[1], back_last.uv[1]);
        }
    }

    #[test]
    fn meridian_join_reuses_one_vertex_for_both_draws() {
        let mesh = curtain();
        let span = 2 * (BANDS + 1);
        for row in 0..ROW_Y.len() {
            // Azimuth 0 and 180 sit in the mirror plane: the second draw feeds
            // the identical vertex through a z-reflected world matrix, so the
            // ring closes with no extra column and no blend.
            let back_first = mesh.vertices[row * span + BANDS + 1];
            let front_last = mesh.vertices[row * span + BANDS];
            assert!(close(back_first.position[2], 0.0, 1e-4));
            assert!(close(front_last.position[2], 0.0, 1e-4));
            assert!(close(back_first.position[0], -front_last.position[0], 1e-4));
            assert_ne!(back_first.uv[0], front_last.uv[0]);
        }
    }

    #[test]
    fn projection_agrees_with_the_vertex_buffer_inside_the_curtain() {
        let mesh = curtain();
        for index in [0usize, 7, 14, 27, 41, 55, 69] {
            let v = mesh.vertices[index];
            let len = (v.position[0] * v.position[0]
                + v.position[1] * v.position[1]
                + v.position[2] * v.position[2])
                .sqrt();
            let direction = [
                v.position[0] / len,
                v.position[1] / len,
                v.position[2] / len,
            ];
            let sample = project(direction).expect("finite direction");
            if sample.region == Region::Curtain {
                assert!(close(sample.uv[0], v.uv[0], 1e-4), "u at vertex {index}");
                assert!(close(sample.uv[1], v.uv[1], 1e-4), "v at vertex {index}");
            }
        }
    }

    #[test]
    fn projection_agrees_with_the_vertex_buffer_inside_the_cap() {
        let mesh = cap();
        for index in [1usize, 2, 13, 25, 144] {
            let v = mesh.vertices[index];
            let len = (v.position[0] * v.position[0]
                + v.position[1] * v.position[1]
                + v.position[2] * v.position[2])
                .sqrt();
            let direction = [
                v.position[0] / len,
                v.position[1] / len,
                v.position[2] / len,
            ];
            let sample = project(direction).expect("finite direction");
            assert_eq!(sample.region, Region::Cap, "cap vertex {index}");
            assert_eq!(sample.layer, Layer::Roof);
            assert!(close(sample.uv[0], v.uv[0], 1e-4), "cap u at {index}");
            assert!(close(sample.uv[1], v.uv[1], 1e-4), "cap v at {index}");
        }
    }

    #[test]
    fn opposite_halves_use_the_front_and_back_textures() {
        let front = project([0.3, 0.2, 0.9]).unwrap();
        let back = project([0.3, 0.2, -0.9]).unwrap();
        assert_eq!(front.layer, Layer::Front);
        assert_eq!(back.layer, Layer::Back);
        assert!(close(front.uv[0], back.uv[0], 1e-5));
        assert!(close(front.uv[1], back.uv[1], 1e-5));
        // The meridian itself is a hard switch, never a blend.
        assert_eq!(project([1.0, 0.1, 0.0]).unwrap().layer, Layer::Front);
        assert_eq!(project([1.0, 0.1, -0.0]).unwrap().layer, Layer::Front);
        assert_eq!(project([1.0, 0.1, -1e-6]).unwrap().layer, Layer::Back);
    }

    #[test]
    fn cap_edge_gap_is_reported_and_invisible() {
        for elevation in [55.0_f32, 60.0, 70.0] {
            let (sin, cos) = elevation.to_radians().sin_cos();
            let sample = project([cos, sin, 0.0]).unwrap();
            assert_eq!(sample.region, Region::CapEdgeGap, "elevation {elevation}");
            assert_eq!(sample.alpha, 0.0, "gap must stay transparent");
        }
        let (sin, cos) = 30.0_f32.to_radians().sin_cos();
        assert_eq!(project([cos, sin, 0.0]).unwrap().region, Region::Curtain);
        let (sin, cos) = 80.0_f32.to_radians().sin_cos();
        assert_eq!(project([cos, sin, 0.0]).unwrap().region, Region::Cap);
    }

    #[test]
    fn curtain_alpha_fades_to_zero_at_the_recovered_rim() {
        assert_eq!(curtain_alpha(ROW_Y[0]), 0.0);
        assert_eq!(curtain_alpha(ROW_Y[0] + 5.0), 0.0);
        assert_eq!(curtain_alpha(ROW_Y[1]), 1.0);
        assert_eq!(curtain_alpha(-30.0), 1.0);
        assert!((curtain_alpha(15.0) - 0.618_65).abs() < 1e-3);
    }

    #[test]
    fn non_finite_directions_are_rejected() {
        assert!(project([f32::NAN, 0.0, 0.0]).is_none());
        assert!(project([0.0, 0.0, 0.0]).is_none());
    }
}
