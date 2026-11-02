//! Native environment float/color shading: 0x41f30f (target setup),
//! 0x41fdc3 (integer-ms -> stored seconds), 0x421e80 (advance/clamp).
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    pub value: f32,
    target: f32,
    delta: f32,
}
/// 0x42fc7d caps the logic tick at 250ms; 0x42b7fd passes that tick
/// to AdjustEnvironment, not the uncapped real-time counter at +0x494.
pub fn frame_ms(elapsed: u64) -> u32 {
    elapsed.min(250) as u32
}
pub fn inverse_time(ms: u32) -> f32 {
    if ms == 0 {
        0.
    } else {
        (1000. / ms as f64) as f32
    }
}
pub fn seconds(ms: u32) -> f32 {
    (ms as f64 * f32::from_bits(0x3a83126f) as f64) as f32
}
impl Channel {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            delta: 0.,
        }
    }
    pub fn target(&mut self, target: f32, inverse_time: f32) {
        if inverse_time == 0. {
            *self = Self::new(target);
            return;
        }
        self.target = target;
        // Native game flags 1/5 (0x42fef6) omit FPU_PRESERVE (engine bit 8,
        // 0x7795c6), so D3D8 selects x87 PC24. Even the scalar subtraction
        // rounds before the multiply: an x87 register is NOT proof of f64.
        self.delta = (target - self.value) * inverse_time;
    }
    pub fn shade(&mut self, seconds: f32) {
        if self.delta == 0. || self.value == self.target {
            return;
        }
        let next = self.value + self.delta * seconds;
        // Separate PC24 multiply/add, no fused multiply-add. Clamp each
        // component by its signed step, not by an elapsed-time ratio.
        self.value =
            if (self.delta > 0. && next > self.target) || (self.delta < 0. && next < self.target) {
                self.target
            } else {
                next
            };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_pc24_precision_rounds_subtraction_before_multiplication() {
        let mut scalar = Channel::new(1. / 255.);
        let mut color = scalar;
        scalar.target(4. / 255., inverse_time(5000));
        color.target(4. / 255., inverse_time(5000));
        assert_eq!(scalar.delta.to_bits(), 0x3b1a33cf);
        assert_eq!(color.delta.to_bits(), 0x3b1a33cf);
    }
    #[test]
    fn stored_steps_accumulate_until_crossing_then_clamp_in_both_directions() {
        let mut up = Channel::new(0.1);
        let mut down = Channel::new(0.9);
        up.target(0.9, inverse_time(5000));
        down.target(0.1, inverse_time(5000));
        for _ in 0..312 {
            up.shade(seconds(16));
            down.shade(seconds(16));
        }
        assert_eq!(up.value.to_bits(), 0x3f6612a9);
        assert_eq!(down.value.to_bits(), 0x3dcf6ab4);
        up.shade(seconds(100));
        down.shade(seconds(100));
        assert_eq!(up.value, 0.9);
        assert_eq!(down.value, 0.1);
    }
    #[test]
    fn real_tick_is_capped_before_environment_shading() {
        for (input, expected) in [(0, 0), (16, 16), (250, 250), (5000, 250), (u64::MAX, 250)] {
            assert_eq!(frame_ms(input), expected);
        }
    }
    #[test]
    fn retarget_uses_current_value_and_zero_time_resets_immediately() {
        let mut c = Channel::new(10.);
        c.target(30., inverse_time(5000));
        c.shade(seconds(2500));
        assert_eq!(c.value, 20.);
        c.target(0., inverse_time(1000));
        c.shade(seconds(500));
        assert_eq!(c.value, 10.);
        c.target(7., 0.);
        assert_eq!(c.value, 7.);
        c.shade(seconds(100000));
        assert_eq!(c.value, 7.);
    }
}
