//! Persisted viewer controls; malformed values fall back safely.
#[path = "post_plan.rs"]
mod post_plan;
/// The renderer must allocate/execute the post chain for each independent
/// scene effect, not only when bloom happens to be enabled. The retained
/// screen-space fog is a diagnostic route and is deliberately not an input:
/// production shading is fogged per material (`fog_matrix`).
pub fn post_required(bloom: bool, sharpen: bool, underwater: bool, gamma: bool) -> bool {
    post_plan::required(post_plan::Inputs {
        bloom,
        sharpen,
        underwater,
        screen_fog: false,
        fog_enabled: false,
        display_gamma: gamma,
        srgb_target: true,
    })
}

/// Which artwork a map surface shows.
///
/// `Realistic` renders the world from above with the live scene. `Original`
/// shows the client's own map textures, read from `surfaces.pck`:
/// `surfaces/maps/` for the world map and `surfaces/minimaps/<zone>/` for the
/// minimap (`Surfaces\\MiniMaps\\%02d%02d`, `0x565220`). The two surfaces select
/// independently because the client draws them from different asset sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapStyle {
    #[default]
    Realistic,
    Original,
}
impl MapStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Realistic => "Realistic",
            Self::Original => "Original",
        }
    }
    /// Persisted spelling; anything unrecognised keeps the default rather than
    /// silently becoming a different style.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "Realistic" => Some(Self::Realistic),
            "Original" => Some(Self::Original),
            _ => None,
        }
    }
}

/// Minimap HUD outline. The client's own minimap is a square tile crop with a
/// round frame; both are offered because the square keeps the full tile view.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MinimapShape {
    #[default]
    Square,
    Circular,
}
impl MinimapShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Circular => "Circular",
        }
    }
    fn parse(text: &str) -> Option<Self> {
        match text {
            "Square" => Some(Self::Square),
            "Circular" => Some(Self::Circular),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DetailSettings {
    /// Terrain block window half-size in blocks (`world+0x1bc / block size`,
    /// `0x77d720`). The client's range is 420 world units for the shipped
    /// quality presets (`0x40e41f`), i.e. 7 blocks of 64 units.
    pub radius: isize,
    pub tree_lod_scale: f32,
    /// Native `CECSceneBlock` quality preset (`0x424230`, `Sight+1` at
    /// `0x40e155`). Preset 2 is the shipped `detail_scene = 1` default and
    /// loads the ornament class within 120 world units (`0x42449c`).
    pub scenery_quality: u8,
}
impl Default for DetailSettings {
    fn default() -> Self {
        Self {
            radius: 7,
            tree_lod_scale: 1.0,
            scenery_quality: 2,
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
    pub sharpen: bool,
    pub bloom_strength: f32,
    pub bloom_mode: u8,
    pub bloom_tint: u32,
    pub bloom_brightness_fade: f32,
    pub bloom_opacity_fade: f32,
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
    /// Optional client-signal modulation of the legacy sampled leaf loop.
    pub client_tree_wind_preview: bool,
    pub grass_wind_strength: f32,
    pub regional_tree_lighting: bool,
    pub streamed_regions: bool,
    pub grass_distance_fade: bool,
    pub area_grass_lightmaps: bool,
    /// Opt-in P4.07 authored vegetation mip experiment. Default false keeps the
    /// approved generated-mip world path (P4.08). Applied at startup; restart to change.
    pub vegetation_authored_mips: bool,
    pub celestial_discs: bool,
    /// World map artwork (the full-world page).
    pub world_map_style: MapStyle,
    /// Minimap artwork, chosen separately from the world map.
    pub minimap_style: MapStyle,
    /// Minimap HUD outline.
    pub minimap_shape: MinimapShape,
    /// Internal 3D render resolution as a percentage of the window size.
    /// 100 is native; the Graphics tab exposes 10 (fast/blurry) to 200
    /// (supersampled). UI and presentation always stay at native resolution.
    pub resolution_scale: f32,
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
            sharpen: false,
            bloom_strength: 1.,
            bloom_mode: 0,
            bloom_tint: 0xffffffff,
            bloom_brightness_fade: 0.,
            bloom_opacity_fade: 0.,
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
            client_tree_wind_preview: false,
            grass_wind_strength: 0.,
            regional_tree_lighting: false,
            streamed_regions: false,
            grass_distance_fade: true,
            area_grass_lightmaps: true,
            vegetation_authored_mips: false,
            // The client sun and moon are the only discs the shipped
            // `Sky\clear` set does not paint, so they are on by default.
            celestial_discs: true,
            world_map_style: MapStyle::Realistic,
            minimap_style: MapStyle::Realistic,
            minimap_shape: MinimapShape::Square,
            resolution_scale: 100.,
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
                "sharpen" => flag!(s.sharpen),
                "radius" => {
                    if let Ok(x) = v.parse::<isize>() {
                        s.detail.radius = x.clamp(2, 12)
                    }
                }
                "tree_lod_scale" => number!(s.detail.tree_lod_scale, 0.25, 4.),
                "scenery_quality" => {
                    if let Ok(x) = v.parse::<u8>() {
                        s.detail.scenery_quality = x.min(5)
                    }
                }
                "camera_speed" => number!(s.camera_speed, 10., 3000.),
                "boost" => number!(s.boost, 1., 10.),
                "sensitivity" => number!(s.sensitivity, 0.0005, 0.01),
                "far" => number!(s.far, 2000., 24000.),
                "time_of_day" => number!(s.time_of_day, 0., 24.),
                "grass_wind_strength" => number!(s.grass_wind_strength, 0., 1.),
                "tree_wind_strength" => number!(s.tree_wind_strength, 0., 1.),
                "client_tree_wind_preview" => flag!(s.client_tree_wind_preview),
                "bloom_strength" => number!(s.bloom_strength, 0., 4.),
                "bloom_tint" => {
                    if let Ok(color) = v
                        .strip_prefix("0x")
                        .map_or_else(|| v.parse::<u32>(), |hex| u32::from_str_radix(hex, 16))
                    {
                        s.bloom_tint = color;
                    }
                }
                "bloom_brightness_fade" => number!(s.bloom_brightness_fade, 0., 1.),
                "bloom_opacity_fade" => number!(s.bloom_opacity_fade, 0., 1.),
                "bloom_mode" => {
                    if let Ok(mode) = v.parse::<u8>() {
                        s.bloom_mode = if matches!(mode, 4 | 5) { mode } else { 0 };
                    }
                }
                "client_gamma" => flag!(s.client_gamma),
                "gamma_setting" => number!(s.gamma_setting, 0., 100.),
                "music_volume" => number!(s.music_volume, 0., 1.),
                "minimap_range" => number!(s.minimap_range, 100., 600.),
                "resolution_scale" => number!(s.resolution_scale, 10., 200.),
                "regional_tree_lighting" => flag!(s.regional_tree_lighting),
                "streamed_regions" => flag!(s.streamed_regions),
                "grass_distance_fade" => flag!(s.grass_distance_fade),
                "area_grass_lightmaps" => flag!(s.area_grass_lightmaps),
                "vegetation_authored_mips" => flag!(s.vegetation_authored_mips),
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
                "world_map_style" => {
                    if let Some(style) = MapStyle::parse(v) {
                        s.world_map_style = style;
                    }
                }
                "minimap_style" => {
                    if let Some(style) = MapStyle::parse(v) {
                        s.minimap_style = style;
                    }
                }
                "minimap_shape" => {
                    if let Some(shape) = MinimapShape::parse(v) {
                        s.minimap_shape = shape;
                    }
                }
                _ => {}
            }
        }
        s
    }
    fn encode(&self) -> String {
        format!(concat!("# Perfect World Explorer settings, version 1\n",
            "radius={}\ntree_lod_scale={}\nscenery_quality={}\ncamera_speed={}\nboost={}\nsensitivity={}\nfar={}\ntime_of_day={}\n",
            "whole_world={}\nwater_reflections={}\natmospheric_fog={}\nbloom={}\nbloom_strength={}\nbloom_mode={}\nbloom_tint={}\nbloom_brightness_fade={}\nbloom_opacity_fade={}\nclient_gamma={}\ngamma_setting={}\n",
            "terrain_lightmaps={}\nmodel_backface_culling={}\nmusic_enabled={}\nmusic_volume={}\nfullscreen={}\nshow_zone_entry={}\n",
            "show_minimap={}\nminimap_range={}\ntree_wind_strength={}\nclient_tree_wind_preview={}\nregional_tree_lighting={}\ngrass_distance_fade={}\ngrass_wind_strength={}\nstreamed_regions={}\narea_grass_lightmaps={}\nvegetation_authored_mips={}\ncelestial_discs={}\nsharpen={}\nresolution_scale={}\nworld_map_style={}\nminimap_style={}\nminimap_shape={}\n"),
            self.detail.radius,self.detail.tree_lod_scale,self.detail.scenery_quality,self.camera_speed,self.boost,self.sensitivity,self.far,self.time_of_day,
            self.whole_world,self.water_reflections,self.atmospheric_fog,self.bloom,self.bloom_strength,self.bloom_mode,self.bloom_tint,self.bloom_brightness_fade,self.bloom_opacity_fade,self.client_gamma,self.gamma_setting,
            self.terrain_lightmaps,self.model_backface_culling,self.music_enabled,self.music_volume,self.fullscreen,self.show_zone_entry,
            self.show_minimap,self.minimap_range,self.tree_wind_strength,self.client_tree_wind_preview,self.regional_tree_lighting,self.grass_distance_fade,self.grass_wind_strength,self.streamed_regions,self.area_grass_lightmaps,self.vegetation_authored_mips,self.celestial_discs,self.sharpen,self.resolution_scale,
            self.world_map_style.label(),self.minimap_style.label(),
            self.minimap_shape.label())
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
    fn authored_sharpen_toggle_round_trips_without_changing_defaults() {
        assert!(!ViewSettings::default().sharpen);
        let s = ViewSettings::decode("sharpen=true");
        assert!(s.sharpen);
        assert_eq!(s, ViewSettings::decode(&s.encode()));
        assert!(!ViewSettings::decode("sharpen=invalid").sharpen);
    }
    #[test]
    fn map_styles_default_to_realistic_and_persist_independently() {
        let d = ViewSettings::default();
        assert_eq!(d.world_map_style, MapStyle::Realistic);
        assert_eq!(d.minimap_style, MapStyle::Realistic);
        // The two surfaces are independent: an original minimap does not drag the
        // world map off the live render, and vice versa.
        let s = ViewSettings::decode("minimap_style=Original");
        assert_eq!(s.minimap_style, MapStyle::Original);
        assert_eq!(s.world_map_style, MapStyle::Realistic);
        let back = ViewSettings::decode(&s.encode());
        assert_eq!(back.minimap_style, MapStyle::Original);
        assert_eq!(back.world_map_style, MapStyle::Realistic);
        let both = ViewSettings::decode("world_map_style=Original\nminimap_style=Original");
        assert_eq!(both.world_map_style, MapStyle::Original);
        assert_eq!(both.minimap_style, MapStyle::Original);
        assert_eq!(ViewSettings::decode(&both.encode()), both);
    }

    #[test]
    fn minimap_shape_round_trips_and_defaults_to_square() {
        assert_eq!(ViewSettings::default().minimap_shape, MinimapShape::Square);
        let s = ViewSettings::decode("minimap_shape=Circular");
        assert_eq!(s.minimap_shape, MinimapShape::Circular);
        assert_eq!(
            ViewSettings::decode(&s.encode()).minimap_shape,
            MinimapShape::Circular
        );
        // Independent of the artwork style.
        assert_eq!(s.minimap_style, MapStyle::Realistic);
        assert_eq!(
            ViewSettings::decode("minimap_shape=round").minimap_shape,
            MinimapShape::Square
        );
    }

    #[test]
    fn an_unrecognised_map_style_keeps_the_default_instead_of_switching() {
        // Falling back to the live render is the safe direction: a typo must not
        // silently select a style the user did not ask for.
        let s = ViewSettings::decode("world_map_style=Photorealistic\nminimap_style=original");
        assert_eq!(s.world_map_style, MapStyle::Realistic);
        assert_eq!(s.minimap_style, MapStyle::Realistic);
    }

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
        s.client_tree_wind_preview = true;
        s.grass_distance_fade = false;
        assert_eq!(s, ViewSettings::decode(&s.encode()));
    }
    #[test]
    fn tree_wind_preview_defaults_off_and_persists() {
        assert!(!ViewSettings::default().client_tree_wind_preview);
        assert!(!ViewSettings::decode("tree_wind_strength=0.5").client_tree_wind_preview);
        let enabled = ViewSettings::decode("client_tree_wind_preview=true");
        assert!(enabled.client_tree_wind_preview);
        assert!(ViewSettings::decode(&enabled.encode()).client_tree_wind_preview);
        assert!(!ViewSettings::decode("client_tree_wind_preview=invalid").client_tree_wind_preview);
    }
    #[test]
    fn scenery_quality_preset_round_trips_and_is_bounded() {
        assert_eq!(ViewSettings::default().detail.scenery_quality, 2);
        let s = ViewSettings::decode("scenery_quality=4");
        assert_eq!(s.detail.scenery_quality, 4);
        assert_eq!(ViewSettings::decode(&s.encode()).detail.scenery_quality, 4);
        assert_eq!(
            ViewSettings::decode("scenery_quality=99")
                .detail
                .scenery_quality,
            5
        );
        assert_eq!(
            ViewSettings::decode("scenery_quality=nan")
                .detail
                .scenery_quality,
            2
        );
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
    #[test]
    fn area_center_lightmaps_default_on_but_explicit_legacy_setting_is_preserved() {
        assert!(ViewSettings::default().area_grass_lightmaps);
        assert!(ViewSettings::decode("unknown=ignored").area_grass_lightmaps);
        let s = ViewSettings::decode("area_grass_lightmaps=false");
        assert!(!s.area_grass_lightmaps);
        assert!(!ViewSettings::decode(&s.encode()).area_grass_lightmaps);
    }
    #[test]
    fn vegetation_authored_mips_defaults_off_and_round_trips() {
        assert!(!ViewSettings::default().vegetation_authored_mips);
        assert!(!ViewSettings::decode("unknown=ignored").vegetation_authored_mips);
        let s = ViewSettings::decode("vegetation_authored_mips=true");
        assert!(s.vegetation_authored_mips);
        assert_eq!(s, ViewSettings::decode(&s.encode()));
        assert!(!ViewSettings::decode("vegetation_authored_mips=invalid").vegetation_authored_mips);
    }
    #[test]
    fn special_glow_preview_is_explicit_persisted_and_bounded() {
        assert_eq!(ViewSettings::default().bloom_mode, 0);
        for mode in [4, 5] {
            let s = ViewSettings::decode(&format!("bloom_mode={mode}"));
            assert_eq!(s.bloom_mode, mode);
            assert_eq!(ViewSettings::decode(&s.encode()).bloom_mode, mode);
        }
        for invalid in ["3", "255", "-1", "bad"] {
            assert_eq!(
                ViewSettings::decode(&format!("bloom_mode={invalid}")).bloom_mode,
                0
            );
        }
    }
    #[test]
    fn glow_caller_color_and_independent_fades_round_trip_safely() {
        let s = ViewSettings::decode(
            "bloom_tint=0x8040a0e0\nbloom_brightness_fade=0.25\nbloom_opacity_fade=0.5",
        );
        assert_eq!(s.bloom_tint, 0x8040a0e0);
        assert_eq!(s.bloom_brightness_fade, 0.25);
        assert_eq!(s.bloom_opacity_fade, 0.5);
        assert_eq!(s, ViewSettings::decode(&s.encode()));
        let invalid = ViewSettings::decode(
            "bloom_tint=bogus\nbloom_brightness_fade=NaN\nbloom_opacity_fade=2",
        );
        assert_eq!(invalid.bloom_tint, 0xffffffff);
        assert_eq!(invalid.bloom_brightness_fade, 0.);
        assert_eq!(invalid.bloom_opacity_fade, 1.);
    }
    #[test]
    fn render_resolution_scale_is_persisted_and_bounded() {
        assert_eq!(ViewSettings::default().resolution_scale, 100.);
        for (text, expected) in [
            ("resolution_scale=10", 10.),
            ("resolution_scale=200", 200.),
            ("resolution_scale=63.5", 63.5),
        ] {
            let s = ViewSettings::decode(text);
            assert_eq!(s.resolution_scale, expected);
            assert_eq!(ViewSettings::decode(&s.encode()), s);
        }
        // Out-of-range, non-numeric and non-finite values are safe.
        for bad in ["0", "201", "-5", "NaN", "inf", "bogus"] {
            let s = ViewSettings::decode(&format!("resolution_scale={bad}"));
            assert!(
                (10. ..=200.).contains(&s.resolution_scale),
                "unexpected scale for {bad}: {}",
                s.resolution_scale
            );
        }
        assert_eq!(
            ViewSettings::decode("resolution_scale=0").resolution_scale,
            10.
        );
        assert_eq!(
            ViewSettings::decode("resolution_scale=999").resolution_scale,
            200.
        );
        assert_eq!(
            ViewSettings::decode("resolution_scale=NaN").resolution_scale,
            100.
        );
    }
    #[test]
    fn every_independent_post_effect_activates_the_renderer_chain() {
        // bloom, sharpen, underwater and the scene-chain ramp each allocate the
        // chain on their own; the screen-space fog diagnostic does not.
        for bits in 0..16 {
            let f = |i| bits & (1 << i) != 0;
            assert_eq!(super::post_required(f(0), f(1), f(2), f(3)), bits != 0);
        }
        assert!(super::post_required(false, true, false, false));
        assert!(!super::post_required(false, false, false, false));
    }
}
