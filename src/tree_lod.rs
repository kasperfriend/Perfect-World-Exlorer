//! Stable cross-fades between the three client-runtime geometry snapshots.
//! The 10% transition widths are an approximation, not a SpeedTreeRT fade port.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub primary: usize,
    pub secondary: Option<usize>,
    pub fraction: f32,
}
pub fn select(distance: f32, height: f32, scale: f32) -> Selection {
    let h = height.max(5.);
    let s = scale.clamp(0.25, 4.);
    let a = h * 5. * s;
    let b = h * 16. * s;
    for (i, boundary) in [(0, a), (1, b)] {
        let start = boundary * 0.9;
        let end = boundary * 1.1;
        if distance >= start && distance < end {
            return Selection {
                primary: i,
                secondary: Some(i + 1),
                fraction: ((distance - start) / (end - start)).clamp(0., 1.),
            };
        }
    }
    Selection {
        primary: if distance < a {
            0
        } else if distance < b {
            1
        } else {
            2
        },
        secondary: None,
        fraction: 0.,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_are_bounded_and_continuous() {
        for (b, i) in [(50., 0), (160., 1)] {
            let before = select(b * 0.899, 10., 1.);
            let start = select(b * 0.9, 10., 1.);
            let half = select(b, 10., 1.);
            let end = select(b * 1.1, 10., 1.);
            assert_eq!(before.primary, i);
            assert_eq!(before.secondary, None);
            assert_eq!(start.secondary, Some(i + 1));
            assert!(start.fraction.abs() < 1e-5);
            assert!((half.fraction - 0.5).abs() < 1e-5);
            assert_eq!(end.primary, i + 1);
            assert_eq!(end.secondary, None);
        }
    }
    #[test]
    fn distance_and_scale_change_selection() {
        assert_eq!(select(40., 10., 1.).primary, 0);
        assert_eq!(select(100., 10., 1.).primary, 1);
        assert_eq!(select(200., 10., 1.).primary, 2);
        assert_eq!(select(200., 10., 2.).primary, 1);
    }
}
