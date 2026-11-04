//! Walker add-on: real player assets, source-backed ground mechanics, and an
//! optional first-person camera. All state, archive handles and GPU resources
//! belong to this module; disabling it restores the host's free-flight path.

pub(crate) mod archive;
mod character;
pub(crate) mod physics;
pub(crate) mod pose;
mod race;

use self::{
    archive::Archive,
    character::{character_transform, Character},
    physics::{Body, Input, Mechanics},
    race::Race,
};
use crate::{model_draw_gpu, navigation::MapSpace};
use glam::Vec3;
use std::{collections::HashSet, path::PathBuf};
use winit::keyboard::KeyCode;

const THIRD_PERSON_DISTANCE: f32 = 4.5;

#[derive(Clone, Copy, Debug)]
struct Settings {
    race: Race,
    mechanics: Mechanics,
    first_person: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            race: Race::Human,
            mechanics: Mechanics::default(),
            first_person: false,
        }
    }
}

struct Gpu {
    draw: model_draw_gpu::Draw,
    // Keep the sampler alive alongside bind groups that reference it.
    _sampler: wgpu::Sampler,
}

pub struct Walker {
    element: Option<PathBuf>,
    map: Option<MapSpace>,
    archive: Option<Archive>,
    settings: Settings,
    body: Option<Body>,
    character: Option<Character>,
    facing: Vec3,
    jump_was_down: bool,
    gpu: Option<Gpu>,
    gpu_failed: bool,
    last_error: Option<String>,
}

impl Default for Walker {
    fn default() -> Self {
        Self {
            element: None,
            map: None,
            archive: None,
            settings: Settings::default(),
            body: None,
            character: None,
            facing: Vec3::NEG_Z,
            jump_was_down: false,
            gpu: None,
            gpu_failed: false,
            last_error: None,
        }
    }
}

impl Walker {
    pub fn with_host(element: PathBuf, map: MapSpace) -> Self {
        Self {
            element: Some(element),
            map: Some(map),
            ..Self::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.body.is_some() && self.character.is_some()
    }

    pub fn status(&self) -> String {
        if let Some(error) = &self.last_error {
            return error.clone();
        }
        if self.is_active() {
            format!("Active — {}", self.settings.race.label())
        } else {
            "Disabled".into()
        }
    }

    pub fn set_active(&mut self, active: bool, spawn: Vec3, facing: Vec3) {
        if !active {
            self.body = None;
            self.character = None;
            self.gpu = None;
            self.gpu_failed = false;
            self.last_error = None;
            self.jump_was_down = false;
            return;
        }
        if self.is_active() {
            return;
        }
        let result = (|| -> Result<(), String> {
            let element = self
                .element
                .as_ref()
                .ok_or("Walker has no client asset host in this mode")?;
            let map = self.map.as_ref().ok_or("Walker has no terrain host")?;
            if self.archive.is_none() {
                self.archive = Some(Archive::open(element)?);
            }
            let ground = physics::ground(map, spawn.x, spawn.z);
            let position = Vec3::new(spawn.x, ground.height, spawn.z);
            let facing = Vec3::new(facing.x, 0., facing.z).normalize_or_zero();
            let facing = if facing == Vec3::ZERO {
                Vec3::NEG_Z
            } else {
                facing
            };
            let character = Character::load(
                self.archive.as_mut().unwrap(),
                self.settings.race,
                character_transform(position, facing),
            )?;
            self.body = Some(Body::new(position));
            self.character = Some(character);
            self.facing = facing;
            self.gpu = None;
            self.gpu_failed = false;
            self.last_error = None;
            Ok(())
        })();
        if let Err(error) = result {
            self.last_error = Some(format!("Walker could not spawn: {error}"));
            self.body = None;
            self.character = None;
            self.gpu = None;
        }
    }

    fn change_race(&mut self, race: Race) {
        if race == self.settings.race {
            return;
        }
        let previous = self.settings.race;
        self.settings.race = race;
        if !self.is_active() {
            return;
        }
        let result = (|| -> Result<Character, String> {
            let body = self.body.as_ref().ok_or("Walker body is not active")?;
            Character::load(
                self.archive
                    .as_mut()
                    .ok_or("Walker model archive is not open")?,
                race,
                character_transform(body.position, self.facing),
            )
        })();
        match result {
            Ok(character) => {
                self.character = Some(character);
                self.gpu = None;
                self.gpu_failed = false;
                self.last_error = None;
            }
            Err(error) => {
                self.settings.race = previous;
                self.last_error = Some(format!("Could not load {}: {error}", race.label()));
            }
        }
    }

    /// Returns the add-on camera position only while Walker owns movement.
    /// `None` is the narrow host signal to retain ordinary free flight.
    pub fn update(
        &mut self,
        delta: f32,
        input_enabled: bool,
        keys: &HashSet<KeyCode>,
        yaw: f32,
        pitch: f32,
    ) -> Option<Vec3> {
        if !self.is_active() {
            return None;
        }
        let (s, c) = yaw.sin_cos();
        let forward = Vec3::new(s, 0., -c);
        let right = Vec3::new(-c, 0., -s);
        let mut direction = Vec3::ZERO;
        if input_enabled {
            if keys.contains(&KeyCode::KeyW) {
                direction += forward;
            }
            if keys.contains(&KeyCode::KeyS) {
                direction -= forward;
            }
            if keys.contains(&KeyCode::KeyD) {
                direction += right;
            }
            if keys.contains(&KeyCode::KeyA) {
                direction -= right;
            }
        }
        direction = direction.normalize_or_zero();
        if direction != Vec3::ZERO {
            self.facing = direction;
        }
        let space = input_enabled && keys.contains(&KeyCode::Space);
        let jump = space && !self.jump_was_down;
        self.jump_was_down = space;

        let map = self.map.as_ref()?.clone();
        let body = self.body.as_mut()?;
        if input_enabled {
            body.advance(
                delta,
                &self.settings.mechanics,
                Input { direction, jump },
                &map,
            );
        }
        if let Some(character) = &mut self.character {
            if let Err(error) = character.update(delta, body, self.facing) {
                self.last_error = Some(format!("Walker animation: {error}"));
            }
            let eye = character.eye;
            if self.settings.first_person {
                return Some(eye);
            }
            let cp = pitch.cos();
            let look = Vec3::new(s * cp, pitch.sin(), -c * cp);
            let mut camera = eye - look * THIRD_PERSON_DISTANCE;
            let clearance = physics::ground(&map, camera.x, camera.z).height + 0.25;
            camera.y = camera.y.max(clearance);
            return Some(camera);
        }
        None
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let mut race = self.settings.race;
        egui::ComboBox::from_label("Race / representative profession")
            .selected_text(race.label())
            .show_ui(ui, |ui| {
                for choice in Race::ALL {
                    ui.selectable_value(&mut race, choice, choice.label());
                }
            });
        if race != self.settings.race {
            self.change_race(race);
        }

        let mut speed = self.settings.mechanics.move_speed;
        if ui
            .add(egui::Slider::new(&mut speed, 0.5..=20.0).suffix(" m/s"))
            .changed()
        {
            self.settings.mechanics.set_move_speed(speed);
        }
        ui.label("Move speed (native default 5.0 m/s)")
            .on_hover_text("CECConfigs' default ground run speed. Server-side class, equipment and buff modifiers are not present in this offline explorer.");

        let mut height = self.settings.mechanics.jump_height;
        if ui
            .add(egui::Slider::new(&mut height, 0.25..=12.0).suffix(" m"))
            .changed()
        {
            self.settings.mechanics.set_jump_height(height);
        }
        ui.label("Jump height at the fixed 60 Hz physics step")
            .on_hover_text(format!(
                "Recovered ordinary jump: 10.0 m/s impulse and 9.8 m/s² gravity. The native default is {:.3} m at 60 Hz; the continuous v²/(2g) reference is {:.3} m. A custom height is converted to an equivalent launch impulse.",
                physics::native_jump_height(),
                physics::CONTINUOUS_JUMP_HEIGHT,
            ));
        ui.checkbox(&mut self.settings.first_person, "First Person Mode")
            .on_hover_text("Places the camera at the evaluated Bip01 Head bone and suppresses the entire character draw, preventing head or body geometry from obstructing the view.");
        if ui.button("Restore native movement defaults").clicked() {
            self.settings.mechanics = Mechanics::default();
        }
        ui.separator();
        ui.label(
            egui::RichText::new("WASD move  •  Space jump / double jump  •  Mouse look").small(),
        );
        if let Some(body) = &self.body {
            ui.label(
                egui::RichText::new(format!(
                    "{}  •  jumps {}/{}  •  xyz {:.1}, {:.1}, {:.1}",
                    if body.grounded {
                        "Grounded"
                    } else {
                        "Airborne"
                    },
                    body.jump_count,
                    physics::MAX_JUMPS,
                    body.position.x,
                    body.position.y,
                    body.position.z,
                ))
                .small()
                .weak(),
            );
        }
        let status = self.status();
        if self.last_error.is_some() {
            ui.colored_label(egui::Color32::LIGHT_RED, status);
        } else {
            ui.label(egui::RichText::new(status).small().weak());
        }
        if let Some(character) = &self.character {
            for warning in character.warnings.iter().take(3) {
                ui.label(egui::RichText::new(warning).small().weak());
            }
        }
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
        if !self.is_active() || self.settings.first_person || self.gpu_failed {
            return;
        }
        if self.gpu.is_none() {
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Walker character sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let result = (|| -> Result<model_draw_gpu::Draw, String> {
                let character = self.character.as_ref().ok_or("Walker character missing")?;
                let archive = self.archive.as_mut().ok_or("Walker archive missing")?;
                model_draw_gpu::Draw::new(
                    device,
                    queue,
                    format,
                    Some((
                        wgpu::TextureFormat::Depth32Float,
                        wgpu::CompareFunction::Greater,
                    )),
                    true,
                    &sampler,
                    &character.asset,
                    &character.posed.meshes,
                    frame,
                    |path| archive.read(path),
                )
            })();
            match result {
                Ok(draw) => {
                    self.gpu = Some(Gpu {
                        draw,
                        _sampler: sampler,
                    })
                }
                Err(error) => {
                    self.last_error = Some(format!("Walker rendering: {error}"));
                    self.gpu_failed = true;
                    return;
                }
            }
        }
        let update = match (&self.gpu, &self.character) {
            (Some(gpu), Some(character)) => {
                gpu.draw
                    .update(queue, &character.asset, &character.posed.meshes, frame)
            }
            _ => return,
        };
        if let Err(error) = update {
            self.last_error = Some(format!("Walker rendering: {error}"));
            self.gpu_failed = true;
            self.gpu = None;
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("add-on Walker character pass"),
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
            gpu.draw.draw(&mut pass);
        }
    }
}
