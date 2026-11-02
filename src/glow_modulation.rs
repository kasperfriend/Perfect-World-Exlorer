//! PS c3 binding recovered at 0x411340–0x41145f.
//! Caller ARGB is multiplied by separate brightness/opacity fade channels.
pub fn constant(argb: u32, brightness_fade: f32, opacity_fade: f32) -> [f32; 4] {
    // Finite bounds are viewer safety; ordinary retail transitions stay 0..1.
    let fade = |v: f32| if v.is_finite() { v.clamp(0., 1.) } else { 0. };
    let rgb = 1. - fade(brightness_fade);
    let alpha = 1. - fade(opacity_fade);
    [
        ((argb >> 16) & 255) as f32 / 255. * rgb,
        ((argb >> 8) & 255) as f32 / 255. * rgb,
        (argb & 255) as f32 / 255. * rgb,
        (argb >> 24) as f32 / 255. * alpha,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn white_default_and_independent_rgb_alpha_fades() {
        assert_eq!(constant(0xffffffff, 0., 0.), [1.; 4]);
        assert_eq!(
            constant(0xff804020, 0.5, 0.25),
            [64. / 255., 32. / 255., 16. / 255., 0.75]
        );
        assert_eq!(
            constant(0x80402010, 0., 1.),
            [64. / 255., 32. / 255., 16. / 255., 0.]
        );
        assert_eq!(constant(0x80402010, 1., 0.), [0., 0., 0., 128. / 255.]);
    }
    #[test]
    fn malformed_preview_values_do_not_poison_the_frame() {
        assert_eq!(constant(0xffffffff, f32::NAN, f32::INFINITY), [1.; 4]);
        assert_eq!(constant(0xffffffff, -1., 2.), [1., 1., 1., 0.]);
    }
}
