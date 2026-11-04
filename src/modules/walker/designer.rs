//! Walker-owned character-design state. Applying an edit returns one design for
//! the Walker to assemble in place; there is no separate preview character.

use super::character::starter_design;
use crate::modules::designer::{
    assets::{Assets, UserPreset},
    catalog::{Class, Gender},
    controls,
    customize::CharacterDesign,
};

pub(super) struct Designer {
    design: CharacterDesign,
    initialized: bool,
    control_state: controls::ControlState,
    preset_name: String,
    user_presets: Vec<UserPreset>,
    user_presets_for: Option<(Class, Gender)>,
    status: String,
    status_error: bool,
}

impl Default for Designer {
    fn default() -> Self {
        Self {
            design: CharacterDesign::new(Class::Blademaster, Gender::Male),
            initialized: false,
            control_state: controls::ControlState::default(),
            preset_name: String::new(),
            user_presets: Vec::new(),
            user_presets_for: None,
            status: "Native profession starter armor will be equipped on spawn.".into(),
            status_error: false,
        }
    }
}

impl Designer {
    pub(super) fn ensure_initialized(
        &mut self,
        assets: &mut Assets,
    ) -> Result<CharacterDesign, String> {
        if !self.initialized {
            self.design = starter_design(assets, self.design.class, self.design.gender)?;
            self.initialized = true;
            self.status = "Native profession starter armor equipped.".into();
            self.status_error = false;
        }
        Ok(self.design.clone())
    }

    pub(super) fn restore_applied(&mut self, design: CharacterDesign) {
        self.design = design;
        self.initialized = true;
    }

    pub(super) fn rebuild_succeeded(&mut self, equipment: usize, warnings: usize) {
        self.status = format!(
            "Applied to this Walker — {equipment} equipment skin(s), {warnings} asset warning(s)."
        );
        self.status_error = false;
    }

    pub(super) fn rebuild_failed(&mut self, error: &str) {
        self.status =
            format!("Could not customize Walker: {error}. The previous character is still active.");
        self.status_error = true;
    }

    fn load_default(
        &mut self,
        assets: &mut Assets,
        class: Class,
        gender: Gender,
    ) -> Option<CharacterDesign> {
        match starter_design(assets, class, gender) {
            Ok(design) => {
                self.design = design;
                self.initialized = true;
                self.status_error = false;
                Some(self.design.clone())
            }
            Err(error) => {
                self.status = format!("Could not load native default: {error}");
                self.status_error = true;
                None
            }
        }
    }

    /// Show the Designer inside Walker settings. A returned design is the only
    /// rebuild request, and the Walker applies it to its current character.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, assets: &mut Assets) -> Option<CharacterDesign> {
        ui.heading("Character Designer");
        ui.label(
            egui::RichText::new(
                "Customize this Walker character; every change applies immediately. New and reset characters use native profession starter armor.",
            )
            .small()
            .weak(),
        );

        let mut class = self.design.class;
        egui::ComboBox::from_label("Profession")
            .selected_text(class.name())
            .show_ui(ui, |ui| {
                for choice in Class::ALL {
                    ui.selectable_value(&mut class, choice, choice.name());
                }
            });
        let mut gender = self.design.gender;
        egui::ComboBox::from_label("Gender")
            .selected_text(gender.name())
            .show_ui(ui, |ui| {
                for choice in Gender::ALL {
                    if class.allows(choice) {
                        ui.selectable_value(&mut gender, choice, choice.name());
                    }
                }
            });
        if !class.allows(gender) {
            gender = class.default_gender();
        }
        let mut rebuild = if class != self.design.class || gender != self.design.gender {
            self.load_default(assets, class, gender)
        } else {
            None
        };

        let mut selected_preset = None;
        egui::ComboBox::from_label("Native preset")
            .selected_text("Choose a preset to apply")
            .show_ui(ui, |ui| {
                for preset in assets
                    .catalog
                    .preset_options(self.design.class, self.design.gender)
                {
                    if ui
                        .selectable_label(false, format!("{} — {}", preset.id, preset.name))
                        .clicked()
                    {
                        selected_preset = Some(preset.path.clone());
                    }
                }
            });
        if let Some(path) = selected_preset {
            let mut design = self.design.clone();
            match assets.apply_preset(&mut design, &path) {
                Ok(()) => {
                    self.design = design;
                    self.status_error = false;
                    rebuild = Some(self.design.clone());
                }
                Err(error) => {
                    self.status = format!("Could not apply preset {path}: {error}");
                    self.status_error = true;
                }
            }
        }

        let preset_class = self.design.class;
        let preset_gender = self.design.gender;
        if self.user_presets_for != Some((preset_class, preset_gender)) {
            match assets.user_presets(preset_class, preset_gender) {
                Ok(presets) => {
                    self.user_presets = presets;
                    self.user_presets_for = Some((preset_class, preset_gender));
                }
                Err(error) => {
                    self.user_presets.clear();
                    self.user_presets_for = Some((preset_class, preset_gender));
                    self.status = format!("Could not list saved presets: {error}");
                    self.status_error = true;
                }
            }
        }
        let mut selected_user_preset = None;
        egui::ComboBox::from_label("Saved preset")
            .selected_text(if self.user_presets.is_empty() {
                "No saved presets for this character"
            } else {
                "Choose a saved preset to apply"
            })
            .show_ui(ui, |ui| {
                for preset in &self.user_presets {
                    if ui.selectable_label(false, &preset.name).clicked() {
                        selected_user_preset = Some(preset.path.clone());
                    }
                }
            });
        if let Some(path) = selected_user_preset {
            let mut design = self.design.clone();
            match assets.apply_user_preset(&mut design, &path) {
                Ok(()) => {
                    self.design = design;
                    self.status_error = false;
                    rebuild = Some(self.design.clone());
                }
                Err(error) => {
                    self.status =
                        format!("Could not apply saved preset {}: {error}", path.display());
                    self.status_error = true;
                }
            }
        }

        if controls::show(
            ui,
            &mut self.design,
            &assets.catalog,
            &assets.bounds,
            &mut self.control_state,
        ) {
            rebuild = Some(self.design.clone());
        }

        if ui.button("Reset class and starter armor").clicked() {
            rebuild = self.load_default(assets, self.design.class, self.design.gender);
        }
        ui.horizontal(|ui| {
            ui.label("Preset name");
            ui.text_edit_singleline(&mut self.preset_name);
            if ui.button("Save new preset").clicked() {
                match assets.save_preset(&self.design, &self.preset_name) {
                    Ok(path) => {
                        self.status = format!("Saved {}", path.display());
                        self.status_error = false;
                        self.user_presets_for = None;
                    }
                    Err(error) => {
                        self.status = format!("Could not save preset: {error}");
                        self.status_error = true;
                    }
                }
            }
        });
        if self.status_error {
            ui.colored_label(egui::Color32::LIGHT_RED, &self.status);
        } else {
            ui.label(egui::RichText::new(&self.status).small().weak());
        }
        rebuild
    }
}
