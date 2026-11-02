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
    let n = glam::Vec3::from_array(p.normal).normalize_or_zero();
    let dir = glam::Vec3::from_array(p.direction);
    let lambert = n.dot(-dir).max(0.);
    let delta = glam::Vec3::from_array(p.point_position) - glam::Vec3::from_array(p.world_position);
    let d = delta.length();
    let point_lambert = if d > 1e-6 {
        n.dot(delta / d).max(0.)
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
 let delta=light_pos-pos;let d=length(delta);let l=delta/max(d,0.000001);
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
    fn zero_distance_is_finite() {
        let mut p = input();
        p.point_position = p.world_position;
        for x in vertex_light(p) {
            assert!(x.is_finite());
        }
    }
}
