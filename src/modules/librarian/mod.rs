//! Optional item browser. Its archives, textures and database are owned by the
//! module and are released when it is disabled.
mod archive;
mod compare;
mod database;
mod descriptions;
mod elements;
mod icons;
mod preview3d;

use crate::modules::designer::catalog::{Class, Gender};
use archive::LibrarianAssets;
use database::{Category, Database};
use icons::IconCache;
use std::path::PathBuf;

pub struct Librarian {
    root: PathBuf,
    state: Option<State>,
    error: Option<String>,
    pending_free: Vec<egui::TextureId>,
}
struct State {
    assets: LibrarianAssets,
    icons: IconCache,
    database: Database,
    category: Option<Category>,
    section: Option<usize>,
    major: Option<(usize, u32)>,
    subtype: Option<(usize, u32)>,
    search: String,
    selected: Option<usize>,
    comparison: Vec<usize>,
    compare_open: bool,
    preview: Option<preview3d::Preview>,
    show_3d: bool,
    preview_female: bool,
    preview_class: Class,
}
impl Librarian {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: None,
            error: None,
            pending_free: Vec::new(),
        }
    }
    pub fn active(&self) -> bool {
        self.state.is_some()
    }
    pub fn status(&self) -> &str {
        if self.active() {
            "Enabled"
        } else {
            self.error.as_deref().unwrap_or("Disabled")
        }
    }
    pub fn set_active(&mut self, active: bool) {
        if !active {
            if let Some(state) = self.state.take() {
                if let Some(preview) = state.preview {
                    self.pending_free.push(preview.texture_id);
                }
            }
            self.error = None;
            return;
        }
        if self.active() {
            return;
        }
        let mut assets = LibrarianAssets::open(&self.root);
        match assets
            .read_elements_data()
            .and_then(|bytes| Database::parse_v27(&bytes))
        {
            Ok(mut database) => {
                for name in ["configs/item_desc.txt", "configs/item_ext_desc.txt"] {
                    if let Some(bytes) = assets.read_config(name) {
                        database
                            .descriptions
                            .extend(descriptions::keyed_strings(&bytes));
                    }
                }
                if let Some(bytes) = assets.read_config("configs/item_ext_prop.txt") {
                    database.addon_types = descriptions::addon_types(&bytes);
                }
                if let Some(bytes) = assets.read_config("configs/item_color.txt") {
                    database.colors = descriptions::numeric_pairs(&bytes);
                }
                if let Some(bytes) = assets.read_config("configs/skillstr.txt") {
                    database.skill_names = descriptions::keyed_strings(&bytes);
                }
                let icons = IconCache::load(&mut assets);
                self.state = Some(State {
                    assets,
                    icons,
                    database,
                    category: None,
                    section: None,
                    major: None,
                    subtype: None,
                    search: String::new(),
                    selected: None,
                    comparison: Vec::new(),
                    compare_open: false,
                    preview: None,
                    show_3d: false,
                    preview_female: false,
                    preview_class: Class::Blademaster,
                });
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn prepare_preview(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
        format: wgpu::TextureFormat,
        visible: bool,
    ) {
        for id in self.pending_free.drain(..) {
            renderer.free_texture(&id);
        }
        if !visible {
            return;
        }
        let Some(state) = &mut self.state else { return };
        if !state.show_3d {
            return;
        }
        let Some(index) = state.selected else { return };
        let preview = state
            .preview
            .get_or_insert_with(|| preview3d::Preview::new(device, renderer, format));
        let item = &state.database.items[index];
        let path = item
            .equipment_name()
            .map(|name| {
                format!(
                    "worn:{}:{}:{name}",
                    state.preview_class.index(),
                    if state.preview_female { "f" } else { "m" }
                )
            })
            .unwrap_or_else(|| item.model.clone());
        preview.update(device, queue, format, &mut state.assets, &path);
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.state else { return };
        egui::Window::new("Librarian")
            .default_pos(egui::pos2(320., 70.))
            .default_size([1050., 700.])
            .min_size([650., 400.])
            .resizable(true)
            .show(ctx, |ui| state.ui(ui));
    }
}
impl State {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(egui::Button::new(egui::RichText::new("All Items").strong().color(egui::Color32::WHITE)).fill(if !self.compare_open && self.category.is_none() { egui::Color32::from_rgb(50, 113, 165) } else { egui::Color32::from_rgb(40, 65, 90) }))
                .clicked()
            {
                self.compare_open = false;
                self.category = None;
                self.section = None;
                self.major = None;
                self.subtype = None;
            }
            for category in Category::ALL {
                if ui
                    .selectable_label(
                        !self.compare_open && self.category == Some(category),
                        category.name(),
                    )
                    .clicked()
                {
                    self.compare_open = false;
                    self.category = Some(category);
                    self.section = None;
                    self.major = None;
                    self.subtype = None;
                }
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("Comparison").strong().color(egui::Color32::WHITE)).fill(if self.compare_open { egui::Color32::from_rgb(158, 104, 35) } else { egui::Color32::from_rgb(95, 68, 33) }))
                .clicked()
            {
                self.compare_open = true;
            }
        });
        ui.separator();
        if !self.icons.has_atlas() {
            ui.colored_label(egui::Color32::YELLOW, "Inventory icon atlases could not be loaded from surfaces.pck; loose icons may still be available.");
        }
        if self.compare_open {
            self.comparison_ui(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search);
            ui.label(format!("{} records", self.database.items.len()));
        });
        if let Some(category) = self.category {
            let mut sections: Vec<usize> = self
                .database
                .items
                .iter()
                .filter(|item| item.category == category)
                .map(|item| item.section)
                .collect();
            sections.sort_unstable();
            sections.dedup();
            if sections.len() > 1 {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(self.section.is_none(), "All types")
                        .clicked()
                    {
                        self.section = None;
                        self.major = None;
                        self.subtype = None;
                    }
                    for section in sections {
                        if ui
                            .selectable_label(
                                self.section == Some(section),
                                database::section_name(section),
                            )
                            .clicked()
                        {
                            self.section = Some(section);
                            self.major = None;
                            self.subtype = None;
                        }
                    }
                });
            }
            let mut majors: Vec<((usize, u32), String)> = self
                .database
                .items
                .iter()
                .filter(|item| {
                    item.category == category
                        && self.section.map_or(true, |section| section == item.section)
                })
                .filter_map(|item| {
                    item.major.map(|id| {
                        let name = database::major_table(item.section)
                            .and_then(|table| self.database.category_names.get(&(table, id)))
                            .filter(|name| !name.is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("Type {id}"));
                        ((item.section, id), name)
                    })
                })
                .collect();
            majors.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            majors.dedup_by_key(|entry| entry.0);
            if !majors.is_empty() {
                egui::ComboBox::from_label("Type")
                    .selected_text(
                        self.major
                            .and_then(|id| majors.iter().find(|entry| entry.0 == id))
                            .map(|entry| entry.1.as_str())
                            .unwrap_or("All types"),
                    )
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut self.major, None, "All types")
                            .changed()
                        {
                            self.subtype = None;
                        }
                        for (id, name) in &majors {
                            if ui
                                .selectable_value(&mut self.major, Some(*id), name)
                                .changed()
                            {
                                self.subtype = None;
                            }
                        }
                    });
            }
            let mut subtypes: Vec<((usize, u32), String)> = self
                .database
                .items
                .iter()
                .filter(|item| {
                    item.category == category
                        && self.section.map_or(true, |section| section == item.section)
                        && self.major.map_or(true, |(section, id)| {
                            item.section == section && item.major == Some(id)
                        })
                })
                .filter_map(|item| {
                    item.subtype.map(|id| {
                        let name = database::subtype_table(item.section)
                            .and_then(|table| self.database.category_names.get(&(table, id)))
                            .filter(|name| !name.is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("Subtype {id}"));
                        ((item.section, id), name)
                    })
                })
                .collect();
            subtypes.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            subtypes.dedup_by_key(|entry| entry.0);
            if !subtypes.is_empty() {
                egui::ComboBox::from_label("Subtype")
                    .selected_text(
                        self.subtype
                            .and_then(|id| subtypes.iter().find(|entry| entry.0 == id))
                            .map(|entry| entry.1.as_str())
                            .unwrap_or("All subtypes"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.subtype, None, "All subtypes");
                        for (id, name) in &subtypes {
                            ui.selectable_value(&mut self.subtype, Some(*id), name);
                        }
                    });
            }
        }
        let query = self.search.to_lowercase();
        let indices: Vec<usize> = self
            .database
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if self
                    .category
                    .is_some_and(|category| category != item.category)
                    || self.section.is_some_and(|section| item.section != section)
                    || self.major.is_some_and(|(section, id)| {
                        item.section != section || item.major != Some(id)
                    })
                    || self.subtype.is_some_and(|(section, id)| {
                        item.section != section || item.subtype != Some(id)
                    })
                    || (!query.is_empty()
                        && !item.search_name.contains(&query)
                        && !item.id.to_string().contains(&query))
                {
                    None
                } else {
                    Some(index)
                }
            })
            .collect();
        ui.label(format!("{} matching items", indices.len()));
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical()
                .id_source("librarian_items")
                .show_rows(&mut columns[0], 23., indices.len(), |ui, range| {
                    for row in range {
                        let index = indices[row];
                        let item = &self.database.items[index];
                        ui.horizontal(|ui| {
                            if let Some(icon) =
                                self.icons.get(ui.ctx(), &mut self.assets, &item.icon)
                            {
                                ui.image((icon, egui::vec2(20., 20.)));
                            } else {
                                ui.allocate_space(egui::vec2(20., 20.));
                            }
                            if ui
                                .selectable_label(
                                    self.selected == Some(index),
                                    format!("{}  [{}]", item.name, item.id),
                                )
                                .clicked()
                            {
                                self.selected = Some(index);
                                if let Some(gender) = item.fashion_gender() {
                                    self.preview_female = gender == 1;
                                }
                            }
                        });
                    }
                });
            egui::ScrollArea::vertical()
                .id_source("librarian_details")
                .show(&mut columns[1], |ui| {
                    if let Some(index) = self.selected {
                        let item = &self.database.items[index];
                        ui.heading(&item.name);
                        ui.label(format!("ID {} · {}", item.id, item.category.name()));
                        if let Some(mask) = item.class_mask() {
                            let classes = Class::ALL
                                .iter()
                                .filter(|class| mask & class.bit() != 0)
                                .map(|class| class.name())
                                .collect::<Vec<_>>()
                                .join(", ");
                            ui.label(if classes.is_empty() {
                                format!("Class mask: {mask:#x}")
                            } else {
                                format!("Classes: {classes}")
                            });
                        }
                        if let Some(gender) = item.fashion_gender() {
                            ui.label(format!(
                                "Fashion gender: {}",
                                match gender {
                                    0 => "Male",
                                    1 => "Female",
                                    _ => "Unknown",
                                }
                            ));
                        }
                        if let Some(color) = self.database.colors.get(&item.id) {
                            ui.label(format!("Client color category: {color}"));
                        }
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.show_3d, false, "2D icon");
                            ui.selectable_value(&mut self.show_3d, true, "3D model");
                        });
                        if self.show_3d {
                            if item.equipment_name().is_some() {
                                egui::ComboBox::from_label("Equipment class variant")
                                    .selected_text(self.preview_class.name())
                                    .show_ui(ui, |ui| {
                                        for class in Class::ALL {
                                            ui.selectable_value(
                                                &mut self.preview_class,
                                                class,
                                                class.name(),
                                            );
                                        }
                                    });
                                let gender = if self.preview_female {
                                    Gender::Female
                                } else {
                                    Gender::Male
                                };
                                if !self.preview_class.allows(gender) {
                                    self.preview_female =
                                        self.preview_class.default_gender() == Gender::Female;
                                }
                                ui.horizontal(|ui| {
                                    ui.label("Equipment gender variant:");
                                    if self.preview_class.allows(Gender::Male) {
                                        ui.selectable_value(
                                            &mut self.preview_female,
                                            false,
                                            "Male",
                                        );
                                    }
                                    if self.preview_class.allows(Gender::Female) {
                                        ui.selectable_value(
                                            &mut self.preview_female,
                                            true,
                                            "Female",
                                        );
                                    }
                                });
                            }
                            if let Some(preview) = &mut self.preview {
                                if let Some(error) = &preview.error {
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                                }
                                if let Some(error) = &preview.animation_error {
                                    ui.colored_label(egui::Color32::YELLOW, error);
                                }
                                for warning in &preview.warnings {
                                    ui.colored_label(egui::Color32::YELLOW, warning);
                                }
                                let size = ui.available_width().min(300.);
                                let response = ui.add(
                                    egui::Image::new((preview.texture_id, egui::vec2(size, size)))
                                        .sense(egui::Sense::drag()),
                                );
                                if response.dragged() {
                                    let (delta, pan) =
                                        ui.input(|i| (i.pointer.delta(), i.modifiers.shift));
                                    if pan {
                                        preview.pan_by(delta);
                                    } else {
                                        preview.yaw += delta.x * 0.01;
                                        preview.pitch =
                                            (preview.pitch - delta.y * 0.01).clamp(-1.45, 1.45);
                                    }
                                }
                                if response.hovered() {
                                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                                    preview.zoom =
                                        (preview.zoom * (-scroll * 0.001).exp()).clamp(0.3, 5.);
                                }
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut preview.turntable, "Turntable");
                                    ui.checkbox(&mut preview.playing, "Animation");
                                });
                                let actions = preview.action_names();
                                if !actions.is_empty() {
                                    egui::ComboBox::from_label("Action")
                                        .selected_text(
                                            actions
                                                .get(preview.action)
                                                .map(String::as_str)
                                                .unwrap_or("Bind pose"),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (index, name) in actions.iter().enumerate() {
                                                ui.selectable_value(
                                                    &mut preview.action,
                                                    index,
                                                    name,
                                                );
                                            }
                                        });
                                }
                                if preview.playing || preview.turntable {
                                    ui.ctx().request_repaint();
                                }
                                ui.label("Drag to orbit; Shift-drag to pan; scroll to zoom.");
                            } else {
                                ui.label("Preparing preview…");
                            }
                        } else if let Some(icon) =
                            self.icons.get(ui.ctx(), &mut self.assets, &item.icon)
                        {
                            ui.image((icon, egui::vec2(64., 64.)));
                        }
                        if ui.button("Add to comparison").clicked()
                            && !self.comparison.contains(&index)
                        {
                            self.comparison.push(index);
                        }
                        ui.separator();
                        if let Some(description) = self.database.descriptions.get(&item.id) {
                            ui.label(descriptions::plain_text(description));
                            ui.separator();
                        }
                        for (name, value) in item.attributes() {
                            ui.label(format!("{name}: {value}"));
                        }
                        if let Some(major) = item.major {
                            let name = database::major_table(item.section)
                                .and_then(|table| self.database.category_names.get(&(table, major)))
                                .map(String::as_str)
                                .unwrap_or("Unknown");
                            ui.label(format!("Type: {name} ({major})"));
                        }
                        if let Some(subtype) = item.subtype {
                            let name = database::subtype_table(item.section)
                                .and_then(|table| {
                                    self.database.category_names.get(&(table, subtype))
                                })
                                .map(String::as_str)
                                .unwrap_or("Unknown");
                            ui.label(format!("Subtype: {name} ({subtype})"));
                        }
                        if !item.model.is_empty() {
                            ui.label(format!("Model: {}", item.model));
                        }
                        if let Some((name, kind, level, speed)) = self.database.pet_info(item) {
                            ui.separator();
                            ui.strong("Pet or mount");
                            ui.label(format!("{name} · {kind}"));
                            ui.label(format!("Required level: {level}"));
                            if speed.is_finite() {
                                ui.label(format!("Base speed: {speed:.2}"));
                            }
                        }
                        if let Some(id) = item.contained_item() {
                            if let Some(&target) = self.database.by_id.get(&id) {
                                if ui
                                    .link(format!("Contains: {}", self.database.items[target].name))
                                    .clicked()
                                {
                                    self.selected = Some(target);
                                }
                            }
                        }
                        let fixed_addons = item.fixed_addons();
                        if !fixed_addons.is_empty() {
                            ui.separator();
                            ui.strong("Item bonuses");
                            for (group, id) in fixed_addons {
                                ui.label(format!("{group}: {}", self.database.addon_label(id)));
                            }
                        }
                        let skills = item.pet_skills();
                        if !skills.is_empty() {
                            ui.separator();
                            ui.strong("Pet skills");
                            for (id, level) in skills {
                                let name = self
                                    .database
                                    .skill_names
                                    .get(&id.saturating_mul(10))
                                    .map(|name| descriptions::plain_text(name))
                                    .unwrap_or_else(|| format!("Skill {id}"));
                                ui.label(format!("{name} · level {level}"));
                            }
                        }
                        for (group, addons) in item.addon_groups() {
                            ui.separator();
                            ui.strong(format!("{group} add-on pool"));
                            for (addon_id, weight) in addons {
                                let weight = if weight.is_finite() {
                                    format!(" (weight {weight:.3})")
                                } else {
                                    String::new()
                                };
                                ui.label(format!(
                                    "{}{weight}",
                                    self.database.addon_label(addon_id)
                                ));
                            }
                        }
                        for (suite_id, name, members) in self.database.suites_for(item.id) {
                            ui.separator();
                            ui.strong(format!("Set: {name}"));
                            for member in members {
                                if let Some(&target) = self.database.by_id.get(&member) {
                                    if ui.link(&self.database.items[target].name).clicked() {
                                        self.selected = Some(target);
                                    }
                                } else {
                                    ui.label(format!("Item {member}"));
                                }
                            }
                            let bonuses = self.database.suite_addons(suite_id);
                            if !bonuses.is_empty() {
                                ui.label("Set add-ons:");
                                for bonus in bonuses {
                                    ui.label(bonus);
                                }
                            }
                        }
                    } else {
                        ui.label("Select an item to see its details.");
                    }
                });
        });
    }
    fn comparison_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparison");
        if self.comparison.is_empty() {
            ui.label("Add items from their details to compare them.");
            return;
        }
        let mut remove = None;
        let mut jump = None;
        let mut swap = None;
        egui::ScrollArea::both()
            .id_source("librarian_compare")
            .show(ui, |ui| {
                for category in Category::ALL {
                    let columns = compare::category_columns(
                        &self.database.items, &self.comparison, category,
                    );
                    if columns.is_empty() { continue; }
                    ui.heading(category.name());
                    ui.label("Comparisons and baseline are limited to this category.");
                    egui::Grid::new(("librarian_comparison_grid", category.name()))
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Attribute");
                        for (group_col, &col) in columns.iter().enumerate() {
                            let index = self.comparison[col];
                            let item = &self.database.items[index];
                            ui.vertical(|ui| {
                                if ui.link(&item.name).clicked() {
                                    jump = Some(index);
                                }
                                ui.horizontal(|ui| {
                                    if group_col > 0 && ui.small_button("←").clicked() {
                                        swap = Some((col, columns[group_col - 1]));
                                    }
                                    if group_col + 1 < columns.len()
                                        && ui.small_button("→").clicked()
                                    {
                                        swap = Some((col, columns[group_col + 1]));
                                    }
                                    if ui.small_button("×").clicked() {
                                        remove = Some(col);
                                    }
                                });
                            });
                        }
                        ui.end_row();
                        let items: Vec<_> = columns
                            .iter()
                            .map(|&col| &self.database.items[self.comparison[col]])
                            .collect();
                        for (name, values) in compare::rows(&items) {
                            ui.label(name);
                            let baseline = values.first().copied().flatten();
                            let best = compare::best(name, &values);
                            for value in values {
                                match value {
                                    Some(value) => {
                                        let delta = baseline.map(|base| value - base).unwrap_or(0.);
                                        let format_number = |n: f64| {
                                            if n.fract() == 0. {
                                                format!("{n:.0}")
                                            } else {
                                                format!("{n:.2}")
                                            }
                                        };
                                        let text = if delta == 0. {
                                            format_number(value)
                                        } else {
                                            format!("{} ({:+.2})", format_number(value), delta)
                                        };
                                        if best == Some(value) {
                                            ui.colored_label(egui::Color32::LIGHT_GREEN, text);
                                        } else {
                                            ui.label(text);
                                        }
                                    }
                                    None => {
                                        ui.label("—");
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
                    ui.separator();
                }
            });
        if let Some((a, b)) = swap {
            self.comparison.swap(a, b);
        }
        if let Some(col) = remove {
            self.comparison.remove(col);
        }
        if let Some(index) = jump {
            self.selected = Some(index);
            if let Some(gender) = self.database.items[index].fashion_gender() {
                self.preview_female = gender == 1;
            }
            self.category = None;
            self.section = None;
            self.major = None;
            self.subtype = None;
            self.compare_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_and_comparison_render_without_optional_archives() {
        let mut record = vec![0; 1412];
        record[..4].copy_from_slice(&42u32.to_le_bytes());
        for (index, unit) in "Test weapon".encode_utf16().enumerate() {
            record[12 + 2 * index..14 + 2 * index].copy_from_slice(&unit.to_le_bytes());
        }
        let database = Database::parse_v27(&elements::synthetic_file(3, &record)).unwrap();
        let mut assets = LibrarianAssets::open(std::path::Path::new("/nonexistent-librarian-test"));
        let icons = IconCache::load(&mut assets);
        let mut state = State {
            assets,
            icons,
            database,
            category: Some(Category::Weapons),
            section: None,
            major: None,
            subtype: None,
            search: String::new(),
            selected: Some(0),
            comparison: vec![0],
            compare_open: false,
            preview: None,
            show_3d: false,
            preview_female: false,
            preview_class: Class::Blademaster,
        };
        let ctx = egui::Context::default();
        let run = |state: &mut State| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 800.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| state.ui(ui));
                },
            );
        };
        run(&mut state);
        state.compare_open = true;
        run(&mut state);
        assert_eq!(state.database.items[state.comparison[0]].id, 42);
    }
}
