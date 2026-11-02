//! Dynamic point + directional diffuse used by retail skin_mesh_m4_ns_dl and
//! rigid_mesh_ns_dl vertex shaders. This module is not yet wired to models.
//! The four-input coefficients correspond to VS constants c7/c8/c10..c13.
#[derive(Clone, Copy, Debug)]
pub struct LightInput {
    pub world_position: [f32; 3],
    pub normal: [f32; 3],
    pub direction: [f32; 3],
    pub point_position: [f32; 3],
    pub attenuation: [f32; 3],
    pub other_ambient: [f32; 3],
    pub directional_diffuse: [f32; 3],
    pub point_diffuse: [f32; 3],
    pub point_ambient: [f32; 3],
    pub point_enabled: bool,
}
/// Scalar reference; the client's `_dl` shader has no range cutoff operation.
/// Zero-distance and zero-denominator are undefined on the original shader;
/// here we choose finite conservative values to avoid poisoning a frame.
pub fn vertex_light(p: LightInput) -> [f32; 3] {
    // Keep the finite fallback identical to WGSL. Retail rsq(0) behavior is
    // hardware-dependent; this guard is viewer safety, not recovered semantics.
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let length = dot(p.normal, p.normal).sqrt();
    let n = if length > 1e-6 {
        p.normal.map(|v| v / length)
    } else {
        [0.; 3]
    };
    let lambert = dot(n, p.direction.map(|v| -v)).max(0.);
    let delta = std::array::from_fn(|i| p.point_position[i] - p.world_position[i]);
    let d = dot(delta, delta).sqrt();
    let point_lambert = if d > 1e-6 {
        dot(n, delta.map(|v| v / d)).max(0.)
    } else {
        0.
    };
    let a = p.attenuation;
    let denom = a[0] + a[1] * d + a[2] * d * d;
    let attenuation = if p.point_enabled && denom > 1e-6 {
        1. / denom
    } else {
        0.
    };
    std::array::from_fn(|i| {
        (p.other_ambient[i]
            + lambert * p.directional_diffuse[i]
            + attenuation * (p.point_ambient[i] + point_lambert * p.point_diffuse[i]))
            .min(1.)
    })
}
/// Reusable WGSL equivalent, ready for the animated skin vertex path.
pub const WGSL: &str = r#"
fn model_vertex_light(pos:vec3<f32>, normal:vec3<f32>, direction:vec3<f32>, light_pos:vec3<f32>, att:vec3<f32>, ambient:vec3<f32>, dir_diff:vec3<f32>, point_diff:vec3<f32>, point_ambient:vec3<f32>, enabled:f32)->vec3<f32> {
 let nl=length(normal);let n=select(vec3<f32>(0.),normal/max(nl,0.000001),nl>0.000001);
 let delta=light_pos-pos;let d=length(delta);let l=select(vec3<f32>(0.),delta/max(d,0.000001),d>0.000001);
 let diffuse=max(dot(n,-direction),0.);let point=max(dot(n,l),0.);
 let denom=dot(att,vec3<f32>(1.,d,d*d));let factor=select(0.,1./max(denom,0.000001),enabled>0.5 && denom>0.000001);
 return min(ambient+dir_diff*diffuse+(point_ambient+point_diff*point)*factor,vec3<f32>(1.));
}
"#;
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> LightInput {
        LightInput {
            world_position: [0., 0., 0.],
            normal: [0., 1., 0.],
            direction: [0., -1., 0.],
            point_position: [0., 10., 0.],
            attenuation: [0., 0.4, 0.],
            other_ambient: [0.1, 0.2, 0.3],
            directional_diffuse: [0.2, 0.1, 0.],
            point_diffuse: [0.4, 0.2, 0.1],
            point_ambient: [0.04, 0.08, 0.12],
            point_enabled: true,
        }
    }
    #[test]
    fn point_directional_and_ambient_are_added_before_clamp() {
        let c = vertex_light(input());
        for (a, b) in c.into_iter().zip([0.41, 0.37, 0.355]) {
            assert!((a - b).abs() < 0.00001, "{a} {b}");
        }
    }
    #[test]
    fn no_point_and_reversed_normal() {
        let mut p = input();
        p.point_enabled = false;
        assert!((vertex_light(p)[0] - 0.3).abs() < 1e-6);
        p.normal = [0., -1., 0.];
        assert!((vertex_light(p)[0] - 0.1).abs() < 1e-6);
    }
    #[test]
    fn degenerate_normal_and_point_guards_match_shader_thresholds() {
        let mut p = input();
        p.point_enabled = false;
        for y in [0., 1e-7, 1e-6] {
            p.normal = [0., y, 0.];
            assert_eq!(vertex_light(p), p.other_ambient);
        }
        p = input();
        p.attenuation = [1., 0., 0.];
        let base = [0.34, 0.38, 0.42]; // other + directional + point ambient
        for y in [0., 1e-7, 1e-6] {
            p.point_position = [0., y, 0.];
            for (a, b) in vertex_light(p).into_iter().zip(base) {
                assert!((a - b).abs() < 1e-6);
            }
        }
        p.point_position = [0., 2e-6, 0.];
        assert!(vertex_light(p)[0] > base[0]);
    }
    #[test]
    fn nonpositive_denominator_disables_the_point_term() {
        let mut p = input();
        for constant in [-1., 0., 1e-7, 1e-6] {
            p.attenuation = [constant, 0., 0.];
            let actual = vertex_light(p);
            p.point_enabled = false;
            assert_eq!(actual, vertex_light(p));
            p.point_enabled = true;
        }
    }
    #[test]
    fn zero_distance_is_finite() {
        let mut p = input();
        p.point_position = p.world_position;
        for x in vertex_light(p) {
            assert!(x.is_finite());
        }
    }
}
