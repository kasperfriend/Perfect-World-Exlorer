//! Optional item browser. Its archives, textures and database are owned by the
//! module and are released when it is disabled.
mod archive;
mod compare;
mod database;
mod elements;
mod icons;

use archive::LibrarianAssets;
use database::{Category, Database};
use icons::IconCache;
use std::path::PathBuf;

pub struct Librarian {
    root: PathBuf,
    state: Option<State>,
    error: Option<String>,
}
struct State {
    assets: LibrarianAssets,
    icons: IconCache,
    database: Database,
    category: Option<Category>,
    subtype: Option<u32>,
    search: String,
    selected: Option<usize>,
    comparison: Vec<usize>,
    compare_open: bool,
}
impl Librarian {
    pub fn new(root: PathBuf) -> Self {
        Self { root, state: None, error: None }
    }
    pub fn active(&self) -> bool { self.state.is_some() }
    pub fn status(&self) -> &str {
        if self.active() { "Enabled" } else { self.error.as_deref().unwrap_or("Disabled") }
    }
    pub fn set_active(&mut self, active: bool) {
        if !active { self.state = None; self.error = None; return; }
        let mut assets = LibrarianAssets::open(&self.root);
        match assets.read_elements_data().and_then(|bytes| Database::parse_v27(&bytes)) {
            Ok(database) => {
                let icons = IconCache::load(&mut assets);
                self.state = Some(State { assets, icons, database, category: None, subtype: None,
                    search: String::new(), selected: None, comparison: Vec::new(), compare_open: false });
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.state else { return };
        egui::Window::new("Librarian")
            .default_size([1050., 700.]).min_size([650., 400.]).resizable(true)
            .show(ctx, |ui| state.ui(ui));
    }
}
impl State {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!self.compare_open && self.category.is_none(), "All Items").clicked() {
                self.compare_open = false; self.category = None; self.subtype = None;
            }
            for category in Category::ALL {
                if ui.selectable_label(!self.compare_open && self.category == Some(category), category.name()).clicked() {
                    self.compare_open = false; self.category = Some(category); self.subtype = None;
                }
            }
            if ui.selectable_label(self.compare_open, "Comparison").clicked() { self.compare_open = true; }
        });
        ui.separator();
        if self.compare_open { self.comparison_ui(ui); return; }
        ui.horizontal(|ui| {
            ui.label("Search:"); ui.text_edit_singleline(&mut self.search);
            ui.label(format!("{} records", self.database.items.len()));
        });
        let query = self.search.to_lowercase();
        let indices: Vec<usize> = self.database.items.iter().enumerate().filter_map(|(index, item)| {
            if self.category.is_some_and(|category| category != item.category)
                || self.subtype.is_some_and(|subtype| item.subtype != Some(subtype))
                || (!query.is_empty() && !item.name.to_lowercase().contains(&query) && !item.id.to_string().contains(&query)) {
                None
            } else { Some(index) }
        }).collect();
        ui.label(format!("{} matching items", indices.len()));
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical().id_source("librarian_items").show_rows(&mut columns[0], 23., indices.len(), |ui, range| {
                for row in range {
                    let index = indices[row];
                    let item = &self.database.items[index];
                    if ui.selectable_label(self.selected == Some(index), format!("{}  [{}]", item.name, item.id)).clicked() {
                        self.selected = Some(index);
                    }
                }
            });
            egui::ScrollArea::vertical().id_source("librarian_details").show(&mut columns[1], |ui| {
                if let Some(index) = self.selected {
                    let item = &self.database.items[index];
                    ui.heading(&item.name);
                    ui.label(format!("ID {} · {}", item.id, item.category.name()));
                    if let Some(icon) = self.icons.get(ui.ctx(), &mut self.assets, &item.icon) {
                        ui.image((icon, egui::vec2(64.,64.)));
                    }
                    if ui.button("Add to comparison").clicked() && !self.comparison.contains(&index) {
                        self.comparison.push(index);
                    }
                    ui.separator();
                    for (name, value) in item.attributes() { ui.label(format!("{name}: {value}")); }
                    if let Some(subtype) = item.subtype { ui.label(format!("Subtype ID: {subtype}")); }
                    if !item.model.is_empty() { ui.label(format!("Model: {}", item.model)); }
                    ui.label("3D preview is not yet available.");
                } else { ui.label("Select an item to see its details."); }
            });
        });
    }
    fn comparison_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparison");
        if self.comparison.is_empty() { ui.label("Add items from their details to compare them."); return; }
        let mut remove = None;
        let mut jump = None;
        let mut swap = None;
        egui::ScrollArea::both().id_source("librarian_compare").show(ui, |ui| {
            egui::Grid::new("librarian_comparison_grid").striped(true).show(ui, |ui| {
                ui.strong("Attribute");
                for (col, index) in self.comparison.iter().enumerate() {
                    let item = &self.database.items[*index];
                    ui.vertical(|ui| {
                        if ui.link(&item.name).clicked() { jump = Some(*index); }
                        ui.horizontal(|ui| {
                            if col > 0 && ui.small_button("←").clicked() { swap = Some((col, col - 1)); }
                            if col + 1 < self.comparison.len() && ui.small_button("→").clicked() { swap = Some((col, col + 1)); }
                            if ui.small_button("×").clicked() { remove = Some(col); }
                        });
                    });
                }
                ui.end_row();
                let items: Vec<_> = self.comparison.iter().map(|index| &self.database.items[*index]).collect();
                for (name, values) in compare::rows(&items) {
                    ui.label(name);
                    let baseline = values.first().copied().flatten();
                    let best = values.iter().flatten().copied().reduce(f64::max);
                    for value in values {
                        match value {
                            Some(value) => {
                                let delta = baseline.map(|base| value - base).unwrap_or(0.);
                                let text = if delta == 0. { format!("{value}") } else { format!("{value} ({delta:+})") };
                                if best == Some(value) { ui.colored_label(egui::Color32::LIGHT_GREEN, text); }
                                else { ui.label(text); }
                            }
                            None => { ui.label("—"); }
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((a, b)) = swap { self.comparison.swap(a, b); }
        if let Some(col) = remove { self.comparison.remove(col); }
        if let Some(index) = jump { self.selected = Some(index); self.category = None; self.subtype = None; self.compare_open = false; }
    }
}
