//! 1.4.2 skin queue classification, 0x7a5857–0x7a58c2. In this version
//! an alpha texture only triggers alpha-shader routing if it is ALSO a shader
//! texture. Related 1.5.2 source removed that condition; do not copy its rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Queue {
    Hidden,
    Opaque,
    Alpha,
}
pub fn classify(
    invisible: bool,
    transparency: f32,
    source_blend: u32,
    dest_blend: u32,
    shader_texture: bool,
    alpha_texture: bool,
) -> Queue {
    if invisible || transparency >= 1. {
        Queue::Hidden
    } else if transparency >= 0.
        || source_blend != 5
        || dest_blend != 6
        || (shader_texture && alpha_texture)
    {
        Queue::Alpha
    } else {
        Queue::Opaque
    }
}
/// Metadata used by the retail alpha-list insertion walk. This is deliberately
/// not a Rust sort comparator: the same-model AABB rule is not a total order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlphaNode {
    pub model: usize,
    pub mesh_weight: i32,
    pub skin_weight: i32,
    pub group: u32,
    pub group_weight: i32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub distance: f32,
}
impl AlphaNode {
    fn valid(self) -> bool {
        self.distance.is_finite()
            && self.distance >= 0.
            && (0..3).all(|i| {
                self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i]
            })
    }
    fn weight(self) -> i32 {
        if self.mesh_weight == 0 {
            self.skin_weight
        } else {
            self.mesh_weight
        }
    }
}
/// Retail 0x7a5460: minimum distance to EIGHT transformed initial-AABB
/// corners, capped at 999999.875. Not distance to its surface, posed bounds,
/// or center: even an eye inside the box can have nonzero sort distance.
/// `world` is column-major and must be affine (as model placements are).
pub fn alpha_corner_distance(
    min: [f32; 3],
    max: [f32; 3],
    world: [[f32; 4]; 4],
    eye: [f32; 3],
) -> Result<f32, String> {
    let bounds = AlphaNode {
        model: 0,
        mesh_weight: 0,
        skin_weight: 0,
        group: 0,
        group_weight: 0,
        min,
        max,
        distance: 0.,
    };
    if !bounds.valid()
        || world
            .iter()
            .flatten()
            .chain(eye.iter())
            .any(|v| !v.is_finite())
        || [world[0][3], world[1][3], world[2][3], world[3][3]] != [0., 0., 0., 1.]
    {
        return Err("invalid alpha distance bounds/affine transform/eye".into());
    }
    let mut nearest = f32::from_bits(0x497423fe);
    for x in [min[0], max[0]] {
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                let mut delta = [0.; 3];
                for i in 0..3 {
                    delta[i] =
                        world[0][i] * x + world[1][i] * y + world[2][i] * z + world[3][i] - eye[i];
                }
                let distance =
                    (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
                if !distance.is_finite() {
                    return Err("alpha distance overflow".into());
                }
                nearest = nearest.min(distance);
            }
        }
    }
    Ok(nearest)
}
/// Source 0x7a5a1d–0x7a5aef. In 1.4.2, AABB comparisons ALSO run when
/// the new same-model weight is greater (not merely equal). Related 1.5.2
/// adds an equality guard; substituting that version changes render order.
pub fn alpha_insert_index(nodes: &[AlphaNode], new: AlphaNode) -> Result<usize, String> {
    if !new.valid() || nodes.iter().any(|n| !n.valid()) {
        return Err("invalid alpha queue bounds/distance".into());
    }
    Ok(nodes
        .iter()
        .position(|old| {
            if old.model == new.model {
                new.weight() < old.weight()
                    || old.max[1] > new.max[1]
                    || old.max[2] > new.max[2]
                    || old.min[2] < new.min[2]
            } else if old.group != 0 && old.group == new.group {
                new.group_weight < old.group_weight
            } else {
                old.distance <= new.distance
            }
        })
        .unwrap_or(nodes.len()))
}
#[cfg(test)]
mod tests {
    use super::*;
    const IDENTITY: [[f32; 4]; 4] = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    #[test]
    fn alpha_distance_uses_initial_corners_not_surface_or_center() {
        let d = alpha_corner_distance([-1.; 3], [1.; 3], IDENTITY, [0.; 3]).unwrap();
        assert!((d - 3f32.sqrt()).abs() < 1e-6);
        assert_eq!(
            alpha_corner_distance([-1.; 3], [1.; 3], IDENTITY, [1.; 3]).unwrap(),
            0.
        );
    }
    #[test]
    fn alpha_corner_distance_applies_rotation_scale_and_translation() {
        let world = [
            [2., 0., 0., 0.],
            [0., 3., 0., 0.],
            [0., 0., 4., 0.],
            [10., 20., 30., 1.],
        ];
        assert_eq!(
            alpha_corner_distance([-1.; 3], [1.; 3], world, [13., 23., 34.]).unwrap(),
            1.
        );
        let rotation = [
            [0., 1., 0., 0.],
            [-1., 0., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        assert_eq!(
            alpha_corner_distance([0.; 3], [1., 2., 0.], rotation, [-2., 1., 0.]).unwrap(),
            0.
        );
        assert_eq!(
            alpha_corner_distance([0.; 3], [0.; 3], IDENTITY, [2_000_000., 0., 0.]).unwrap(),
            999999.875
        );
    }
    #[test]
    fn alpha_corner_distance_rejects_nonaffine_nonfinite_and_overflow() {
        let mut bad = IDENTITY;
        bad[0][3] = 1.;
        assert!(alpha_corner_distance([0.; 3], [1.; 3], bad, [0.; 3]).is_err());
        assert!(alpha_corner_distance([0.; 3], [1.; 3], IDENTITY, [f32::NAN; 3]).is_err());
        assert!(alpha_corner_distance([0.; 3], [1.; 3], IDENTITY, [f32::MAX; 3]).is_err());
        assert!(alpha_corner_distance([2.; 3], [1.; 3], IDENTITY, [0.; 3]).is_err());
    }
    fn node(model: usize, distance: f32) -> AlphaNode {
        AlphaNode {
            model,
            mesh_weight: 0,
            skin_weight: 0,
            group: 0,
            group_weight: 0,
            min: [0.; 3],
            max: [1.; 3],
            distance,
        }
    }
    #[test]
    fn alpha_distance_is_far_first_with_new_equal_before_old() {
        let nodes = [node(1, 10.), node(2, 5.)];
        assert_eq!(alpha_insert_index(&nodes, node(3, 20.)).unwrap(), 0);
        assert_eq!(alpha_insert_index(&nodes, node(3, 10.)).unwrap(), 0);
        assert_eq!(alpha_insert_index(&nodes, node(3, 7.)).unwrap(), 1);
        assert_eq!(alpha_insert_index(&nodes, node(3, 1.)).unwrap(), 2);
    }
    #[test]
    fn same_model_142_aabb_rule_is_not_152_weight_sort() {
        let mut old = node(1, 10.);
        old.skin_weight = 4;
        let mut new = old;
        new.mesh_weight = 3;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
        new.mesh_weight = 5;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 1);
        new.max[1] = 0.5; // Greater weight STILL permits AABB insertion in 1.4.2.
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
        new = old;
        new.min[2] = 0.5;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
        new = old;
        new.max[2] = 0.5;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
    }
    #[test]
    fn group_weights_override_distance_but_zero_group_does_not() {
        let mut old = node(1, 1.);
        old.group = 7;
        old.group_weight = 4;
        let mut new = node(2, 100.);
        new.group = 7;
        new.group_weight = 4;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 1);
        new.group_weight = 3;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
        new.group = 0;
        assert_eq!(alpha_insert_index(&[old], new).unwrap(), 0);
    }
    #[test]
    fn alpha_insertion_rejects_invalid_input_before_mutation() {
        for bad in [f32::NAN, f32::INFINITY, -1.] {
            let mut n = node(1, 1.);
            n.distance = bad;
            assert!(alpha_insert_index(&[], n).is_err());
            assert!(alpha_insert_index(&[n], node(2, 1.)).is_err());
        }
        let mut n = node(1, 1.);
        n.min[1] = 2.;
        assert!(alpha_insert_index(&[], n).is_err());
    }
    #[test]
    fn ordinary_graded_alpha_dds_is_not_an_alpha_shader() {
        assert_eq!(classify(false, -1., 5, 6, false, true), Queue::Opaque);
        assert_eq!(classify(false, -1., 5, 6, true, false), Queue::Opaque);
        assert_eq!(classify(false, -1., 5, 6, true, true), Queue::Alpha);
    }
    #[test]
    fn zero_transparency_still_routes_alpha_and_hidden_wins() {
        assert_eq!(classify(false, 0., 5, 6, false, false), Queue::Alpha);
        assert_eq!(classify(false, 0.5, 5, 6, false, false), Queue::Alpha);
        assert_eq!(classify(false, 1., 1, 1, true, true), Queue::Hidden);
        assert_eq!(classify(true, -1., 5, 6, false, false), Queue::Hidden);
    }
    #[test]
    fn nondefault_blend_routes_alpha_but_does_not_change_texture_flags() {
        assert_eq!(classify(false, -1., 1, 6, false, false), Queue::Alpha);
        assert_eq!(classify(false, -1., 5, 1, false, false), Queue::Alpha);
    }
}
