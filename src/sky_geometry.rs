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
//!   texture `+0x5a4` (`clear\01.bmp`) and once with the back texture `+0x5a8`
//!   (`clear\02.bmp`) under a mirrored world matrix; 0x799a40 draws the cap
//!   with `+0x54` (`clear\roof.bmp`).
//! * the names come from configuration: `element_client.cfg` `[DefEnv]`
//!   `sky_image01/02/03` are `Sky\clear\Roof.bmp`, `Sky\clear\01.bmp` and
//!   `Sky\clear\02.bmp`, `autofamilyconfigs/sky.lst` lists the named sets, and
//!   0x79ac30 (`TransSky`) prefixes `Textures\Sky\` to the three paths it is
//!   given.
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
/// Sine of the curtain rim (38.16 degrees) and of the cap rim (72.5 degrees):
/// the ends of the band the two native buffers leave open.
pub const GAP_BOTTOM_SIN: f32 = 0.618;
/// Sine of the cap rim, the top of the same band.
pub const GAP_TOP_SIN: f32 = 0.9537;

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
    /// `clear\01.bmp`, drawn for the half space the world matrix does not mirror.
    Front,
    /// `clear\02.bmp`, drawn mirrored to cover the opposite half space.
    Back,
    /// `clear\roof.bmp`, the polar cap disc.
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
    /// Band between the curtain rim (38.16 degrees) and the cap rim (72.5
    /// degrees). The client's two buffers leave this band empty; the viewer
    /// covers it with the cap's own lookup continued outward, which is the same
    /// surface the cap draws and therefore joins it without an edge.
    CapEdgeGap,
}

/// A texture lookup for one view direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub layer: Layer,
    pub uv: [f32; 2],
    /// Interpolated vertex alpha at this direction; the curtain fades from 1
    /// at `y = 19.635` to 0 at its rim, the cap and the band are opaque.
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

/// Vertical drop `A3DSky::Render` applies to the cap (`0x799a53`:
/// `translation.y -= cos(+0x4c4) * [+0x4c8]`), which lands the rim on the
/// horizon plane and the apex `RADIUS - CAP_DROP` above the eye.
pub const CAP_DROP: f32 = 47.685_848;

/// Where a ray of elevation `dy` meets the cap, measured from the eye.
///
/// The cap is the top of a sphere of radius [`RADIUS`] whose centre sits
/// [`CAP_DROP`] below the eye, so `|t*d - (0, -CAP_DROP, 0)| = RADIUS` gives
/// `t = -CAP_DROP*dy + sqrt(R² - CAP_DROP²(1 - dy²))`. At the horizon
/// (`dy = 0`) that is `R*sin(step)` = 15.035290, the rim; at the zenith it is
/// `RADIUS - CAP_DROP` = 2.314152, the apex geometry. `dy` is clamped to zero
/// because the cap only exists above the eye.
pub fn bowl_hit(dy: f32) -> f32 {
    let dy = dy.max(0.0);
    let under = RADIUS * RADIUS - CAP_DROP * CAP_DROP * (1.0 - dy * dy);
    -CAP_DROP * dy + under.max(0.0).sqrt()
}

/// Planar cap lookup for a unit direction: `0.5 + hit.xz / disc`.
///
/// The value is the client's own vertex UV (`0x79aa73`: `(1 + p.x / (R
/// sin(step))) / 2`, i.e. `0.5 + p.xz / (2 R sin(step))`), evaluated at the
/// point the ray reaches on the dropped cap instead of at the vertex itself.
/// The disc is therefore `2 R sin(step)` = 30.071 and the whole texture spans
/// the cap: the centre lands on the zenith and the horizon ray lands on the
/// rim, which is why the sampled radius runs from 0.5 at the horizon to 0
/// overhead.
pub fn cap_uv(d: [f32; 3]) -> [f32; 2] {
    let disc = 2.0 * RADIUS * ANGULAR_STEP.sin();
    let t = bowl_hit(d[1]);
    [0.5 + t * d[0] / disc, 0.5 + t * d[2] / disc]
}

/// Project a unit view direction onto the recovered sky textures.
///
/// Returns `None` only for a direction that is not finite. `CapEdgeGap` rays
/// carry the cap's own lookup, continued outward: the shader draws that same
/// surface across the band, and the curtain's authored alpha fades over it.
pub fn project(direction: [f32; 3]) -> Option<Sample> {
    let len =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let d = [direction[0] / len, direction[1] / len, direction[2] / len];
    // The cap is a sphere, so a ray meets it at `50 * d`; the curtain is a
    // cylinder or radius 50, so a ray meets it at `50 * tan(elevation)`.
    if d[1] * RADIUS >= CAP_EDGE_Y {
        return Some(Sample {
            layer: Layer::Roof,
            uv: cap_uv(d),
            alpha: 1.0,
            region: Region::Cap,
        });
    }
    let horizontal = (d[0] * d[0] + d[2] * d[2]).sqrt();
    if horizontal <= 1e-6 {
        return None;
    }
    let y = RADIUS * d[1] / horizontal;
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
    if y > CURTAIN_TOP_Y {
        // The band has no native geometry. The shader continues the cap's own
        // planar lookup over the whole upper hemisphere — the same surface the
        // cap draws outside its 17.5 degree disc — and `clear\roof.bmp tiles`,
        // so the repeat sampler carries it past one tile. On the cap rim the
        // radius is exactly the cap's 0.5, so both surfaces are continuous.
        return Some(Sample {
            layer: Layer::Roof,
            uv: cap_uv(d),
            alpha: 1.0,
            region: Region::CapEdgeGap,
        });
    }
    Some(Sample {
        layer,
        uv: [u, curtain_v(y, front_island)],
        alpha: curtain_alpha(y),
        region: Region::Curtain,
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
            (
                1,
                [-12.940_955, 39.27, 48.296_291],
                0x00ff_ffff,
                [0.833_333_3, 0.05],
            ),
            (6, [-50.0, 39.27, -0.000_004_4], 0x00ff_ffff, [0.0, 0.05]),
            (7, [50.0, 39.27, 0.0], 0x00ff_ffff, [1.0, 0.95]),
            (13, [-0.000_002_2, 39.27, 50.0], 0x00ff_ffff, [0.0, 0.95]),
            (14, [-0.000_002_2, 19.635, 50.0], 0xffff_ffff, [1.0, 0.275]),
            (27, [-0.000_002_2, 19.635, 50.0], 0xffff_ffff, [0.0, 0.725]),
            (
                41,
                [-0.000_002_2, 6.558_09, 50.0],
                0xffff_ffff,
                [0.0, 0.575_15],
            ),
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
            (
                2,
                [2.457_48, 49.935_230, 0.658_479_8],
                [0.581_723_7, 0.521_897_8],
            ),
            (
                13,
                [-2.544_170_4, 49.935_230, -0.000_000_2],
                [0.415_393_4, 0.5],
            ),
            (
                144,
                [14.522_973, 47.685_848, -3.891_416_3],
                [0.982_962_8, 0.370_590_6],
            ),
        ];
        for (index, position, uv) in cases {
            let v = mesh.vertices[index];
            for k in 0..3 {
                assert!(
                    close(v.position[k], position[k], 1e-4),
                    "cap vertex {index}"
                );
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
            "the native buffers leave the band between 38.16 and 72.5 degrees open"
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
            // Every row mirrors around the texture's horizontal midline, so the
            // two runs always add up to one texture height.
            assert!(
                close(front_first.uv[1] + back_last.uv[1], 1.0, 1e-6),
                "row {row}: {} / {}",
                front_first.uv[1],
                back_last.uv[1]
            );
            if ROW_Y[row] <= 0.0 {
                // The two below-horizon rows (y = 0 and y = -10.6) collapse
                // both islands onto that midline instead of running the image
                // any further down.
                assert_eq!(front_first.uv[1], 0.5);
                assert_eq!(back_last.uv[1], 0.5);
            } else {
                assert_ne!(front_first.uv[1], back_last.uv[1]);
            }
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
        for index in [7usize, 14, 21, 28, 35, 42, 49, 56, 63] {
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
        // The mesh is built in object space, where the cap is a sphere around
        // the origin; `A3DSky::Render` drops it by `CAP_DROP` so the eye sits
        // `CAP_DROP` *above* that centre. Casting the ray from the eye is what
        // has to reproduce the stored UVs, `0.5 + p.xz / (2 R sin(step))`
        // (`0x79aa73`).
        let mesh = cap();
        let mut roof_samples = 0;
        for index in [1usize, 2, 13, 25, 144] {
            let v = mesh.vertices[index];
            let eye = [0.0, CAP_DROP, 0.0];
            let rel = [
                v.position[0] - eye[0],
                v.position[1] - eye[1],
                v.position[2] - eye[2],
            ];
            let len = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
            let direction = [rel[0] / len, rel[1] / len, rel[2] / len];
            let sample = project(direction).expect("finite direction");
            // Seen from the dropped eye the cap's outer rings sit at the horizon,
            // so some of them fall on the curtain's side of the divide; those are
            // the skirt's business, not the ceiling's.
            if sample.layer != Layer::Roof {
                continue;
            }
            roof_samples += 1;
            assert!(
                matches!(sample.region, Region::Cap | Region::CapEdgeGap),
                "cap vertex {index} region {:?}",
                sample.region
            );
            assert!(close(sample.uv[0], v.uv[0], 1e-4), "cap u at {index}");
            assert!(close(sample.uv[1], v.uv[1], 1e-4), "cap v at {index}");
        }
        assert!(
            roof_samples >= 2,
            "only {roof_samples} cap vertices checked"
        );
        // Overhead is unambiguously the ceiling, and it is the sheet's centre.
        let zenith = project([0.0, 1.0, 0.0]).expect("zenith");
        assert_eq!(zenith.region, Region::Cap);
        assert!(close(zenith.uv[0], 0.5, 1e-6) && close(zenith.uv[1], 0.5, 1e-6));
        // The rim of the cap is the horizon ray, and its stored UV is 1.0.
        let rim = mesh
            .vertices
            .iter()
            .find(|v| (v.position[1] - CAP_DROP).abs() < 1e-3)
            .expect("rim ring");
        assert!(close(rim.uv[0], 1.0, 1e-4) || close(rim.uv[1], 1.0, 1e-4));
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
    fn the_open_band_is_the_cap_surface_continued() {
        // The band carries the cap's own lookup, evaluated outside the cap
        // disc: one smooth function of the direction, so it can neither hold a
        // single circle (radial streaks) nor be a fixed colour (a ring).
        let mut radii = Vec::new();
        for elevation in [39.0_f32, 50.0, 60.0, 70.0] {
            let (sin, cos) = elevation.to_radians().sin_cos();
            let sample = project([cos, sin, 0.0]).unwrap();
            assert_eq!(sample.region, Region::CapEdgeGap, "elevation {elevation}");
            assert_eq!(sample.layer, Layer::Roof);
            assert_eq!(sample.alpha, 1.0, "the band is opaque at {elevation}");
            radii.push((sample.uv[0] - 0.5).hypot(sample.uv[1] - 0.5));
        }
        // Radius shrinks monotonically as the elevation rises: the horizon ray
        // meets the rim of the dropped cap (radius 0.5 of the sheet) and the
        // zenith meets its apex (radius 0).
        assert!(radii.windows(2).all(|w| w[1] < w[0]), "radii {radii:?}");
        assert!(
            radii[0] > 0.085 && radii[0] < 0.1,
            "band floor {}",
            radii[0]
        );
        assert!(
            radii[3] > 0.025 && radii[3] < 0.032,
            "near the cap {}",
            radii[3]
        );
        // The horizon ray lands on the sheet's rim, which is where the
        // curtain's own texture edge is too.
        let rim = project([1.0, 0.0, 0.0]).unwrap();
        assert!(close(rim.uv[0], 1.0, 1e-4), "rim {}", rim.uv[0]);
        let zenith = project([0.0, 1.0, 0.0]).unwrap();
        assert!(close(zenith.uv[0], 0.5, 1e-4) && close(zenith.uv[1], 0.5, 1e-4));
        // Continuity with the cap: 72.4 and 72.6 degrees are the same texel.
        let (sin, cos) = 72.4_f32.to_radians().sin_cos();
        let below = project([cos, sin, 0.0]).unwrap();
        let (sin, cos) = 72.6_f32.to_radians().sin_cos();
        let above = project([cos, sin, 0.0]).unwrap();
        assert_eq!(above.region, Region::Cap);
        // 0.2 degrees of elevation either side of the rim is one texel of the
        // 512 square roof sheet, so the two surfaces meet without an edge.
        assert!(close(below.uv[0], above.uv[0], 1e-2), "u joins the cap");
        assert!(close(below.uv[1], above.uv[1], 1e-2), "v joins the cap");
    }

    #[test]
    fn regions_follow_the_recovered_rims() {
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
        // Halfway up the opaque part of the skirt.
        assert!((curtain_alpha(29.45) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn non_finite_directions_are_rejected() {
        assert!(project([f32::NAN, 0.0, 0.0]).is_none());
        assert!(project([0.0, 0.0, 0.0]).is_none());
    }
}
