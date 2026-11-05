//! Optional item browser. Its archives, textures and database are owned by the
//! module and are released when it is disabled.
mod archive;
mod compare;
mod database;
mod descriptions;
mod elements;
mod icons;
mod preview3d;

use archive::LibrarianAssets;
use database::{Category, Database};
use icons::IconCache;
use std::path::PathBuf;

pub struct Librarian {
    root: PathBuf,
    state: Option<State>,
    error: Option<String>,
    pending_free: Option<egui::TextureId>,
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
}
impl Librarian {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: None,
            error: None,
            pending_free: None,
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
                self.pending_free = state.preview.map(|preview| preview.texture_id);
            }
            self.error = None;
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
    ) {
        if let Some(id) = self.pending_free.take() {
            renderer.free_texture(&id);
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
                    "worn:{}:{name}",
                    if state.preview_female { "f" } else { "m" }
                )
            })
            .unwrap_or_else(|| item.model.clone());
        preview.update(device, queue, format, &mut state.assets, &path);
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.state else { return };
        egui::Window::new("Librarian")
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
                .selectable_label(!self.compare_open && self.category.is_none(), "All Items")
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
                .selectable_label(self.compare_open, "Comparison")
                .clicked()
            {
                self.compare_open = true;
            }
        });
        ui.separator();
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
                        && !item.name.to_lowercase().contains(&query)
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
                        if ui
                            .selectable_label(
                                self.selected == Some(index),
                                format!("{}  [{}]", item.name, item.id),
                            )
                            .clicked()
                        {
                            self.selected = Some(index);
                        }
                    }
                });
            egui::ScrollArea::vertical()
                .id_source("librarian_details")
                .show(&mut columns[1], |ui| {
                    if let Some(index) = self.selected {
                        let item = &self.database.items[index];
                        ui.heading(&item.name);
                        ui.label(format!("ID {} · {}", item.id, item.category.name()));
                        if let Some(color) = self.database.colors.get(&item.id) {
                            ui.label(format!("Client color category: {color}"));
                        }
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.show_3d, false, "2D icon");
                            ui.selectable_value(&mut self.show_3d, true, "3D model");
                        });
                        if self.show_3d {
                            if item.equipment_name().is_some() {
                                ui.horizontal(|ui| {
                                    ui.label("Blademaster mannequin:");
                                    ui.selectable_value(&mut self.preview_female, false, "Male");
                                    ui.selectable_value(&mut self.preview_female, true, "Female");
                                });
                            }
                            if let Some(preview) = &mut self.preview {
                                if let Some(error) = &preview.error {
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
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
                        if let Some(subtype) = item.subtype {
                            ui.label(format!("Subtype ID: {subtype}"));
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
                egui::Grid::new("librarian_comparison_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Attribute");
                        for (col, index) in self.comparison.iter().enumerate() {
                            let item = &self.database.items[*index];
                            ui.vertical(|ui| {
                                if ui.link(&item.name).clicked() {
                                    jump = Some(*index);
                                }
                                ui.horizontal(|ui| {
                                    if col > 0 && ui.small_button("←").clicked() {
                                        swap = Some((col, col - 1));
                                    }
                                    if col + 1 < self.comparison.len()
                                        && ui.small_button("→").clicked()
                                    {
                                        swap = Some((col, col + 1));
                                    }
                                    if ui.small_button("×").clicked() {
                                        remove = Some(col);
                                    }
                                });
                            });
                        }
                        ui.end_row();
                        let items: Vec<_> = self
                            .comparison
                            .iter()
                            .map(|index| &self.database.items[*index])
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
            });
        if let Some((a, b)) = swap {
            self.comparison.swap(a, b);
        }
        if let Some(col) = remove {
            self.comparison.remove(col);
        }
        if let Some(index) = jump {
            self.selected = Some(index);
            self.category = None;
            self.section = None;
            self.major = None;
            self.subtype = None;
            self.compare_open = false;
        }
    }
}
