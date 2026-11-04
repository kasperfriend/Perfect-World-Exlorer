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
            .default_size(egui::vec2(780., 650.))
            .min_size(egui::vec2(560., 480.))
            .collapsible(false)
            .resizable([true, true])
            .show(ctx, |ui| {
                // Reserve the header/footer first. The central scroll panes then
                // fill only the resized remainder instead of growing Resize back
                // to their content height after a downward drag.
                egui::TopBottomPanel::top("modules_intro")
                    .frame(egui::Frame::default())
                    .show_inside(ui, |ui| {
                        ui.label(
                            egui::RichText::new(
                                "Optional, independent add-ons. Select a module on the left to read its full description and controls.",
                            )
                            .small()
                            .weak(),
                        );
                        for error in &self.packages.errors {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        ui.add_space(8.);
                    });
                egui::TopBottomPanel::bottom("modules_back")
                    .frame(egui::Frame::default())
                    .show_inside(ui, |ui| {
                        ui.separator();
                        if ui.button("Back to menu").clicked() {
                            back = true;
                        }
                    });

                let mut selected = self.selected;
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show_inside(ui, |ui| {
                        self.module_browser_ui(ui, &mut selected, spawn, yaw);
                    });
                self.selected = selected;
            });
        back
    }

    fn module_browser_ui(
        &mut self,
        ui: &mut egui::Ui,
        selected: &mut usize,
        spawn: Vec3,
        yaw: f32,
    ) {
        let content_height = ui.available_height();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(190.);
                ui.heading("Installed modules");
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_source("installed_module_list")
                    .auto_shrink([false, false])
                    .max_height((content_height - 34.).max(80.))
                    .show(ui, |ui| {
                        for (index, installed) in
                            self.packages.packages.iter().enumerate()
                        {
                            let enabled =
                                installed.id == "walker" && self.walker.is_active();
                            let text = if enabled {
                                egui::RichText::new(format!("● {}", installed.name))
                                    .color(egui::Color32::LIGHT_GREEN)
                                    .strong()
                            } else {
                                egui::RichText::new(installed.name.as_str())
                            };
                            let width = ui.available_width();
                            if ui
                                .add_sized(
                                    [width, 28.],
                                    egui::SelectableLabel::new(*selected == index, text),
                                )
                                .clicked()
                            {
                                *selected = index;
                            }
                        }
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
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                let width = ui.available_width();
                ui.set_width(width);
                egui::ScrollArea::vertical()
                    .id_source("selected_module_description")
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .show(ui, |ui| {
                        if let Some(installed) =
                            self.packages.packages.get(*selected).cloned()
                        {
                            self.selected_module_ui(ui, installed, spawn, yaw);
                        } else {
                            ui.heading("Module details");
                            ui.label("Select a module on the left.");
                        }
                    });
            });
        });
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
                "While enabled: WASD moves, Space jumps (press again for the client's second jump), and the mouse looks around. Open Escape to use the integrated Character Designer and movement settings beneath the minimap. New characters start with native profession armor.",
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
            .default_pos(egui::pos2((screen_width - 406.).max(16.), 294.))
            .default_size(egui::vec2(390., 540.))
            .min_size(egui::vec2(300., 280.))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modules_window_bottom_edge_resizes_without_content_feedback() {
        let context = egui::Context::default();
        let mut modules = Modules::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 1000.));
        let mut frame = |events| {
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |context| {
                    modules.menu_ui(context, Vec3::ZERO, 0.);
                },
            );
        };
        // Let the default size and panel layout settle before grabbing the edge.
        frame(Vec::new());
        frame(Vec::new());
        let id = egui::Id::new("MODULES  |  ESC to go back");
        let before = context
            .memory(|memory| memory.area_rect(id))
            .expect("Modules window area");
        let start = egui::pos2(before.center().x, before.bottom() - 2.);
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ]);
        let end = start + egui::vec2(0., 100.);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(Vec::new());
        frame(vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }]);
        frame(Vec::new());
        let after = context
            .memory(|memory| memory.area_rect(id))
            .expect("resized Modules window area");
        assert!(
            after.height() >= before.height() + 70. && after.height() <= before.height() + 130.,
            "bottom drag should persist without content forcing the old height: {} -> {}",
            before.height(),
            after.height()
        );
    }
}
