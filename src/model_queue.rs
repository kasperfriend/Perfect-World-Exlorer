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
#[cfg(test)]
mod tests {
    use super::*;
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
