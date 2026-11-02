//! Recovered sky-object direction curve from elementclient 0x4b60b0/0x4b60e0.
//! This is a research primitive, not yet wired to world/material lighting:
//! the consumers and correspondence with animated sky textures remain unverified.
#[allow(dead_code)]
pub fn sky_object_direction(hour: f32, authored_direction: [f32; 3]) -> Option<[f32; 3]> {
    if !hour.is_finite() || !authored_direction.iter().all(|x| x.is_finite()) {
        return None;
    }
    let x = authored_direction[0];
    let z = authored_direction[2];
    if x.hypot(z) < 1e-6 {
        return None;
    }
    let azimuth = (-z).atan2(-x);
    let t = hour.rem_euclid(24.) / 24.;
    let elevation = if t < f32::from_bits(0x3e155555) {
        (1. - t * f32::from_bits(0x40db6db7)) * f32::from_bits(0x3f860a92)
    } else if t < 0.5 {
        (t - f32::from_bits(0x3e155555)) * f32::from_bits(0x405cc61e)
    } else if t < 0.875 {
        (1. - (t - 0.5) * f32::from_bits(0x402aaaab)) * f32::from_bits(0x3f9c61aa)
    } else {
        (t - 0.875) * f32::from_bits(0x41060a92)
    };
    let (s, c) = elevation.sin_cos();
    let (az_s, az_c) = azimuth.sin_cos();
    Some([-az_c * c, -s, -az_s * c])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_elevation_curve_and_direction_are_periodic() {
        let base = [0.6, -0.4, 0.8];
        for (h, angle) in [(0., 60_f32), (3.5, 0.), (12., 70.), (21., 0.), (24., 60.)] {
            let v = sky_object_direction(h, base).unwrap();
            assert!(
                (v[1] + angle.to_radians().sin()).abs() < 0.00001,
                "hour {h}: {v:?}"
            );
            assert!(((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.).abs() < 0.00001);
            assert!(v[0] >= -1e-6 && v[2] >= -1e-6);
        }
        assert!(sky_object_direction(1., [0., 1., 0.]).is_none());
        assert!(sky_object_direction(f32::NAN, base).is_none());
    }
}
