//! Native weather-object fixed-step clock from elementclient 1.4.2.
//!
//! `CECWorld` accumulates its frame delta in `0x9b3ed4` at `0x429378..0x42939d`.
//! It advances both optional weather objects through vtable `+0x48` while the
//! accumulator is strictly greater than `0x21` milliseconds, subtracting 33 ms
//! after each update (`0x4293a2..0x4293c7`). The region weather selector is
//! recovered separately: `0x421520` reads the current area's flags at runtime
//! offset `+0x80`, enables rain for bit 1 and snow for bit 2, and disables each
//! missing effect (`0x421533..0x42156f`). Those are EC-BSD flag-word bits 1 and
//! 2 (`world.ecbsd` record offset `+0x04`). `weather_gpu` consumes this
//! selection for depth-tested world-space precipitation in main, reflection,
//! and above-water-view passes. The deterministic camera-centred particle
//! model still differs from native RNG, lifetimes, collisions and transitions.

/// Precipitation choices encoded in the currently selected EC-BSD area's
/// flags. Native `0x421520` applies rain and snow independently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeatherSelection {
    pub rain: bool,
    pub snow: bool,
}
impl WeatherSelection {
    pub const RAIN_FLAG: u32 = 1 << 1;
    pub const SNOW_FLAG: u32 = 1 << 2;

    pub fn from_area_flags(flags: u32) -> Self {
        Self {
            rain: flags & Self::RAIN_FLAG != 0,
            snow: flags & Self::SNOW_FLAG != 0,
        }
    }
}
/// One precipitation object's tuned particle parameters.
///
/// The client writes these as plain stores in the two constructors —
/// `0x42901c`–`0x429067` for `textures/rain.tga` and `0x42919f`–`0x4291eb`
/// for `textures/snow.bmp` — and its own debug dumper `0x79e270` labels the
/// fields (`+0x1600` `Speed`, `+0x1604` `SpeedVariation: %f%%`, `+0x160c`
/// `Size: %f`, `+0x1610` `SizeVariation: %f%%`). A field a constructor never
/// stores stays at the object's own default, so `None` means "the client does
/// not write this for that object", not "zero".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrecipitationProfile {
    pub speed: f32,
    pub speed_variation: Option<f32>,
    pub size: f32,
    pub size_variation: Option<f32>,
}

/// `textures/rain.tga`: `Speed 0.5`, `SpeedVariation 50 %`, `Size 0.04`,
/// `SizeVariation 30 %`.
pub const RAIN: PrecipitationProfile = PrecipitationProfile {
    speed: 0.5,
    speed_variation: Some(0.5),
    size: 0.04,
    size_variation: Some(0.3),
};

/// `textures/snow.bmp`: `Speed 0.5`, `Size 0.3`; the constructor stores no
/// variation for either field.
pub const SNOW: PrecipitationProfile = PrecipitationProfile {
    speed: 0.5,
    speed_variation: None,
    size: 0.3,
    size_variation: None,
};

/// How much of the shared `Speed 0.5` the viewer keeps for the flakes.
///
/// Both objects store `Speed = 0.5`, but the constructors also write two
/// integer fields that are not decoded (`+0x1790 = 40` for snow,
/// `+0x1794 = 8` for rain) and the client's flakes visibly descend far slower
/// than its drops. Rather than pretend the single recovered `Speed` explains
/// both, the viewer applies it to rain and keeps its own flake descent as this
/// explicit factor.
pub const SNOW_SPEED_SCALE: f32 = 0.06;

/// Viewer override for the area's own weather flags (World tab).
///
/// `Auto` is the client's own selection: `0x421520` turns rain on for EC-BSD
/// flag bit 1 and snow on for bit 2, independently. The other choices force one
/// effect so the recovered constants and the two client textures can be checked
/// in a region that never asks for them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WeatherOverride {
    #[default]
    Auto,
    Clear,
    Rain,
    Snow,
}

impl WeatherOverride {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Clear, Self::Rain, Self::Snow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (area flags)",
            Self::Clear => "Clear",
            Self::Rain => "Rain (rain.tga)",
            Self::Snow => "Snow (snow.bmp)",
        }
    }

    /// Persisted spelling; short, so a settings file stays readable.
    pub fn token(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Clear => "Clear",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.token() == text)
    }

    /// The forced selection, given what the selected area asks for.
    pub fn apply(self, area: WeatherSelection) -> WeatherSelection {
        match self {
            Self::Auto => area,
            Self::Clear => WeatherSelection::default(),
            Self::Rain => WeatherSelection {
                rain: true,
                snow: false,
            },
            Self::Snow => WeatherSelection {
                rain: false,
                snow: true,
            },
        }
    }
}

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
    fn native_area_bits_select_rain_and_snow_independently() {
        assert_eq!(
            WeatherSelection::from_area_flags(0x18),
            WeatherSelection::default()
        );
        assert_eq!(
            WeatherSelection::from_area_flags(WeatherSelection::RAIN_FLAG),
            WeatherSelection {
                rain: true,
                snow: false
            }
        );
        assert_eq!(
            WeatherSelection::from_area_flags(WeatherSelection::SNOW_FLAG),
            WeatherSelection {
                rain: false,
                snow: true
            }
        );
        assert_eq!(
            WeatherSelection::from_area_flags(
                WeatherSelection::RAIN_FLAG | WeatherSelection::SNOW_FLAG | 0x18
            ),
            WeatherSelection {
                rain: true,
                snow: true
            }
        );
    }

    #[test]
    fn native_particle_constants_are_the_two_constructors_stores() {
        // Rain: 0x42901c..0x429067.
        assert_eq!(RAIN.speed, 0.5);
        assert_eq!(RAIN.speed_variation, Some(0.5));
        assert_eq!(RAIN.size, 0.04);
        assert_eq!(RAIN.size_variation, Some(0.3));
        // Snow: 0x42919f..0x4291eb. Same Speed, no variation stored, Size 0.3.
        assert_eq!(SNOW.speed, 0.5);
        assert_eq!(SNOW.speed_variation, None);
        assert_eq!(SNOW.size, 0.3);
        assert_eq!(SNOW.size_variation, None);
        // The flake descent the viewer keeps is slower than the drop descent.
        assert!(SNOW_SPEED_SCALE > 0.0 && SNOW_SPEED_SCALE < 0.2);
    }

    #[test]
    fn the_weather_override_forces_one_effect_or_keeps_the_area() {
        let area = WeatherSelection {
            rain: false,
            snow: true,
        };
        assert_eq!(WeatherOverride::Auto.apply(area), area);
        assert_eq!(
            WeatherOverride::Clear.apply(area),
            WeatherSelection::default()
        );
        let rain = WeatherOverride::Rain.apply(area);
        assert!(rain.rain && !rain.snow, "rain override clears the snow");
        let snow = WeatherOverride::Snow.apply(WeatherSelection::default());
        assert!(!snow.rain && snow.snow, "snow override clears the rain");
        for option in WeatherOverride::ALL {
            assert_eq!(WeatherOverride::parse(option.token()), Some(option));
        }
        assert_eq!(WeatherOverride::parse("bogus"), None);
    }

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
