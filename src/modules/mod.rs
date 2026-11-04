//! Optional add-on registry. Modules own their settings, assets, simulation and
//! GPU resources. The host only supplies narrow input/camera, terrain and draw
//! hooks; a disabled module has no effect on core features.

mod walker;

use crate::{model_draw_gpu, navigation::MapSpace};
use glam::Vec3;
use std::{collections::HashSet, path::PathBuf};
use walker::Walker;
use winit::keyboard::KeyCode;

struct Descriptor {
    name: &'static str,
    description: &'static str,
}

const MODULES: &[Descriptor] = &[Descriptor {
    name: "Walker",
    description: "Spawn a real, animated player character from models.pck and walk on the loaded terrain. Includes four race/profession models, source-backed run/jump mechanics, adjustable move speed and jump height, third-person view, and a bone-positioned first-person view. Walker uses terrain support, not flight; it does not change the core camera, world, interface, or persisted settings.",
}];

pub struct Modules {
    walker: Walker,
    selected: usize,
}

impl Default for Modules {
    fn default() -> Self {
        Self {
            walker: Walker::default(),
            selected: 0,
        }
    }
}

impl Modules {
    pub fn new(element: PathBuf, map: MapSpace) -> Self {
        Self {
            walker: Walker::with_host(element, map),
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
                ui.heading("Installed and future modules");
                for (index, descriptor) in MODULES.iter().enumerate() {
                    if ui
                        .selectable_label(self.selected == index, descriptor.name)
                        .clicked()
                    {
                        self.selected = index;
                    }
                }
                ui.label(
                    egui::RichText::new("Future modules will appear in this list as they are registered.")
                        .small()
                        .weak(),
                );
                ui.separator();
                if let Some(descriptor) = MODULES.get(self.selected) {
                    ui.heading(descriptor.name);
                    ui.label(descriptor.description);
                    ui.add_space(8.);
                    if self.selected == 0 {
                        let active = self.walker.is_active();
                        let label = if active { "Disable Walker" } else { "Enable Walker" };
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
                }
                ui.separator();
                if ui.button("Back to menu").clicked() {
                    back = true;
                }
            });
        back
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
