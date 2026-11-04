//! AutoFamily sky configuration and the native day/night resource transition.
//!
//! The 1.4.2 AutoScene loader reads `Sky/sky_id_day` and
//! `Sky/sky_id_night`, resolves each integer through `Resource/sky_list`, and
//! asks A3DSkySphere to transition to the selected three-texture set over
//! 1000 ms. EC-BSD v12 areas do not contain a sky-set ID; their authored
//! celestial direction/visibility remain independent regional inputs.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkySet {
    pub id: i32,
    pub label: String,
    pub roof: String,
    pub panorama_01: String,
    pub panorama_02: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoSkyConfig {
    pub day: i32,
    pub night: i32,
    pub sky_list: String,
}

/// AutoFamily's scene-level fog defaults. EC-BSD area fog remains the
/// regional override; these values are used only when no authored area is
/// selected. Colors are normalized RGB, matching the renderer's shader input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoFogConfig {
    pub enabled: bool,
    pub day_start: f32,
    pub day_end: f32,
    pub day_color: [f32; 3],
    pub night_start: f32,
    pub night_end: f32,
    pub night_color: [f32; 3],
    pub water_color: [f32; 3],
}

/// Parse the AutoFamily parser's global Fog section. The executable reads
/// these fields alongside the Sky and Resource sections (1.4.2, 0x6c12c0).
pub fn parse_autofog(bytes: &[u8]) -> Result<AutoFogConfig, String> {
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        return Err("AutoFamily config is not valid GBK/ASCII".into());
    }
    let mut section = String::new();
    let mut enabled = None;
    let mut day_start = None;
    let mut day_end = None;
    let mut day_color = None;
    let mut night_start = None;
    let mut night_end = None;
    let mut night_color = None;
    let mut water_color = None;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.split(';').next().unwrap_or("").trim();
        let key = key.trim().to_ascii_lowercase();
        if section != "fog" {
            continue;
        }
        let number = |name: &str| -> Result<f32, String> {
            let parsed = value
                .parse::<f32>()
                .map_err(|e| format!("Fog/{name}: {e}"))?;
            if !parsed.is_finite() {
                return Err(format!("Fog/{name} is not finite"));
            }
            Ok(parsed)
        };
        let color = |name: &str| -> Result<[f32; 3], String> {
            let parts: Vec<&str> = value
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|part| !part.is_empty())
                .collect();
            if parts.len() != 3 {
                return Err(format!("Fog/{name} must contain three color channels"));
            }
            let mut out = [0.; 3];
            for i in 0..3 {
                let channel = parts[i]
                    .parse::<f32>()
                    .map_err(|e| format!("Fog/{name}: {e}"))?;
                if !channel.is_finite() {
                    return Err(format!("Fog/{name} has a non-finite channel"));
                }
                out[i] = channel.clamp(0., 255.) / 255.;
            }
            Ok(out)
        };
        match key.as_str() {
            "fog_enable" => {
                let v = number("fog_enable")?;
                enabled = Some(v != 0.);
            }
            "fog_start_day" => day_start = Some(number("fog_start_day")?),
            "fog_end_day" => day_end = Some(number("fog_end_day")?),
            "fog_color_day" => day_color = Some(color("fog_color_day")?),
            "fog_start_night" => night_start = Some(number("fog_start_night")?),
            "fog_end_night" => night_end = Some(number("fog_end_night")?),
            "fog_color_night" => night_color = Some(color("fog_color_night")?),
            "fog_color_water" => water_color = Some(color("fog_color_water")?),
            _ => {}
        }
    }
    Ok(AutoFogConfig {
        enabled: enabled.ok_or("AutoFamily config is missing Fog/fog_enable")?,
        day_start: day_start.ok_or("AutoFamily config is missing Fog/fog_start_day")?,
        day_end: day_end.ok_or("AutoFamily config is missing Fog/fog_end_day")?,
        day_color: day_color.ok_or("AutoFamily config is missing Fog/fog_color_day")?,
        night_start: night_start.ok_or("AutoFamily config is missing Fog/fog_start_night")?,
        night_end: night_end.ok_or("AutoFamily config is missing Fog/fog_end_night")?,
        night_color: night_color.ok_or("AutoFamily config is missing Fog/fog_color_night")?,
        water_color: water_color.ok_or("AutoFamily config is missing Fog/fog_color_water")?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyMix {
    /// Layer index in the two-set GPU array: zero is configured day, one night.
    pub from: u32,
    pub to: u32,
    pub amount: f32,
}

/// Parse the runtime AutoFamily integer IDs from an INI-like config.
pub fn parse_autoscene(bytes: &[u8]) -> Result<AutoSkyConfig, String> {
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        return Err("AutoFamily config is not valid GBK/ASCII".into());
    }
    let mut section = String::new();
    let mut day = None;
    let mut night = None;
    let mut sky_list = None;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.split(';').next().unwrap_or("").trim();
        let key = key.trim().to_ascii_lowercase();
        match (section.as_str(), key.as_str()) {
            ("sky", "sky_id_day") => day = Some(value.parse::<i32>().map_err(|e| e.to_string())?),
            ("sky", "sky_id_night") => {
                night = Some(value.parse::<i32>().map_err(|e| e.to_string())?)
            }
            ("resource", "sky_list") => sky_list = Some(value.replace('\\', "/")),
            _ => {}
        }
    }
    Ok(AutoSkyConfig {
        day: day.ok_or("AutoFamily config is missing Sky/sky_id_day")?,
        night: night.ok_or("AutoFamily config is missing Sky/sky_id_night")?,
        sky_list: sky_list.ok_or("AutoFamily config is missing Resource/sky_list")?,
    })
}

/// Parse `sky.lst`: count, then id/label/roof/01/02/flags for each record.
/// The trailing legacy integer is read and validated but is not a texture.
pub fn parse_sky_list(bytes: &[u8]) -> Result<Vec<SkySet>, String> {
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        return Err("sky.lst is not valid GBK".into());
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let count: usize = lines
        .first()
        .ok_or("sky.lst is empty")?
        .trim_start_matches('\u{feff}')
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    if count == 0 || count > 4096 {
        return Err(format!("invalid sky.lst set count {count}"));
    }
    let mut sets = Vec::with_capacity(count);
    let mut at = 1;
    for _ in 0..count {
        let take = |at: &mut usize| -> Result<&str, String> {
            let value = *lines.get(*at).ok_or("truncated sky.lst record")?;
            *at += 1;
            Ok(value)
        };
        let id = take(&mut at)?.parse::<i32>().map_err(|e| e.to_string())?;
        let label = take(&mut at)?.to_owned();
        let roof = take(&mut at)?.replace('\\', "/");
        let panorama_01 = take(&mut at)?.replace('\\', "/");
        let panorama_02 = take(&mut at)?.replace('\\', "/");
        let _legacy_flags = take(&mut at)?.parse::<i32>().map_err(|e| e.to_string())?;
        if sets.iter().any(|set: &SkySet| set.id == id) {
            return Err(format!("duplicate sky.lst ID {id}"));
        }
        sets.push(SkySet {
            id,
            label,
            roof,
            panorama_01,
            panorama_02,
        });
    }
    Ok(sets)
}

/// Mirror A3DSkySphere's day/night choice (night factor >= 0.5) and its
/// 1000-ms transition request. The native config path passes 1000 to
/// `TransSky`; the viewer consumes one frozen logic delta per stable target
/// frame and starts a changed target at the currently displayed mix.
#[derive(Clone, Copy, Debug)]
pub struct SkyTransition {
    start_mix: f32,
    target_mix: f32,
    elapsed_ms: u32,
}
impl Default for SkyTransition {
    fn default() -> Self {
        Self {
            start_mix: 0.0,
            target_mix: 0.0,
            elapsed_ms: Self::DURATION_MS,
        }
    }
}
impl SkyTransition {
    pub const DURATION_MS: u32 = 1000;

    pub fn update(&mut self, night_factor: f32, delta_ms: u32) -> SkyMix {
        let desired = if night_factor >= 0.5 { 1.0 } else { 0.0 };
        let current = self.mix();
        if desired != self.target_mix {
            // TransSky can be requested again before the previous fade ends.
            // Retarget from the currently displayed mix instead of snapping to
            // the previous destination sky on a rapid clock reversal. As with
            // regional fog key changes, do not consume the old frame delta on
            // the frame that installs a new transition target.
            self.start_mix = current;
            self.target_mix = desired;
            self.elapsed_ms = 0;
            return SkyMix {
                from: 0,
                to: 1,
                amount: current,
            };
        }
        self.elapsed_ms = self
            .elapsed_ms
            .saturating_add(delta_ms)
            .min(Self::DURATION_MS);
        let amount = self.mix();
        SkyMix {
            from: 0,
            to: 1,
            amount,
        }
    }

    fn mix(&self) -> f32 {
        let t = self.elapsed_ms as f32 / Self::DURATION_MS as f32;
        self.start_mix + (self.target_mix - self.start_mix) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &[u8] = b"[Sky]\r\nsky_id_day = 2\r\nsky_id_night=7\r\n[Fog]\r\nfog_enable=1\r\n[Resource]\r\nsky_list=Configs\\AutoFamilyConfigs\\sky.lst\r\n";
    const FOG_CONFIG: &[u8] = b"[Fog]\r\nfog_enable=1\r\nfog_start_day=20.000000\r\nfog_end_day=500.000000\r\nfog_color_day=192, 192, 192\r\nfog_start_night=50.000000\r\nfog_end_night=200.000000\r\nfog_color_night=192, 192, 192\r\nfog_color_water=255, 255, 255\r\n";
    const LIST: &[u8] = b"2\r\n2\r\nSunset\r\nsunset\\roof.bmp\r\nsunset\\01.bmp\r\nsunset\\02.bmp\r\n0\r\n7\r\nCloud\r\ncloud\\roof.bmp\r\ncloud\\01.bmp\r\ncloud\\02.bmp\r\n0\r\n";

    #[test]
    fn auto_family_ids_resolve_to_list_entries_and_normalized_paths() {
        let config = parse_autoscene(CONFIG).unwrap();
        assert_eq!(config.day, 2);
        assert_eq!(config.night, 7);
        assert_eq!(config.sky_list, "Configs/AutoFamilyConfigs/sky.lst");
        let sets = parse_sky_list(LIST).unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].id, config.day);
        assert_eq!(sets[1].id, config.night);
        assert_eq!(sets[0].roof, "sunset/roof.bmp");
        assert_eq!(sets[0].panorama_02, "sunset/02.bmp");
    }

    #[test]
    fn auto_family_fog_values_parse_and_normalize_rgb() {
        let fog = parse_autofog(FOG_CONFIG).unwrap();
        assert!(fog.enabled);
        assert_eq!(fog.day_start, 20.);
        assert_eq!(fog.day_end, 500.);
        assert_eq!(fog.day_color, [192. / 255.; 3]);
        assert_eq!(fog.night_start, 50.);
        assert_eq!(fog.night_end, 200.);
        assert_eq!(fog.night_color, [192. / 255.; 3]);
        assert_eq!(fog.water_color, [1.; 3]);
    }

    #[test]
    fn day_night_selection_uses_half_night_factor_and_native_one_second_fade() {
        let mut t = SkyTransition::default();
        assert_eq!(
            t.update(0.499, 16),
            SkyMix {
                from: 0,
                to: 1,
                amount: 0.
            }
        );
        assert_eq!(
            t.update(0.5, 500),
            SkyMix {
                from: 0,
                to: 1,
                amount: 0.
            }
        );
        assert_eq!(t.update(1., 500).amount, 0.5);
        assert_eq!(t.update(1., 500).amount, 1.);
        assert_eq!(t.update(0.5, 0).amount, 1.);
        assert_eq!(t.update(0.499, 0).amount, 1.);
    }

    #[test]
    fn reversing_day_night_transition_does_not_jump_to_the_old_destination() {
        let mut t = SkyTransition::default();
        t.update(0.5, 0);
        assert_eq!(t.update(0.5, 500).amount, 0.5);
        assert_eq!(t.update(0.499, 0).amount, 0.5);
        assert_eq!(t.update(0.499, 250).amount, 0.375);
        assert_eq!(t.update(0.499, 750).amount, 0.);
    }
}
