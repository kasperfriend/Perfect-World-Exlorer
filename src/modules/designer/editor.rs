//! Interactive character-designer module and world preview.

use super::{
    assembly::{Character, CharacterMeshes},
    assets::Assets,
    catalog::{Class, Gender},
    controls,
    customize::CharacterDesign,
};
use crate::{
    model_draw_gpu::{self, Frame},
    modules::walker::physics,
    navigation::MapSpace,
};
use glam::{Mat4, Vec3};
use std::path::PathBuf;

struct Gpu {
    draws: Vec<model_draw_gpu::Draw>,
    _sampler: wgpu::Sampler,
}

pub(crate) struct Designer {
    element: Option<PathBuf>,
    map: Option<MapSpace>,
    assets: Option<Assets>,
    design: CharacterDesign,
    character: Option<Character>,
    meshes: Option<CharacterMeshes>,
    world: Mat4,
    active: bool,
    dirty: bool,
    gpu: Option<Gpu>,
    gpu_failed: bool,
    status: String,
    preset_name: String,
}

impl Default for Designer {
    fn default() -> Self {
        Self {
            element: None,
            map: None,
            assets: None,
            design: CharacterDesign::new(Class::Blademaster, Gender::Male),
            character: None,
            meshes: None,
            world: Mat4::IDENTITY,
            active: false,
            dirty: false,
            gpu: None,
            gpu_failed: false,
            status: "Disabled".into(),
            preset_name: String::new(),
        }
    }
}

impl Designer {
    pub(crate) fn with_host(element: PathBuf, map: MapSpace) -> Self {
        Self {
            element: Some(element),
            map: Some(map),
            ..Self::default()
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    fn clear_preview(&mut self) {
        self.character = None;
        self.meshes = None;
        self.gpu = None;
        self.gpu_failed = false;
    }

    fn rebuild(&mut self) {
        self.clear_preview();
        let result = (|| -> Result<(Character, CharacterMeshes), String> {
            let assets = self
                .assets
                .as_mut()
                .ok_or("character assets are not open")?;
            let character = Character::assemble(self.design.clone(), assets)?;
            let pose = character.bind_pose(self.world)?;
            let meshes = character.meshes_from_pose(&pose)?;
            Ok((character, meshes))
        })();
        match result {
            Ok((character, meshes)) => {
                self.status = format!(
                    "Preview ready — {} {}, {} equipment skin(s), {} warning(s)",
                    self.design.class.name(),
                    self.design.gender.name(),
                    character.equipment.len(),
                    character.warnings.len()
                );
                self.character = Some(character);
                self.meshes = Some(meshes);
                self.dirty = false;
            }
            Err(error) => {
                self.status = format!("Could not assemble character: {error}");
                self.dirty = true;
            }
        }
    }

    fn load_default(&mut self, class: Class, gender: Gender) {
        let result = self
            .assets
            .as_mut()
            .ok_or("character assets are not open".into())
            .and_then(|assets| assets.default_design(class, gender));
        match result {
            Ok(design) => {
                self.design = design;
                self.rebuild();
            }
            Err(error) => self.status = format!("Could not load native default: {error}"),
        }
    }

    fn apply_preset(&mut self, path: &str) {
        let mut design = self.design.clone();
        let result = self
            .assets
            .as_mut()
            .ok_or("character assets are not open".into())
            .and_then(|assets| assets.apply_preset(&mut design, path));
        match result {
            Ok(()) => {
                self.design = design;
                self.rebuild();
            }
            Err(error) => self.status = format!("Could not apply preset {path}: {error}"),
        }
    }

    fn set_active(&mut self, active: bool, spawn: Vec3, yaw: f32) {
        if !active {
            self.active = false;
            self.clear_preview();
            self.status = "Disabled".into();
            return;
        }
        if self.active {
            return;
        }
        self.active = true;
        let facing = Vec3::new(yaw.sin(), 0., -yaw.cos());
        let position = spawn + facing * 3.0;
        let position = if let Some(map) = &self.map {
            Vec3::new(
                position.x,
                physics::ground(map, position.x, position.z).height,
                position.z,
            )
        } else {
            position
        };
        // Face the camera. This is only the preview's world placement; the
        // assembled geometry and all attachment transforms remain unchanged.
        self.world = preview_transform(position, -facing);
        if self.assets.is_none() {
            let result = self
                .element
                .as_deref()
                .ok_or("Character Designer has no client asset host in this mode".into())
                .and_then(Assets::open);
            match result {
                Ok(assets) => self.assets = Some(assets),
                Err(error) => {
                    self.status = format!("Character Designer could not open assets: {error}");
                    return;
                }
            }
        }
        self.load_default(self.design.class, self.design.gender);
    }

    pub(crate) fn menu_ui(&mut self, ui: &mut egui::Ui, spawn: Vec3, yaw: f32) {
        ui.label(
            egui::RichText::new(
                "Authentic PW 1.4.2 v27 data, native presets, full-body equipment and EFACE customization. The preview is placed on the terrain ahead of the camera.",
            )
            .small()
            .weak(),
        );
        let button = if self.active {
            "Disable Character Designer"
        } else {
            "Enable Character Designer"
        };
        if ui
            .add_sized([220., 32.], egui::Button::new(button))
            .clicked()
        {
            self.set_active(!self.active, spawn, yaw);
        }
        if !self.active {
            ui.label(egui::RichText::new(&self.status).weak());
            return;
        }

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
        if class != self.design.class || gender != self.design.gender {
            self.load_default(class, gender);
        }

        let mut selected_preset = None;
        if let Some(assets) = &self.assets {
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
        }
        if let Some(path) = selected_preset {
            self.apply_preset(&path);
        }

        if let Some(assets) = &self.assets {
            let bounds = &assets.bounds;
            let catalog = &assets.catalog;
            if controls::show(ui, &mut self.design, catalog, bounds) {
                self.dirty = true;
            }
        }

        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.dirty, egui::Button::new("Apply / rebuild preview"))
                .clicked()
            {
                self.rebuild();
            }
            if ui.button("Reload native default").clicked() {
                self.load_default(self.design.class, self.design.gender);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Preset name");
            ui.text_edit_singleline(&mut self.preset_name);
            if ui.button("Save new preset").clicked() {
                let result = self
                    .assets
                    .as_ref()
                    .ok_or("character assets are not open".into())
                    .and_then(|assets| assets.save_preset(&self.design, &self.preset_name));
                match result {
                    Ok(path) => self.status = format!("Saved {}", path.display()),
                    Err(error) => self.status = format!("Could not save preset: {error}"),
                }
            }
        });
        if self.dirty {
            ui.label(egui::RichText::new("Unapplied changes").small().weak());
        }
        let error = self.character.is_none() || self.gpu_failed;
        if error {
            ui.colored_label(egui::Color32::LIGHT_RED, &self.status);
        } else {
            ui.label(egui::RichText::new(&self.status).color(egui::Color32::LIGHT_GREEN));
        }
        if let Some(character) = &self.character {
            egui::CollapsingHeader::new(format!("Asset warnings ({})", character.warnings.len()))
                .show(ui, |ui| {
                    for warning in &character.warnings {
                        ui.label(egui::RichText::new(warning).small());
                    }
                });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame: Frame,
    ) {
        if !self.active || self.character.is_none() || self.meshes.is_none() || self.gpu_failed {
            return;
        }
        if self.gpu.is_none() {
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Character Designer sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let result = (|| -> Result<Gpu, String> {
                let character = self.character.as_ref().unwrap();
                let meshes = self.meshes.as_ref().unwrap();
                let assets = self.assets.as_mut().ok_or("designer assets missing")?;
                let mut draws = Vec::with_capacity(2 + character.weapons.len());
                for (asset, posed) in std::iter::once((&character.body, &meshes.body))
                    .chain(std::iter::once((&character.face, &meshes.face)))
                    .chain(
                        character
                            .weapons
                            .iter()
                            .zip(&meshes.weapons)
                            .map(|(weapon, posed)| (&weapon.asset, posed)),
                    )
                {
                    draws.push(model_draw_gpu::Draw::new(
                        device,
                        queue,
                        format,
                        Some((
                            wgpu::TextureFormat::Depth32Float,
                            wgpu::CompareFunction::Greater,
                        )),
                        true,
                        &sampler,
                        asset,
                        posed,
                        frame,
                        |path| assets.read(path),
                    )?);
                }
                Ok(Gpu {
                    draws,
                    _sampler: sampler,
                })
            })();
            match result {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(error) => {
                    self.status = format!("Character Designer rendering: {error}");
                    self.gpu_failed = true;
                    return;
                }
            }
        }

        let update = (|| -> Result<(), String> {
            let gpu = self.gpu.as_ref().unwrap();
            let character = self.character.as_ref().unwrap();
            let meshes = self.meshes.as_ref().unwrap();
            let mut index = 0;
            for (asset, posed) in std::iter::once((&character.body, &meshes.body))
                .chain(std::iter::once((&character.face, &meshes.face)))
                .chain(
                    character
                        .weapons
                        .iter()
                        .zip(&meshes.weapons)
                        .map(|(weapon, posed)| (&weapon.asset, posed)),
                )
            {
                gpu.draws[index].update(queue, asset, posed, frame)?;
                index += 1;
            }
            Ok(())
        })();
        if let Err(error) = update {
            self.status = format!("Character Designer rendering: {error}");
            self.gpu_failed = true;
            self.gpu = None;
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Character Designer preview pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some(gpu) = &self.gpu {
            for draw in &gpu.draws {
                draw.draw(&mut pass);
            }
        }
    }
}

fn preview_transform(position: Vec3, facing: Vec3) -> Mat4 {
    let direction = Vec3::new(facing.x, 0., facing.z).normalize_or_zero();
    let direction = if direction == Vec3::ZERO {
        Vec3::NEG_Z
    } else {
        direction
    };
    let right = Vec3::Y.cross(direction).normalize_or_zero();
    Mat4::from_cols(
        right.extend(0.),
        Vec3::Y.extend(0.),
        direction.extend(0.),
        position.extend(1.),
    )
}
