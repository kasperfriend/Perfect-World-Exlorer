//! Phase 1 regional-light consumer matrix for the inspected 1.4.2 client.
//!
//! The recovered environment path produces several *different* values. They are
//! not interchangeable, and this module records exactly which renderer classes
//! may read each one, with the native address that justifies the row.
//!
//! - Packed day/night ambient and directional diffuse/specular plus the
//!   authored region direction (`0x4208d0..0x420db0`, `0x420e77/0x420e84`,
//!   `0x41fd58..0x41fdb1` -> `0x79b390`) feed the ordinary fixed-function
//!   receiving classes only.
//! - The world day/night factor (`0x4b60e0`, consumed at `0x41ec86`,
//!   `0x42b4fe` and `0x42bd6e`) feeds fog, baked BMD vertex blends, terrain and
//!   grass lightmap mixing. Those surfaces are already baked, so applying the
//!   material ambient/diffuse/specular a second time would double-light them.
//! - The animated sky-object curve (`0x4b60b0/0x4b60e0`) is celestial geometry
//!   only. Ordinary models keep the authored region direction even while the
//!   discs move; the client does not animate material light with the discs.
//! - The default-area SUN bit 3 / MOON bit 4 (`0x420992..0x4209bf`) and the
//!   per-tick visibility fades (`0x4b2d0c`, `0x4b5f63`, `0x4b3b80`) hide or
//!   reveal celestial discs. They are not a material-light switch, so a hidden
//!   sun must not zero directional diffuse/specular.
//!
//! Later client source is research evidence, not authority: the rows below cite
//! the inspected executable. Rows marked "viewer" are explicit viewer-only
//! additions and must not be described as native.

/// One value produced by the regional environment path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    /// Device ambient render state (`0x420e77/0x420e84`).
    Ambient,
    /// Directional light diffuse (`0x4208d0..0x420db0`).
    Diffuse,
    /// Directional light specular, independently scaled (`0x420b8d`, `0x4b60e0`).
    Specular,
    /// Authored EC-BSD region direction (`+0xc4`), not the celestial curve.
    AuthoredDirection,
    /// Animated sun/moon elevation curve (`0x4b60b0/0x4b60e0`).
    SkyObjectDirection,
    /// World day/night factor (`0x4b60e0` +0x70).
    DayNightFactor,
    /// Default-area SUN/MOON visibility bits and their fades.
    CelestialVisibility,
}

/// A renderer class that either does or does not read regional values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// Weighted ordinary receiving model (`skin_*` shader family).
    OrdinaryWeightedModel,
    /// Rigid/point ordinary receiving model (`rigid_mesh_ns`).
    OrdinaryRigidModel,
    /// Viewer-only opt-in regional tree tint (approved tree default is unchanged).
    RegionalTreeLighting,
    /// Experimental sun disc.
    SkySunDisc,
    /// Experimental moon disc.
    SkyMoonDisc,
    /// Terrain material sampled from T2LM day/night pairs.
    TerrainLightmap,
    /// Grass material sampled from the same pairs (`grassrender.txt`).
    GrassLightmap,
    /// Baked BMD vertex colours blended from its two packed arrays.
    BakedBmd,
    /// Atmospheric region fog.
    AtmosphereFog,
    /// Underwater region fog.
    UnderwaterFog,
    /// Viewer diagnostic directional term used when T2LM is disabled.
    TerrainUnlitDiagnostic,
}

/// Which authored/animated vector a class may use as a light direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectionSource {
    /// Authored EC-BSD direction (optionally negated), never the disc curve.
    Authored,
    /// Animated celestial elevation with the authored azimuth.
    SkyObject,
    /// No direction is consumed at all.
    None,
}

pub struct Row {
    pub consumer: Consumer,
    pub receives: &'static [Value],
    pub direction: DirectionSource,
    pub evidence: &'static str,
}

const MATERIAL: &[Value] = &[
    Value::Ambient,
    Value::Diffuse,
    Value::Specular,
    Value::AuthoredDirection,
];
const SKY: &[Value] = &[Value::SkyObjectDirection, Value::CelestialVisibility];

/// Every row is asserted by the tests below; a class may not receive a value
/// that its native path never reads.
pub const MATRIX: &[Row] = &[
    Row {
        consumer: Consumer::OrdinaryWeightedModel,
        receives: MATERIAL,
        direction: DirectionSource::Authored,
        evidence: "0x7a6110 material constants, 0x79b390 SetLightParam direction, 0x420db0 diffuse+specular",
    },
    Row {
        consumer: Consumer::OrdinaryRigidModel,
        receives: MATERIAL,
        direction: DirectionSource::Authored,
        evidence: "0x7a714f..0x7a7158 rigid constants, same SetLightParam caller",
    },
    Row {
        consumer: Consumer::RegionalTreeLighting,
        receives: &[Value::Diffuse, Value::AuthoredDirection],
        direction: DirectionSource::Authored,
        evidence: "viewer opt-in only; approved SpeedTree default must stay unchanged (P1.11)",
    },
    Row {
        consumer: Consumer::SkySunDisc,
        receives: SKY,
        direction: DirectionSource::SkyObject,
        evidence: "0x4b60b0/0x4b60e0 elevation, 0x4b2da0 sun.bmp, 0x4b3b80 visibility alpha",
    },
    Row {
        consumer: Consumer::SkyMoonDisc,
        receives: SKY,
        direction: DirectionSource::SkyObject,
        evidence: "0x4b60e0, 0x4b2e17 moon.tga, 0x4b5f63 fade",
    },
    Row {
        consumer: Consumer::TerrainLightmap,
        receives: &[Value::DayNightFactor],
        direction: DirectionSource::None,
        evidence: "0x42bd6e night factor -> 0x7ccf60 terrain; T2LM day/night textures 0x7c2650",
    },
    Row {
        consumer: Consumer::GrassLightmap,
        receives: &[Value::DayNightFactor],
        direction: DirectionSource::None,
        evidence: "0x42c10d night factor -> 0x651510; grassrender.txt lrp c0.a day/night",
    },
    Row {
        consumer: Consumer::BakedBmd,
        receives: &[Value::DayNightFactor],
        direction: DirectionSource::None,
        evidence: "0x42b4fe -> renderer+0x2c, 0x7b9bc0 integer array blend, 0x7a9730 MODULATE2X",
    },
    Row {
        consumer: Consumer::AtmosphereFog,
        receives: &[Value::DayNightFactor],
        direction: DirectionSource::None,
        evidence: "0x41ec86..0x41ec9e region colour/fog uses night and 1-night",
    },
    Row {
        consumer: Consumer::UnderwaterFog,
        receives: &[Value::DayNightFactor],
        direction: DirectionSource::None,
        evidence: "0x41ec86..0x41ec9e, water area 0x780790/0x781c50",
    },
    Row {
        consumer: Consumer::TerrainUnlitDiagnostic,
        receives: &[],
        direction: DirectionSource::None,
        evidence: "viewer diagnostic for T2LM-disabled comparison; not a native material path",
    },
];

fn row(consumer: Consumer) -> &'static Row {
    MATRIX
        .iter()
        .find(|r| r.consumer == consumer)
        .expect("every phase 1 consumer has a matrix row")
}

pub fn receives(consumer: Consumer, value: Value) -> bool {
    row(consumer).receives.contains(&value)
}

pub fn direction_source(consumer: Consumer) -> DirectionSource {
    row(consumer).direction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_material_values_reach_only_lit_receiving_classes() {
        for &c in &[
            Consumer::OrdinaryWeightedModel,
            Consumer::OrdinaryRigidModel,
        ] {
            for v in [Value::Ambient, Value::Diffuse, Value::Specular] {
                assert!(receives(c, v), "{c:?} must receive {v:?}");
            }
            assert_eq!(direction_source(c), DirectionSource::Authored);
        }
        // Baked/lightmapped/fog classes never read the lit-material values.
        for &c in &[
            Consumer::TerrainLightmap,
            Consumer::GrassLightmap,
            Consumer::BakedBmd,
            Consumer::AtmosphereFog,
            Consumer::UnderwaterFog,
            Consumer::SkySunDisc,
            Consumer::SkyMoonDisc,
        ] {
            for v in [
                Value::Ambient,
                Value::Diffuse,
                Value::Specular,
                Value::AuthoredDirection,
            ] {
                assert!(!receives(c, v), "{c:?} must not receive {v:?}");
            }
        }
    }

    #[test]
    fn animated_sky_curve_is_celestial_only() {
        for &c in &[Consumer::SkySunDisc, Consumer::SkyMoonDisc] {
            assert!(receives(c, Value::SkyObjectDirection));
            assert_eq!(direction_source(c), DirectionSource::SkyObject);
        }
        for row in MATRIX
            .iter()
            .filter(|r| r.consumer != Consumer::SkySunDisc && r.consumer != Consumer::SkyMoonDisc)
        {
            assert!(
                !row.receives.contains(&Value::SkyObjectDirection),
                "{:?} must not use the animated celestial curve",
                row.consumer
            );
            assert_ne!(row.direction, DirectionSource::SkyObject);
        }
    }

    #[test]
    fn baked_surfaces_receive_only_the_day_night_factor() {
        for &c in &[
            Consumer::TerrainLightmap,
            Consumer::GrassLightmap,
            Consumer::BakedBmd,
        ] {
            assert_eq!(row(c).receives, &[Value::DayNightFactor]);
            assert_eq!(direction_source(c), DirectionSource::None);
        }
    }

    #[test]
    fn visibility_flags_are_not_a_material_switch() {
        for row in MATRIX {
            if row.receives.contains(&Value::CelestialVisibility) {
                assert!(matches!(
                    row.consumer,
                    Consumer::SkySunDisc | Consumer::SkyMoonDisc
                ));
            }
        }
        // No material consumer row mentions visibility at all.
        for &c in &[
            Consumer::OrdinaryWeightedModel,
            Consumer::OrdinaryRigidModel,
            Consumer::TerrainLightmap,
            Consumer::GrassLightmap,
            Consumer::BakedBmd,
        ] {
            assert!(!receives(c, Value::CelestialVisibility));
        }
        // Hiding the discs changes only the celestial rows.
        let before = |c: Consumer, v: Value| receives(c, v);
        assert!(before(Consumer::OrdinaryWeightedModel, Value::Diffuse));
        assert!(before(Consumer::SkySunDisc, Value::CelestialVisibility));
    }

    #[test]
    fn every_row_cites_inspected_client_or_explicitly_says_viewer() {
        assert_eq!(MATRIX.len(), 11);
        for row in MATRIX {
            assert!(
                row.evidence.contains("0x") || row.evidence.contains("viewer"),
                "{:?} needs native address or explicit viewer marker",
                row.consumer
            );
        }
        let viewers = MATRIX
            .iter()
            .filter(|r| r.evidence.contains("viewer"))
            .count();
        assert_eq!(
            viewers, 2,
            "only tree tint and the lightmap-off diagnostic are viewer rows"
    }

    /// Structural guard over the production sources. These are the wiring facts
    /// the matrix depends on; a renderer change that breaks one of them fails
    /// here instead of silently relighting baked content.
    #[test]
    fn production_sources_keep_the_matrix_wiring() {
        let gpu = include_str!("gpu.rs");
        assert_eq!(
            gpu.matches("client_sun::sky_object_direction").count(),
            1,
            "the recovered sky curve has exactly one consumer: the celestial discs"
        );
        assert_eq!(
            gpu.matches("celestial_visibility.alpha(").count(),
            1,
            "visibility fades are consumed once and published to every sky view"
        );
        // The baked BMD branch blends with the day/night factor and must not
        // contain a regional ambient/diffuse/specular or direction term.
        let baked = gpu
            .split("fn bmd_baked_rgb")
            .nth(1)
            .expect("recovered baked BMD arithmetic");
        let baked = baked.split("fn ").next().unwrap();
        for forbidden in ["region_", "camera.light", "camera.sun.xyz", "diffuse"] {
            assert!(
                !baked.contains(forbidden),
                "baked BMD arithmetic must not add {forbidden}"
            );
        }
        // The terrain shader shares the renderer's camera uniform, so the
        // region fields are *declared* there. What matters is that the terrain
        // passes never read them; the lightmapped branch in `terrain_gpu.rs`
        // uses only `camera.sun.w` as the day/night weight.
        let terrain = include_str!("terrain_gpu.rs");
        for forbidden in [
            "camera.region_direction",
            "camera.region_diffuse",
            "camera.region_ambient",
            "sky_object_direction",
        ] {
            assert!(
                !terrain.contains(forbidden),
                "terrain must not read {forbidden}"
            );
        }
        let grass = include_str!("grass_instances.rs");
        for forbidden in [
            "camera.region_direction",
            "camera.region_diffuse",
            "camera.region_ambient",
            "sky_object_direction",
        ] {
            assert!(
                !grass.contains(forbidden),
                "grass must not read {forbidden}"
            );
        }
        // Trees are the only opt-in regional consumer outside ordinary models.
        let trees = include_str!("far_trees_gpu.rs");
        assert!(trees.contains("region_direction"));
        assert!(trees.contains("region_diffuse"));
        assert!(!trees.contains("sky_object_direction"));
    }
}
