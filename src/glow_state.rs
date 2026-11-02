//! Retail full-glow presenter state, independently testable without a GPU.
//! Setters 0x411100/130/160/1a0, Update 0x4111f0, SetGlowType 0x410bb0.
//! Milliseconds and stored f32 steps matter: elapsed-ratio lerp is not equivalent.
#[derive(Default, Debug, Clone)]
struct Channel {
    value: f32,
    target: f32,
    step: f32,
    remaining: i32,
}
impl Channel {
    fn set(&mut self, start: f32, end: f32, ms: i32) {
        self.value = start;
        self.target = end;
        // Native x87 subtract/divide, then one f32 store. In particular zero
        // duration leaves start active (remaining==0), not an instant endpoint.
        self.step = ((end as f64 - start as f64) / ms as f64) as f32;
        self.remaining = ms;
    }
    fn continue_to(&mut self, end: f32, ms: i32) {
        self.set(self.value, end, ms);
    }
    fn tick(&mut self, ms: i32) {
        if self.remaining == 0 {
            return;
        }
        self.remaining = self.remaining.wrapping_sub(ms);
        if self.remaining > 0 {
            // fild(int); fmul(stored step); fadd(stored value); fstp(float).
            self.value = (ms as f64 * self.step as f64 + self.value as f64) as f32;
        } else {
            self.value = self.target;
            self.remaining = 0;
        }
    }
}
#[derive(Default, Debug)]
pub struct WorldGlow {
    enabled: bool,
    mode: u8,
    color: u32,
    power: Channel,
    level: Channel,
    mono: Channel,
    fade: Channel,
    mono_color: [f32; 3],
}
impl WorldGlow {
    /// SetGlowType checks BOTH mode and caller ARGB for changes. Disabled
    /// updates freeze presenter state rather than clearing it (0x40fcc0).
    pub fn select(
        &mut self,
        enabled: bool,
        daylight: f32,
        underwater: bool,
        override_mode: u8,
        color: u32,
    ) {
        self.enabled = enabled;
        if !enabled {
            return;
        }
        let mode = if matches!(override_mode, 4 | 5) {
            override_mode
        } else if underwater {
            3
        } else if 1. - daylight < 0.5 {
            1
        } else {
            2
        };
        if self.mode == mode && self.color == color {
            return;
        }
        self.mode = mode;
        self.color = color;
        // Modes 1/2/3/5 call StopFade(true); mode 4 intentionally does not.
        if mode != 4 {
            self.stop_fade(true);
        }
        match mode {
            1 | 2 | 5 => {
                self.mono.continue_to(0., 5000);
                let (power, level, time) = match mode {
                    1 => (0.25, 0.25, 5000),
                    2 => (0.35, 0.35, 5000),
                    _ => (0.1, 0.25, 1000),
                };
                self.power.continue_to(power, time);
                self.level.continue_to(level, time);
            }
            3 | 4 => {
                let (power, level, mono, time, mono_time, color) = if mode == 3 {
                    (
                        0.5,
                        0.6,
                        0.5,
                        2000,
                        1500,
                        [187. / 255., 234. / 255., 187. / 255.],
                    )
                } else {
                    (1., 0.7, 1., 2000, 2000, [226. / 255., 1., 1.])
                };
                self.power.continue_to(power, time);
                self.level.continue_to(level, time);
                self.mono.set(0., mono, mono_time);
                self.mono_color = color;
            }
            _ => unreachable!(),
        }
    }
    /// FullGlowRender::SetFade is guarded by enabled/capability just like
    /// SetGlowType. No fabricated gameplay trigger is introduced here.
    pub fn set_fade(&mut self, start: f32, end: f32, ms: i32) {
        if self.enabled {
            self.fade.set(start, end, ms);
        }
    }
    pub fn stop_fade(&mut self, to_white: bool) {
        self.fade.continue_to(if to_white { 0. } else { 1. }, 1000);
    }
    pub fn brightness_fade(&self) -> f32 {
        self.fade.value
    }
    pub fn tick(&mut self, ms: i32) {
        if !self.enabled {
            return;
        }
        self.fade.tick(ms);
        self.mono.tick(ms);
        self.power.tick(ms);
        self.level.tick(ms);
    }
    pub fn parameters(&self) -> [f32; 6] {
        if !self.enabled {
            return [0.; 6];
        }
        [
            // Native PS c0.r = level (+0x10), c0.g = power (+0).
            self.level.value,
            self.power.value,
            self.mono_color[0],
            self.mono_color[1],
            self.mono_color[2],
            self.mono.value,
        ]
    }
    pub fn update(&mut self, enabled: bool, daylight: f32, underwater: bool, dt: f32) -> [f32; 6] {
        self.update_with_mode(enabled, daylight, underwater, 0, dt)
    }
    /// Seconds adapter for existing callers/tests. The viewer uses integer
    /// monotonic millisecond differences directly, preserving sub-ms carry.
    pub fn update_with_mode(
        &mut self,
        enabled: bool,
        daylight: f32,
        underwater: bool,
        override_mode: u8,
        dt: f32,
    ) -> [f32; 6] {
        self.select(enabled, daylight, underwater, override_mode, 0xffffffff);
        let ms = if dt.is_finite() {
            (dt.max(0.) as f64 * 1000.).min(i32::MAX as f64) as i32
        } else {
            0
        };
        self.tick(ms);
        self.parameters()
    }
}
#[cfg(test)]
mod glow_tests {
    use super::WorldGlow;
    #[test]
    fn special_modes_use_distinct_targets_tints_and_transition_durations() {
        let mut g = WorldGlow::default();
        let a = g.update_with_mode(true, 1., false, 4, 1.);
        assert_eq!(&a[..2], &[0.35, 0.5]);
        assert_eq!(&a[2..5], &[226. / 255., 1., 1.]);
        assert_eq!(a[5], 0.5);
        let b = g.update_with_mode(true, 1., false, 4, 1.);
        assert_eq!(&b[..2], &[0.7, 1.]);
        assert_eq!(b[5], 1.);
        let c = g.update_with_mode(true, 1., false, 5, 0.5);
        assert!((c[0] - 0.475).abs() < 1e-6 && (c[1] - 0.55).abs() < 1e-6);
        assert!((c[5] - 0.9).abs() < 1e-6);
        let d = g.update_with_mode(true, 1., false, 5, 0.5);
        assert!(
            (d[0] - 0.25).abs() < 1e-6 && (d[1] - 0.1).abs() < 1e-6 && (d[5] - 0.8).abs() < 1e-6
        );
        assert_eq!(g.update_with_mode(false, 1., false, 4, 1.), [0.; 6]);
    }
    #[test]
    fn normal_modes_transition_from_current_level() {
        let mut g = WorldGlow::default();
        assert_eq!(&g.update(true, 1., false, 0.)[..2], &[0., 0.]);
        assert_eq!(&g.update(true, 1., false, 5.)[..2], &[0.25, 0.25]);
        assert_eq!(&g.update(true, 0., false, 2.5)[..2], &[0.30, 0.30]);
        assert!((g.update(true, 0., false, 2.5)[0] - 0.35).abs() < 1e-6);
        assert_eq!(g.update(false, 0., false, 1.), [0.; 6]);
    }
    #[test]
    fn underwater_mode_uses_independent_transitions_and_fades_on_exit() {
        let mut g = WorldGlow::default();
        g.update(true, 1., false, 5.);
        let a = g.update(true, 1., true, 1.);
        assert!((a[0] - 0.425).abs() < 1e-6);
        assert!((a[1] - 0.375).abs() < 1e-6);
        assert!((a[5] - 1. / 3.).abs() < 1e-6);
        assert_eq!(&a[2..5], &[187. / 255., 234. / 255., 187. / 255.]);
        let b = g.update(true, 1., true, 1.);
        assert_eq!(&b[..2], &[0.6, 0.5]);
        assert_eq!(b[5], 0.5);
        let c = g.update(true, 1., false, 2.5);
        assert!((c[0] - 0.425).abs() < 1e-6);
        assert!((c[1] - 0.375).abs() < 1e-6);
        assert_eq!(c[5], 0.25);
        assert_eq!(g.update(true, 1., false, 2.5)[5], 0.);
    }
}
#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    fn native_stored_step_accumulates_per_frame_not_elapsed_ratio() {
        let mut c = Channel::default();
        c.set(0., 0.25, 5000);
        assert_eq!(c.step.to_bits(), 0x3851b717);
        for _ in 0..312 {
            c.tick(16);
        }
        // Exact recurrence from fild/fmul/fadd/fstp; elapsed-ratio lerp is
        // 24 ULPs away. Endpoint snaps when remaining time is exhausted.
        assert_eq!(c.value.to_bits(), 0x3e7f970c);
        assert_eq!(c.remaining, 8);
        c.tick(8);
        assert_eq!(c.value, 0.25);
        c.tick(1000);
        assert_eq!(c.value, 0.25);
    }
    #[test]
    fn zero_duration_holds_start_negative_duration_snaps_on_update() {
        let mut c = Channel::default();
        c.set(0.2, 0.8, 0);
        c.tick(100);
        assert_eq!(c.value, 0.2);
        c.set(0.2, 0.8, -1);
        assert_eq!(c.value, 0.2);
        c.tick(0);
        assert_eq!(c.value, 0.8);
        let mut c = Channel::default();
        c.set(0., 1., 100);
        c.tick(-1);
        assert_eq!(c.remaining, 101);
        assert_eq!(c.value, -0.01);
    }
    #[test]
    fn shader_register_order_is_level_then_power_not_setter_order() {
        let mut g = WorldGlow::default();
        g.select(true, 1., false, 0, 0xffffffff);
        g.tick(5000);
        g.select(true, 1., true, 0, 0xffffffff);
        g.tick(1000);
        let p = g.parameters();
        assert_eq!(&p[..2], &[0.425, 0.375]);
        assert_eq!(p[5], 1. / 3.);
        g.tick(1000);
        assert_eq!(&g.parameters()[..2], &[0.6, 0.5]);
    }
    #[test]
    fn mode_four_keeps_fade_other_modes_release_it_over_one_second() {
        let mut g = WorldGlow::default();
        g.select(true, 1., false, 0, 0xffffffff);
        g.set_fade(0., 1., 1000);
        g.tick(500);
        assert_eq!(g.brightness_fade(), 0.5);
        g.select(true, 1., false, 4, 0xffffffff);
        g.tick(500);
        assert_eq!(g.brightness_fade(), 1.);
        g.select(true, 1., true, 0, 0xffffffff);
        g.tick(500);
        assert_eq!(g.brightness_fade().to_bits(), 0x3effffff);
        g.tick(500);
        assert_eq!(g.brightness_fade(), 0.);
        g.stop_fade(false);
        g.tick(1000);
        assert_eq!(g.brightness_fade(), 1.);
    }
    #[test]
    fn tint_change_restarts_same_mode_but_unchanged_type_and_color_do_not() {
        let mut g = WorldGlow::default();
        g.select(true, 1., false, 0, 0xffffffff);
        g.tick(1000);
        let remaining = g.level.remaining;
        g.select(true, 1., false, 0, 0xffffffff);
        assert_eq!(g.level.remaining, remaining);
        let current = g.parameters();
        g.select(true, 1., false, 0, 0xff804020);
        assert_eq!(g.level.remaining, 5000);
        assert_eq!(g.parameters(), current);
    }
    #[test]
    fn disabled_presenter_freezes_and_resumes_without_erasing_state() {
        let mut g = WorldGlow::default();
        g.select(true, 1., true, 0, 0xffffffff);
        g.tick(500);
        let before = g.parameters();
        let remaining = g.mono.remaining;
        g.select(false, 0., false, 5, 0xff000000);
        g.set_fade(0., 1., 1000);
        g.tick(10000);
        assert_eq!(g.parameters(), [0.; 6]);
        assert_eq!(g.mono.remaining, remaining);
        g.select(true, 1., true, 0, 0xffffffff);
        assert_eq!(g.parameters(), before);
        g.tick(1000);
        assert_eq!(g.parameters()[5], 0.5);
    }
}
