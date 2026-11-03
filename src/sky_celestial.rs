//! Native sun/moon billboard geometry and visibility rules (P5.03).
//!
//! Recovered from `elementclient.exe`:
//!
//! * `CECSunMoon` render 0x4b3e00..0x4b41ee submits two billboards. Each one
//!   is gated on its own elevation (`+0x4c` for the sun, `+0x50` for the moon)
//!   being greater than `0.0` (`fcomp ds:0x8d58a0`), places the quad centre at
//!   `direction * ds:0x8d6a9c` (100.0 world units) and offsets the four corners
//!   by `+/- (0.5 * 2.0)` (constants `0x8d6a94` and `0x8d6a98`), so the disc
//!   subtends `2 * atan(1 / 100)` = 1.146 degrees.
//! * The direction is rebuilt every frame from `[+0x50]`/`[+0x4c]` and `[+0x60]`
//!   as `(cos(az)cos(el), sin(el), sin(az)cos(el))`.
//! * The vertex diffuse alpha is `fade_byte << 24` over an opaque RGB, taken
//!   from `+0x48`; the fade itself is the `0.0003 / ms` ramp in
//!   [`crate::client_sun::Visibility`] (`+0x7c` for the sun, `+0x80` for the
//!   moon).
//! * Textures are `textures/nature/sun.bmp` and `textures/nature/moon.tga`.
//!
//! No halo or flare geometry is submitted by that path: the flare looks are
//! `solarflare_14.txt` style pixel-shader effects, and the package that holds
//! them (`shaders.pck`) uses a key this repository cannot read yet. States this
//! module cannot reproduce are reported as [`CelestialError`] instead of being
//! drawn approximately.

use crate::sky_time::SunMoon;

/// Placement distance of both discs in world units (`ds:0x8d6a9c`).
pub const DISTANCE: f32 = 100.0;
/// Half extent of the quad (`0.5 * 2.0`).
pub const HALF_EXTENT: f32 = 1.0;
/// Index list submitted per disc (six indices, two triangles).
pub const DISC_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// Which of the two objects a disc belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscKind {
    Sun,
    Moon,
}

impl DiscKind {
    /// Config/asset name, also used in diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            DiscKind::Sun => "sun",
            DiscKind::Moon => "moon",
        }
    }
    /// Native texture path.
    pub fn texture(self) -> &'static str {
        match self {
            DiscKind::Sun => "textures/nature/sun.bmp",
            DiscKind::Moon => "textures/nature/moon.tga",
        }
    }
}

/// Why a disc cannot be drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum CelestialError {
    /// The supplied state was not finite.
    NotFinite(&'static str),
    /// Native gate: the object's elevation is not above the horizon.
    BelowHorizon {
        kind: &'static str,
        elevation_degrees: f32,
    },
    /// The regional flags switched the object off and its fade reached zero.
    Hidden { kind: &'static str, fade: f32 },
    /// The native texture is missing from the package.
    MissingAsset(&'static str),
}

impl std::fmt::Display for CelestialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CelestialError::NotFinite(what) => write!(f, "non-finite {what}"),
            CelestialError::BelowHorizon {
                kind,
                elevation_degrees,
            } => write!(
                f,
                "{kind} is below the horizon ({elevation_degrees:.3} deg)"
            ),
            CelestialError::Hidden { kind, fade } => {
                write!(f, "{kind} is switched off (fade {fade:.3})")
            }
            CelestialError::MissingAsset(path) => write!(f, "missing client asset {path}"),
        }
    }
}

/// One native billboard ready for submission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
    pub kind: DiscKind,
    /// Quad centre, `direction * DISTANCE`.
    pub center: [f32; 3],
    /// Four corners in native submission order.
    pub corners: [[f32; 3]; 4],
    /// Vertex diffuse alpha as the client uploads it (`fade * 255` truncated).
    pub alpha: u8,
    /// Half angle subtended by the quad, in radians.
    pub angular_radius: f32,
}

impl Disc {
    /// Native packed diffuse: opaque RGB with the fade in the alpha byte.
    #[allow(dead_code)]
    pub fn diffuse(&self) -> [u8; 4] {
        [255, 255, 255, self.alpha]
    }
}

/// Half angle subtended by a disc of `HALF_EXTENT` at `DISTANCE`.
pub fn angular_radius() -> f32 {
    (HALF_EXTENT / DISTANCE).atan()
}

/// Gain the native path multiplies the sun's `+0x44` light colour by
/// (`ds:0x8db0f8` = 5.0).
pub const GLOW_GAIN: f32 = 5.0;
/// Reference value the night strength `+0x70` is subtracted from
/// (`ds:0x8d5c28` = 1.0).
pub const GLOW_REFERENCE: f32 = 1.0;
/// Scale applied to the factor while the night flag `+0x74` selects the night
/// set (`ds:0x8d62dc` = 0.8).
pub const GLOW_NIGHT_SCALE: f32 = 0.8;

/// Texture factor the native path hands to the effect's stage-1 `TA_TFACTOR`
/// modulation (`0x4b3e00`–`0x4b3e39`).
///
/// The code tests the strength for unordered (NaN) first and falls back to the
/// literal `1.0` (`mov dword [esp+0x10], 0x3f800000`); otherwise it computes
/// `(1.0 - strength) * 5.0` and, when `[+0x74] == 1.0`, multiplies by `0.8`.
/// The client does not clamp the result: at the 1.75 midnight strength the
/// factor is negative and the hardware saturates it away when the colour is
/// modulated, which is why the glow is invisible while the sun is down.
#[allow(dead_code)]
pub fn glow_texture_factor(night_strength: f32, night_set: bool) -> f32 {
    if !night_strength.is_finite() {
        return 1.0;
    }
    let factor = (GLOW_REFERENCE - night_strength) * GLOW_GAIN;
    if night_set {
        factor * GLOW_NIGHT_SCALE
    } else {
        factor
    }
}

/// Unit direction of a disc from its elevation and the shared azimuth.
pub fn direction(elevation: f32, azimuth: f32) -> [f32; 3] {
    let (sin_e, cos_e) = elevation.sin_cos();
    let (sin_a, cos_a) = azimuth.sin_cos();
    [cos_a * cos_e, sin_e, sin_a * cos_e]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len.is_finite() && len > 1e-6 {
        Some([v[0] / len, v[1] / len, v[2] / len])
    } else {
        None
    }
}

/// Build the native billboard for one object.
///
/// `fade` is the 0..1 ramp from [`crate::client_sun::Visibility`]; `asset_loaded`
/// says whether the native texture is present, so a missing asset fails here
/// instead of rendering an empty quad.
pub fn disc(
    kind: DiscKind,
    state: &SunMoon,
    fade: f32,
    asset_loaded: bool,
) -> Result<Disc, CelestialError> {
    if !fade.is_finite() || !state.azimuth.is_finite() {
        return Err(CelestialError::NotFinite("celestial fade or azimuth"));
    }
    if !asset_loaded {
        return Err(CelestialError::MissingAsset(kind.texture()));
    }
    let elevation = state.elevation(kind == DiscKind::Sun);
    if !elevation.is_finite() {
        return Err(CelestialError::NotFinite("celestial elevation"));
    }
    if elevation <= 0.0 {
        return Err(CelestialError::BelowHorizon {
            kind: kind.name(),
            elevation_degrees: elevation.to_degrees(),
        });
    }
    if fade <= 0.0 {
        return Err(CelestialError::Hidden {
            kind: kind.name(),
            fade,
        });
    }
    let dir = direction(elevation, state.azimuth);
    let center = [dir[0] * DISTANCE, dir[1] * DISTANCE, dir[2] * DISTANCE];
    // Screen-aligned billboard: the native path builds the quad in view space,
    // which for a camera-anchored sky equals a basis perpendicular to the disc
    // direction. The fallback handles an exactly vertical disc.
    let right = normalize(cross([0.0, 1.0, 0.0], dir))
        .or_else(|| normalize(cross([1.0, 0.0, 0.0], dir)))
        .ok_or(CelestialError::NotFinite("celestial basis"))?;
    let up = cross(dir, right);
    let mut corners = [[0.0_f32; 3]; 4];
    for (index, (rs, us)) in [(-1.0_f32, 1.0_f32), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
        .iter()
        .enumerate()
    {
        for axis in 0..3 {
            corners[index][axis] =
                center[axis] + right[axis] * rs * HALF_EXTENT + up[axis] * us * HALF_EXTENT;
        }
    }
    Ok(Disc {
        kind,
        center,
        corners,
        alpha: (fade.clamp(0.0, 1.0) * 255.0).trunc() as u8,
        angular_radius: angular_radius(),
    })
}

/// Both discs of a frame, each with its own explicit failure.
pub fn discs(
    state: &SunMoon,
    sun_fade: f32,
    moon_fade: f32,
    sun_asset: bool,
    moon_asset: bool,
) -> [Result<Disc, CelestialError>; 2] {
    [
        disc(DiscKind::Sun, state, sun_fade, sun_asset),
        disc(DiscKind::Moon, state, moon_fade, moon_asset),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky_time::evaluate;

    fn state(hour: f32) -> SunMoon {
        evaluate(hour, [0.6, -0.4, 0.8]).expect("finite")
    }

    #[test]
    fn disc_size_matches_the_native_quad_at_distance_100() {
        assert!((angular_radius().to_degrees() - 0.572_938).abs() < 1e-5);
        assert!((angular_radius() * 2.0 * DISTANCE - 2.0 * HALF_EXTENT).abs() < 1e-3);
    }

    #[test]
    fn sun_and_moon_are_placed_by_their_own_elevation() {
        let noon = state(12.0);
        let sun = disc(DiscKind::Sun, &noon, 1.0, true).expect("noon sun");
        assert_eq!(sun.kind, DiscKind::Sun);
        // 70 degrees elevation, azimuth from the authored vector.
        let up = sun.center[1] / DISTANCE;
        assert!((up - 70_f32.to_radians().sin()).abs() < 1e-5);
        let horizontal =
            (sun.center[0] * sun.center[0] + sun.center[2] * sun.center[2]).sqrt() / DISTANCE;
        assert!((horizontal - 70_f32.to_radians().cos()).abs() < 1e-5);
        assert_eq!(
            disc(DiscKind::Moon, &noon, 1.0, true),
            Err(CelestialError::BelowHorizon {
                kind: "moon",
                elevation_degrees: 0.0
            })
        );
        let midnight = state(0.0);
        assert!(disc(DiscKind::Moon, &midnight, 1.0, true).is_ok());
        assert!(matches!(
            disc(DiscKind::Sun, &midnight, 1.0, true),
            Err(CelestialError::BelowHorizon { .. })
        ));
    }

    #[test]
    fn quad_is_a_flat_two_triangle_billboard_of_the_recovered_extent() {
        let sun = disc(DiscKind::Sun, &state(12.0), 1.0, true).expect("sun");
        assert_eq!(DISC_INDICES.len(), 6);
        for corner in sun.corners {
            let mut offset = [0.0_f32; 3];
            for axis in 0..3 {
                offset[axis] = corner[axis] - sun.center[axis];
            }
            let distance =
                (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt();
            assert!((distance - std::f32::consts::SQRT_2).abs() < 1e-4);
            // Every corner lies in the tangent plane of the disc direction.
            let dot =
                offset[0] * sun.center[0] + offset[1] * sun.center[1] + offset[2] * sun.center[2];
            assert!(dot.abs() < 1e-3);
        }
    }

    #[test]
    fn visibility_fade_is_uploaded_as_a_truncated_alpha_byte() {
        let sun = disc(DiscKind::Sun, &state(12.0), 0.925, true).expect("sun");
        assert_eq!(sun.alpha, 235);
        assert_eq!(sun.diffuse(), [255, 255, 255, 235]);
        assert_eq!(
            disc(DiscKind::Sun, &state(12.0), 0.0, true),
            Err(CelestialError::Hidden {
                kind: "sun",
                fade: 0.0
            })
        );
    }

    #[test]
    fn glow_texture_factor_follows_the_native_expression() {
        // Daytime: strength 0, day set -> the full 5x gain.
        assert_eq!(glow_texture_factor(0.0, false), 5.0);
        // Same clock with the night set selected -> 80 percent of it.
        assert!((glow_texture_factor(0.0, true) - 4.0).abs() < 1e-6);
        // 18:00: strength 0 -> 5.0; 21:00: strength 1 -> 0.
        assert!(glow_texture_factor(0.5, false) > 0.0);
        assert_eq!(glow_texture_factor(1.0, false), 0.0);
        // Midnight strength 1.75 overdrives it negative; the native code leaves
        // that to the modulator saturating, and so does this function.
        assert!((glow_texture_factor(1.75, false) + 3.75).abs() < 1e-6);
        // A non-finite strength takes the literal 1.0 branch.
        assert_eq!(glow_texture_factor(f32::NAN, true), 1.0);
        assert_eq!(glow_texture_factor(f32::INFINITY, false), 1.0);
    }

    #[test]
    fn unsupported_states_fail_explicitly() {
        assert_eq!(
            disc(DiscKind::Sun, &state(12.0), 1.0, false),
            Err(CelestialError::MissingAsset("textures/nature/sun.bmp"))
        );
        let mut broken = state(12.0);
        broken.sun_elevation = f32::NAN;
        assert_eq!(
            disc(DiscKind::Sun, &broken, 1.0, true),
            Err(CelestialError::NotFinite("celestial elevation"))
        );
        let errors = discs(&state(12.0), 1.0, 1.0, true, true);
        assert!(errors[0].is_ok());
        assert!(errors[1].is_err());
        // The messages are what the viewer shows, so keep them readable.
        assert_eq!(
            errors[1].as_ref().unwrap_err().to_string(),
            "moon is below the horizon (0.000 deg)"
        );
    }
}
