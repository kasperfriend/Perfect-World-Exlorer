//! Retail grass refraction vertex fog (`grass_wave_refract`, 0x651319).
//! c91 = (1, water_height, 0.075, 0). The shader uses displaced Y for
//! water depth but the original vertex for the direction toward the eye.
pub const DENSITY: f32 = 0.075;
pub fn grass_transmittance(
    original: [f32; 3],
    displaced: [f32; 3],
    eye: [f32; 3],
    water_height: f32,
) -> f32 {
    let delta: [f32; 3] = std::array::from_fn(|i| eye[i] - original[i]);
    let depth = water_height - displaced[1];
    // Coincident/horizontal rays are undefined on retail rsq/rcp hardware.
    // Choose finite limits rather than propagating NaN through rasterization.
    if delta[1].abs() <= 1e-6 {
        return if depth > 0. { 0. } else { 1. };
    }
    let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
    (-DENSITY * depth * distance / delta[1]).exp2()
}
/// Lit-model refraction clamps above-plane vertex depth before exponentiation,
/// unlike the grass shader. The returned factor is still vertex-interpolated.
pub fn bmd_transmittance(world: [f32; 3], eye: [f32; 3], water_height: f32) -> f32 {
    let clamped = [world[0], world[1].min(water_height), world[2]];
    grass_transmittance(world, clamped, eye, water_height)
}
pub const WGSL: &str = r#"
fn grass_refraction_transmittance(original:vec3<f32>,displaced:vec3<f32>,eye:vec3<f32>,water_height:f32)->f32 {
 let delta=eye-original;let depth=water_height-displaced.y;
 if abs(delta.y)<=0.000001 {return select(1.,0.,depth>0.);}
 return exp2(-0.075*depth*length(delta)/delta.y);
}
fn grass_pass_fog(linear:vec3<f32>,reciprocal_w:f32,transmittance:f32)->vec3<f32> {
 if camera.clip.y>=0. || camera.fog_range.z<0.5 {return fogged(linear,reciprocal_w);}
 let c=max(linear,vec3<f32>(0.));
 let encoded=select(c*12.92,1.055*pow(c,vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));
 let v=mix(camera.fog_color.rgb,encoded,clamp(transmittance,0.,1.));
 return select(v/12.92,pow((max(v,vec3<f32>(0.))+0.055)/1.055,vec3<f32>(2.4)),v>vec3<f32>(0.04045));
}
"#;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vertical_ray_uses_only_submerged_length_and_base_two() {
        let f = grass_transmittance([0., -10., 0.], [0., -10., 0.], [0., 10., 0.], 0.);
        assert!((f - 0.59460354).abs() < 1e-6);
        assert!((f - (-0.75f32).exp()).abs() > 0.1);
    }
    #[test]
    fn oblique_ray_is_longer_but_not_full_eye_to_vertex_distance() {
        let f = grass_transmittance([0., -10., 0.], [0., -10., 0.], [10., 10., 0.], 0.);
        assert!((f - (-DENSITY * 10. * 500f32.sqrt() / 20.).exp2()).abs() < 1e-6);
        assert!(f < 0.59460354 && f > (-DENSITY * 500f32.sqrt()).exp2());
    }
    #[test]
    fn displaced_depth_keeps_original_ray_and_no_vertex_clamp() {
        let f = grass_transmittance([0., -10., 0.], [0., -8., 0.], [0., 10., 0.], 0.);
        assert!((f - (-0.6f32).exp2()).abs() < 1e-6);
        assert!(grass_transmittance([0., 1., 0.], [0., 1., 0.], [0., 10., 0.], 0.) > 1.);
    }
    #[test]
    fn translating_camera_geometry_and_water_does_not_change_fog() {
        let a = grass_transmittance([1., -8., 3.], [1., -7., 3.], [10., 10., 5.], 0.);
        let b = grass_transmittance([21., 92., -17.], [21., 93., -17.], [30., 110., -15.], 100.);
        assert_eq!(a, b);
    }
    #[test]
    fn undefined_horizontal_and_coincident_rays_have_finite_limits() {
        assert_eq!(grass_transmittance([0.; 3], [0., -1., 0.], [0.; 3], 0.), 0.);
        assert_eq!(grass_transmittance([0.; 3], [0.; 3], [1., 0., 0.], 0.), 1.);
    }
    #[test]
    fn bmd_clamps_above_plane_depth_before_interpolation() {
        let p = [0., 1., 0.];
        let eye = [0., 10., 0.];
        assert_eq!(bmd_transmittance(p, eye, 0.), 1.);
        assert!(grass_transmittance(p, p, eye, 0.) > 1.);
        let p = [0., -2., 0.];
        assert_eq!(
            bmd_transmittance(p, eye, 0.),
            grass_transmittance(p, p, eye, 0.)
        );
    }
}
