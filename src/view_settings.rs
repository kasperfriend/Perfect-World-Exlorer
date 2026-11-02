//! Persisted viewer controls; malformed values fall back safely.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DetailSettings {
    pub radius: isize,
    pub tree_lod_scale: f32,
}
impl Default for DetailSettings {
    fn default() -> Self {
        Self {
            radius: 6,
            tree_lod_scale: 1.0,
        }
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct ViewSettings {
    pub detail: DetailSettings,
    pub camera_speed: f32,
    pub boost: f32,
    pub sensitivity: f32,
    pub far: f32,
    pub time_of_day: f32,
    pub whole_world: bool,
    pub water_reflections: bool,
    pub atmospheric_fog: bool,
    pub bloom: bool,
    pub bloom_strength: f32,
    pub client_gamma: bool,
    pub gamma_setting: f32,
    pub terrain_lightmaps: bool,
    pub model_backface_culling: bool,
    pub music_enabled: bool,
    pub music_volume: f32,
    pub fullscreen: bool,
    pub show_zone_entry: bool,
    pub show_minimap: bool,
    pub minimap_range: f32,
    pub tree_wind_strength: f32,
    pub grass_wind_strength: f32,
    pub regional_tree_lighting: bool,
    pub streamed_regions: bool,
    pub grass_distance_fade: bool,
    pub area_grass_lightmaps: bool,
    pub celestial_discs: bool,
}
impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            detail: DetailSettings::default(),
            camera_speed: 240.,
            boost: 4.,
            sensitivity: 0.0025,
            far: 16000.,
            time_of_day: 12.,
            whole_world: false,
            water_reflections: true,
            atmospheric_fog: false,
            bloom: false,
            bloom_strength: 1.,
            client_gamma: false,
            gamma_setting: 41.,
            terrain_lightmaps: true,
            model_backface_culling: true,
            music_enabled: false,
            music_volume: 0.5,
            fullscreen: false,
            show_zone_entry: true,
            show_minimap: true,
            minimap_range: 256.,
            tree_wind_strength: 0.2,
            grass_wind_strength: 0.,
            regional_tree_lighting: false,
            streamed_regions: false,
            grass_distance_fade: true,
            area_grass_lightmaps: false,
            celestial_discs: false,
        }
    }
}

impl ViewSettings {
    fn decode(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines().take(100) {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim();
            macro_rules! number {
                ($field:expr,$min:expr,$max:expr) => {
                    if let Ok(x) = v.parse::<f32>() {
                        if x.is_finite() {
                            $field = x.clamp($min, $max)
                        }
                    }
                };
            }
            macro_rules! flag {
                ($field:expr) => {
                    if let Ok(x) = v.parse::<bool>() {
                        $field = x
                    }
                };
            }
            match k {
                "radius" => {
                    if let Ok(x) = v.parse::<isize>() {
                        s.detail.radius = x.clamp(2, 12)
                    }
                }
                "tree_lod_scale" => number!(s.detail.tree_lod_scale, 0.25, 4.),
                "camera_speed" => number!(s.camera_speed, 10., 3000.),
                "boost" => number!(s.boost, 1., 10.),
                "sensitivity" => number!(s.sensitivity, 0.0005, 0.01),
                "far" => number!(s.far, 2000., 24000.),
                "time_of_day" => number!(s.time_of_day, 0., 24.),
                "grass_wind_strength" => number!(s.grass_wind_strength, 0., 1.),
                "tree_wind_strength" => number!(s.tree_wind_strength, 0., 1.),
                "bloom_strength" => number!(s.bloom_strength, 0., 4.),
                "client_gamma" => flag!(s.client_gamma),
                "gamma_setting" => number!(s.gamma_setting, 0., 100.),
                "music_volume" => number!(s.music_volume, 0., 1.),
                "minimap_range" => number!(s.minimap_range, 100., 600.),
                "regional_tree_lighting" => flag!(s.regional_tree_lighting),
                "streamed_regions" => flag!(s.streamed_regions),
                "grass_distance_fade" => flag!(s.grass_distance_fade),
                "area_grass_lightmaps" => flag!(s.area_grass_lightmaps),
                "celestial_discs" => flag!(s.celestial_discs),
                "whole_world" => flag!(s.whole_world),
                "water_reflections" => flag!(s.water_reflections),
                "atmospheric_fog" => flag!(s.atmospheric_fog),
                "bloom" => flag!(s.bloom),
                "terrain_lightmaps" => flag!(s.terrain_lightmaps),
                "model_backface_culling" => flag!(s.model_backface_culling),
                "music_enabled" => flag!(s.music_enabled),
                "fullscreen" => flag!(s.fullscreen),
                "show_zone_entry" => flag!(s.show_zone_entry),
                "show_minimap" => flag!(s.show_minimap),
                _ => {}
            }
        }
        s
    }
    fn encode(&self) -> String {
        format!(concat!("# Perfect World Explorer settings, version 1\n",
            "radius={}\ntree_lod_scale={}\ncamera_speed={}\nboost={}\nsensitivity={}\nfar={}\ntime_of_day={}\n",
            "whole_world={}\nwater_reflections={}\natmospheric_fog={}\nbloom={}\nbloom_strength={}\nclient_gamma={}\ngamma_setting={}\n",
            "terrain_lightmaps={}\nmodel_backface_culling={}\nmusic_enabled={}\nmusic_volume={}\nfullscreen={}\nshow_zone_entry={}\n",
            "show_minimap={}\nminimap_range={}\ntree_wind_strength={}\nregional_tree_lighting={}\ngrass_distance_fade={}\ngrass_wind_strength={}\nstreamed_regions={}\narea_grass_lightmaps={}\ncelestial_discs={}\n"),
            self.detail.radius,self.detail.tree_lod_scale,self.camera_speed,self.boost,self.sensitivity,self.far,self.time_of_day,
            self.whole_world,self.water_reflections,self.atmospheric_fog,self.bloom,self.bloom_strength,self.client_gamma,self.gamma_setting,
            self.terrain_lightmaps,self.model_backface_culling,self.music_enabled,self.music_volume,self.fullscreen,self.show_zone_entry,
            self.show_minimap,self.minimap_range,self.tree_wind_strength,self.regional_tree_lighting,self.grass_distance_fade,self.grass_wind_strength,self.streamed_regions,self.area_grass_lightmaps,self.celestial_discs)
    }
    fn file() -> Option<std::path::PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("LOCALAPPDATA")
                .map(std::path::PathBuf::from)
                .map(|p| p.join("PerfectWorldViewer").join("viewer-settings.txt"))
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config"))
                })
                .map(|p| p.join("PerfectWorldViewer").join("viewer-settings.txt"))
        }
    }
    pub fn load() -> Self {
        Self::file()
            .and_then(|p| std::fs::read(p).ok())
            .filter(|b| b.len() <= 16384)
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| Self::decode(&s))
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = Self::file() else {
            return Err("settings directory is unavailable".into());
        };
        let parent = path.parent().ok_or("settings directory is unavailable")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // Write a complete temporary file before replacing the old one. Windows
        // rename cannot replace existing files; remove the old file only after the temp write succeeds.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.encode()).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
        }
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_all_settings() {
        let mut s = ViewSettings::default();
        s.detail.radius = 11;
        s.bloom = true;
        s.whole_world = true;
        s.camera_speed = 511.;
        s.show_minimap = false;
        s.music_volume = 0.21;
        s.atmospheric_fog = true;
        s.celestial_discs = true;
        s.regional_tree_lighting = true;
        s.grass_distance_fade = false;
        assert_eq!(s, ViewSettings::decode(&s.encode()));
    }
    #[test]
    fn bad_values_are_safe() {
        let s = ViewSettings::decode(
            "far=NaN\nradius=90000\nboost=-5\nsensitivity=inf\nbloom=bad\nunknown=stuff\n",
        );
        assert_eq!(s.far, 16000.);
        assert_eq!(s.detail.radius, 12);
        assert_eq!(s.boost, 1.);
        assert_eq!(s.sensitivity, 0.0025);
        assert!(!s.bloom);
    }
}
