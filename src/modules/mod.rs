//! Optional add-on registry. Modules own their settings, assets, simulation and
//! GPU resources. The host only supplies narrow input/camera, terrain and draw
//! hooks; a disabled module has no effect on core features.

mod designer;
mod package;
mod walker;

use crate::{model_draw_gpu, navigation::MapSpace};
use glam::Vec3;
use package::{Package, Registry};
use std::{collections::HashSet, path::PathBuf};
use walker::Walker;
use winit::keyboard::KeyCode;

pub struct Modules {
    walker: Walker,
    packages: Registry,
    selected: usize,
}

impl Default for Modules {
    fn default() -> Self {
        Self {
            walker: Walker::default(),
            packages: Registry::discover(package::default_root()),
            selected: 0,
        }
    }
}

impl Modules {
    pub fn new(element: PathBuf, map: MapSpace) -> Self {
        Self {
            walker: Walker::with_host(element, map),
            packages: Registry::discover(package::default_root()),
            selected: 0,
        }
    }

    pub fn walker_active(&self) -> bool {
        self.walker.is_active()
    }

    /// Returns true when the user asks to go back to the main Escape menu.
    pub fn menu_ui(&mut self, ctx: &egui::Context, spawn: Vec3, yaw: f32) -> bool {
        let mut back = false;
        egui::Window::new("MODULES  |  ESC to go back")
            .default_pos(egui::pos2(16., 16.))
            .default_size(egui::vec2(500., 390.))
            .min_size(egui::vec2(350., 280.))
            .collapsible(false)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Optional, independent add-ons. Enable only what you want; core features and settings remain unchanged.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(8.);
                ui.heading("Installed modules");
                let mut selected = self.selected;
                for (index, installed) in self.packages.packages.iter().enumerate() {
                    if ui
                        .selectable_label(selected == index, &installed.name)
                        .clicked()
                    {
                        selected = index;
                    }
                }
                self.selected = selected;
                if self.packages.packages.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "No module packages found in {}.",
                            self.packages.root.display()
                        ))
                        .small()
                        .weak(),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Keep the shipped Modules folder beside Perfect-World-Explorer.exe.",
                        )
                        .small()
                        .weak(),
                    );
                }
                for error in &self.packages.errors {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                ui.separator();
                if let Some(installed) = self.packages.packages.get(self.selected).cloned() {
                    self.selected_module_ui(ui, installed, spawn, yaw);
                }
                ui.separator();
                if ui.button("Back to menu").clicked() {
                    back = true;
                }
            });
        back
    }

    fn selected_module_ui(&mut self, ui: &mut egui::Ui, installed: Package, spawn: Vec3, yaw: f32) {
        ui.heading(&installed.name);
        ui.label(
            egui::RichText::new(format!("Version {}", installed.version))
                .small()
                .weak(),
        );
        ui.label(&installed.description);
        ui.label(
            egui::RichText::new(format!("Package: {}", installed.directory.display()))
                .small()
                .weak(),
        );
        ui.add_space(8.);
        if installed.id != "walker" {
            ui.label(
                egui::RichText::new(format!(
                    "This executable does not provide an implementation for module id '{}'.",
                    installed.id
                ))
                .small()
                .weak(),
            );
            return;
        }

        let active = self.walker.is_active();
        let label = if active {
            "Disable Walker"
        } else {
            "Enable Walker"
        };
        if ui
            .add_sized([160., 32.], egui::Button::new(label))
            .clicked()
        {
            let facing = Vec3::new(yaw.sin(), 0., -yaw.cos());
            self.walker.set_active(!active, spawn, facing);
        }
        let status = self.walker.status();
        if active {
            ui.label(egui::RichText::new(status).color(egui::Color32::LIGHT_GREEN));
        } else if status == "Disabled" {
            ui.label(egui::RichText::new(status).weak());
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, status);
        }
        ui.label(
            egui::RichText::new(
                "While enabled: WASD moves, Space jumps (press again for the client's second jump), and the mouse looks around. Open Escape to adjust Walker beneath the minimap.",
            )
            .small()
            .weak(),
        );
    }

    /// Escape-menu-owned panel. egui Window supplies persistent dragging and
    /// resizing; its initial position is directly beneath the 256 px minimap.
    pub fn settings_panel(&mut self, ctx: &egui::Context, screen_width: f32) {
        if !self.walker.is_active() {
            return;
        }
        egui::Window::new("Walker settings")
            .default_pos(egui::pos2((screen_width - 306.).max(16.), 294.))
            .default_size(egui::vec2(290., 250.))
            .min_size(egui::vec2(245., 190.))
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| self.walker.settings_ui(ui));
    }

    pub fn update(
        &mut self,
        delta: f32,
        input_enabled: bool,
        keys: &HashSet<KeyCode>,
        yaw: f32,
        pitch: f32,
    ) -> Option<Vec3> {
        self.walker.update(delta, input_enabled, keys, yaw, pitch)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame: model_draw_gpu::Frame,
    ) {
        self.walker
            .render(device, queue, format, encoder, target, depth, frame);
    }
}
