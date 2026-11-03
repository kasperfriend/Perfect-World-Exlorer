//! Native weather-object fixed-step clock from elementclient 1.4.2.
//!
//! `CECWorld` accumulates its frame delta in `0x9b3ed4` at `0x429378..0x42939d`.
//! It advances both optional weather objects through vtable `+0x48` while the
//! accumulator is strictly greater than `0x21` milliseconds, subtracting 33 ms
//! after each update (`0x4293a2..0x4293c7`). This is timing only; it does not
//! choose weather, create sprites, or implement their visual simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeatherClock {
    accumulator_ms: u32,
}
impl WeatherClock {
    /// Add one native world-frame delta and return the number of 33 ms updates
    /// due for each extant weather object. The strict `> 33` threshold is
    /// intentional: the client skips an update at exactly 33 ms.
    pub fn advance(&mut self, delta_ms: u32) -> u32 {
        let total = self.accumulator_ms.wrapping_add(delta_ms);
        let ticks = total.saturating_sub(1) / 33;
        self.accumulator_ms = total - ticks * 33;
        ticks
    }

    pub fn remainder_ms(self) -> u32 {
        self.accumulator_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_weather_tick_uses_strict_33_millisecond_threshold() {
        let mut clock = WeatherClock::default();
        assert_eq!(clock.advance(33), 0);
        assert_eq!(clock.remainder_ms(), 33);
        assert_eq!(clock.advance(1), 1);
        assert_eq!(clock.remainder_ms(), 1);

        assert_eq!(clock.advance(65), 1); // total 66: one tick, 33 ms retained
        assert_eq!(clock.remainder_ms(), 33);
        assert_eq!(clock.advance(1), 1);
        assert_eq!(clock.remainder_ms(), 1);
    }

    #[test]
    fn native_weather_tick_catches_up_multiple_steps_and_preserves_remainder() {
        let mut clock = WeatherClock::default();
        assert_eq!(clock.advance(100), 3);
        assert_eq!(clock.remainder_ms(), 1);
        assert_eq!(clock.advance(0), 0);
        assert_eq!(clock.advance(32), 0); // exactly 33 ms still does not tick
        assert_eq!(clock.remainder_ms(), 33);
        assert_eq!(clock.advance(1), 1);
        assert_eq!(clock.remainder_ms(), 1);
    }

    #[test]
    fn weather_clock_wraps_its_u32_accumulator_like_the_native_dword() {
        let mut clock = WeatherClock {
            accumulator_ms: u32::MAX - 10,
        };
        assert_eq!(clock.advance(20), 0);
        assert_eq!(clock.remainder_ms(), 9);
    }
}
