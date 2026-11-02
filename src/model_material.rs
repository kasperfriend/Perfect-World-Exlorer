//! Non-specular skin/rigid material constants recovered from retail 0x7a6110.
//! Kept separate from baked BMD materials: these coefficients must never be
//! applied to terrain, BMD vertex colors, or the approved tree fallback.
#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub emissive: [f32; 4],
}
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub environment_ambient: [f32; 4],
    pub additional_ambient: [f32; 4],
    pub directional_diffuse: [f32; 4],
    pub point_ambient: [f32; 4],
    pub point_diffuse: [f32; 4],
    /// Mesh transparency, not opacity. Only strictly positive values override c8.a.
    pub transparency: f32,
    pub point_enabled: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constants {
    pub ambient: [f32; 4],
    pub directional_diffuse: [f32; 4],
    pub point_ambient: [f32; 4],
    pub point_diffuse: [f32; 4],
}
/// c7 = material ambient * environment ambient + emissive + model extra ambient.
/// c8 = material diffuse * directional diffuse, with the mesh alpha override.
/// c12 = material ambient * point ambient; c11 = material diffuse * point diffuse,
/// but c11.a is forced to zero (0x7a63af). Do not clamp these constants: the
/// vertex shader adds all light terms before its upper clamp.
pub fn bind(m: Material, l: Lighting) -> Constants {
    let mut c = Constants {
        ambient: std::array::from_fn(|i| {
            m.ambient[i] * l.environment_ambient[i] + m.emissive[i] + l.additional_ambient[i]
        }),
        directional_diffuse: std::array::from_fn(|i| m.diffuse[i] * l.directional_diffuse[i]),
        point_ambient: [0.; 4],
        point_diffuse: [0.; 4],
    };
    if l.transparency > 0. {
        c.directional_diffuse[3] = 1. - l.transparency;
    }
    if l.point_enabled {
        c.point_ambient = std::array::from_fn(|i| m.ambient[i] * l.point_ambient[i]);
        c.point_diffuse = std::array::from_fn(|i| m.diffuse[i] * l.point_diffuse[i]);
        c.point_diffuse[3] = 0.;
    }
    c
}

/// GPU counterpart for model material binding and shader regression tests.
pub const WGSL: &str = r#"
struct ModelMaterialConstants {
 ambient:vec4<f32>, directional_diffuse:vec4<f32>,
 point_ambient:vec4<f32>, point_diffuse:vec4<f32>
};
fn bind_model_material(mat_ambient:vec4<f32>,mat_diffuse:vec4<f32>,emissive:vec4<f32>,env_ambient:vec4<f32>,extra_ambient:vec4<f32>,dir_diffuse:vec4<f32>,point_ambient:vec4<f32>,point_diffuse:vec4<f32>,transparency:f32,point_enabled:bool)->ModelMaterialConstants {
 var c:ModelMaterialConstants;
 c.ambient=mat_ambient*env_ambient+emissive+extra_ambient;
 c.directional_diffuse=mat_diffuse*dir_diffuse;
 if transparency>0. {c.directional_diffuse.a=1.-transparency;}
 c.point_ambient=vec4<f32>(0.);c.point_diffuse=vec4<f32>(0.);
 if point_enabled {
  c.point_ambient=mat_ambient*point_ambient;
  c.point_diffuse=vec4<f32>((mat_diffuse*point_diffuse).rgb,0.);
 }
 return c;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> Material {
        Material {
            ambient: [0.5; 4],
            diffuse: [0.8; 4],
            emissive: [0.1; 4],
        }
    }
    fn lighting() -> Lighting {
        Lighting {
            environment_ambient: [0.2; 4],
            additional_ambient: [0.3; 4],
            directional_diffuse: [0.5; 4],
            point_ambient: [0.4; 4],
            point_diffuse: [0.6; 4],
            transparency: 0.,
            point_enabled: true,
        }
    }
    #[test]
    fn binds_all_channels_without_premature_clamping() {
        let mut m = material();
        m.emissive[0] = 2.;
        let c = bind(m, lighting());
        assert!((c.ambient[0] - 2.4).abs() < 1e-6);
        assert_eq!(c.ambient[1..], [0.5; 3]);
        assert_eq!(c.directional_diffuse, [0.4; 4]);
        assert_eq!(c.point_ambient, [0.2; 4]);
        assert_eq!(c.point_diffuse, [0.8 * 0.6, 0.8 * 0.6, 0.8 * 0.6, 0.]);
    }
    #[test]
    fn transparency_is_strict_positive_override_not_multiplier() {
        let mut l = lighting();
        for t in [-1., 0.] {
            l.transparency = t;
            assert_eq!(bind(material(), l).directional_diffuse[3], 0.4);
        }
        for t in [0.01, 0.5, 1., 1.2] {
            l.transparency = t;
            assert_eq!(bind(material(), l).directional_diffuse[3], 1. - t);
        }
    }
    #[test]
    fn disabling_point_clears_both_constants_without_stale_alpha() {
        let mut l = lighting();
        l.point_enabled = false;
        let c = bind(material(), l);
        assert_eq!(c.point_ambient, [0.; 4]);
        assert_eq!(c.point_diffuse, [0.; 4]);
    }
}
