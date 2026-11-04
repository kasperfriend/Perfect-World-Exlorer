//! Walker add-on: real player assets, source-backed ground mechanics, and an
//! optional first-person camera. All state, archive handles and GPU resources
//! belong to this module; disabling it restores the host's free-flight path.

pub(crate) mod archive;
mod character;
mod designer;
mod gfx;
pub(crate) mod physics;
pub(crate) mod pose;

use self::{
    character::{character_transform, Character},
    designer::Designer,
    physics::{Body, FlightInput, Input, Mechanics},
};
use crate::{
    gfx_decal_gpu,
    model_asset::{ModelAsset, PosedMesh},
    model_draw_gpu,
    modules::designer::{
        assembly::{CharacterMeshes, TravelState},
        assets::Assets,
    },
    navigation::MapSpace,
    water::WaterArea,
};
use glam::Vec3;
use std::{collections::HashSet, path::PathBuf};
use winit::keyboard::KeyCode;

const THIRD_PERSON_DISTANCE: f32 = 4.5;

#[derive(Clone, Copy, Debug)]
struct Settings {
    mechanics: Mechanics,
    first_person: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mechanics: Mechanics::default(),
            first_person: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HotkeyAction {
    Flight,
    Mount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Hotkey {
    key: KeyCode,
    control: bool,
}

impl Hotkey {
    fn is_down(self, keys: &HashSet<KeyCode>) -> bool {
        let control = keys.contains(&KeyCode::ControlLeft) || keys.contains(&KeyCode::ControlRight);
        control == self.control && keys.contains(&self.key)
    }

    fn label(self) -> String {
        format!("{}{:?}", if self.control { "Ctrl+" } else { "" }, self.key).replace("Key", "")
    }
}

fn captured_hotkey(keys: &HashSet<KeyCode>) -> Option<Hotkey> {
    let key = keys.iter().copied().find(|key| {
        !matches!(
            key,
            KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::ShiftLeft | KeyCode::ShiftRight
        )
    })?;
    Some(Hotkey {
        key,
        control: keys.contains(&KeyCode::ControlLeft) || keys.contains(&KeyCode::ControlRight),
    })
}

fn edge_triggered(down: bool, was_down: &mut bool) -> bool {
    let triggered = down && !*was_down;
    *was_down = down;
    triggered
}

fn jump_requested(space: bool, was_down: bool, travel: TravelState) -> bool {
    space && !was_down && travel != TravelState::Flying
}

#[derive(Clone, Copy)]
enum ModelPart {
    Body,
    Face,
    Weapon { right: bool },
    Flight,
    Mount,
}

struct RenderModel<'a> {
    part: ModelPart,
    asset: &'a ModelAsset,
    poses: &'a [PosedMesh],
}

struct Gpu {
    draws: Vec<model_draw_gpu::Draw>,
    parts: Vec<ModelPart>,
    gfx: gfx_decal_gpu::Draw,
    // Keep the sampler alive alongside bind groups that reference it.
    _sampler: wgpu::Sampler,
}

fn render_models<'a>(
    character: &'a Character,
    meshes: &'a CharacterMeshes,
) -> Result<Vec<RenderModel<'a>>, String> {
    let assembly = &character.assembly;
    let mut models = Vec::with_capacity(4 + assembly.weapons.len());
    models.push(RenderModel {
        part: ModelPart::Body,
        asset: &assembly.body,
        poses: &meshes.body,
    });
    models.push(RenderModel {
        part: ModelPart::Face,
        asset: &assembly.face,
        poses: &meshes.face,
    });
    models.extend(
        assembly
            .weapons
            .iter()
            .zip(&meshes.weapons)
            .filter(|(_, posed)| !posed.is_empty())
            .map(|(weapon, posed)| RenderModel {
                part: ModelPart::Weapon {
                    right: weapon.hook == "HH_righthandweapon",
                },
                asset: &weapon.asset,
                poses: posed,
            }),
    );
    if !meshes.flight.is_empty() {
        let flight = assembly
            .flight
            .as_ref()
            .ok_or("flight meshes have no asset")?;
        models.push(RenderModel {
            part: ModelPart::Flight,
            asset: &flight.asset,
            poses: &meshes.flight,
        });
    }
    if !meshes.mount.is_empty() {
        let mount = assembly
            .mount
            .as_ref()
            .ok_or("mount meshes have no asset")?;
        models.push(RenderModel {
            part: ModelPart::Mount,
            asset: &mount.asset,
            poses: &meshes.mount,
        });
    }
    Ok(models)
}

pub struct Walker {
    element: Option<PathBuf>,
    map: Option<MapSpace>,
    water: Vec<WaterArea>,
    assets: Option<Assets>,
    designer: Designer,
    settings: Settings,
    body: Option<Body>,
    character: Option<Character>,
    facing: Vec3,
    travel: TravelState,
    flight_hotkey: Hotkey,
    mount_hotkey: Hotkey,
    capture: Option<HotkeyAction>,
    capture_wait_release: bool,
    flight_was_down: bool,
    mount_was_down: bool,
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
            water: Vec::new(),
            assets: None,
            designer: Designer::default(),
            settings: Settings::default(),
            body: None,
            character: None,
            facing: Vec3::NEG_Z,
            travel: TravelState::Ground,
            flight_hotkey: Hotkey {
                key: KeyCode::KeyF,
                control: true,
            },
            mount_hotkey: Hotkey {
                key: KeyCode::KeyH,
                control: true,
            },
            capture: None,
            capture_wait_release: false,
            flight_was_down: false,
            mount_was_down: false,
            jump_was_down: false,
            gpu: None,
            gpu_failed: false,
            last_error: None,
        }
    }
}

impl Walker {
    pub fn with_host(element: PathBuf, map: MapSpace, water: Vec<WaterArea>) -> Self {
        Self {
            element: Some(element),
            map: Some(map),
            water,
            ..Self::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.body.is_some() && self.character.is_some()
    }

    pub fn visible_in_water_pass(&self, plane: f32, reflection: bool) -> bool {
        let (Some(body), Some(character)) = (&self.body, &self.character) else {
            return false;
        };
        let design = &character.assembly.design;
        let constants = physics::MovementConstants::new(design.class, design.gender);
        let (min_y, max_y) = body.aabb_y(constants);
        let (reflect, refract) = physics::water_pass_visibility(min_y, max_y, plane);
        if reflection {
            reflect
        } else {
            refract
        }
    }

    pub fn status(&self) -> String {
        if let Some(error) = &self.last_error {
            return error.clone();
        }
        if let Some(character) = &self.character {
            format!(
                "Active — {} {}",
                character.assembly.design.class.name(),
                character.assembly.design.gender.name()
            )
        } else {
            "Disabled".into()
        }
    }

    pub fn set_active(&mut self, active: bool, spawn: Vec3, facing: Vec3) {
        if !active {
            if let Some(character) = &self.character {
                self.designer
                    .restore_applied(character.assembly.design.clone());
            }
            self.body = None;
            self.character = None;
            self.gpu = None;
            self.gpu_failed = false;
            self.last_error = None;
            self.jump_was_down = false;
            self.travel = TravelState::Ground;
            self.capture = None;
            self.flight_was_down = false;
            self.mount_was_down = false;
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
            if self.assets.is_none() {
                self.assets = Some(Assets::open(element)?);
            }
            let ground = physics::ground(map, spawn.x, spawn.z);
            let position = Vec3::new(spawn.x, ground.height, spawn.z);
            let facing = Vec3::new(facing.x, 0., facing.z).normalize_or_zero();
            let facing = if facing == Vec3::ZERO {
                Vec3::NEG_Z
            } else {
                facing
            };
            let assets = self.assets.as_mut().unwrap();
            let design = self.designer.ensure_initialized(assets)?;
            let character = Character::load(assets, design, character_transform(position, facing))?;
            self.settings
                .mechanics
                .set_move_speed(character.assembly.class_speeds.run);
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

    fn change_design(&mut self, design: crate::modules::designer::customize::CharacterDesign) {
        if !self.is_active() {
            return;
        }
        let result = (|| -> Result<Character, String> {
            let body = self.body.as_ref().ok_or("Walker body is not active")?;
            Character::load(
                self.assets.as_mut().ok_or("Walker assets are not open")?,
                design,
                character_transform(body.position, self.facing),
            )
        })();
        match result {
            Ok(character) => {
                self.designer.rebuild_succeeded(
                    character.assembly.equipment.len(),
                    character.assembly.warnings.len(),
                );
                if (self.travel == TravelState::Flying && character.assembly.flight.is_none())
                    || (self.travel == TravelState::Mounted && character.assembly.mount.is_none())
                {
                    self.travel = TravelState::Ground;
                }
                self.character = Some(character);
                self.gpu = None;
                self.gpu_failed = false;
                self.last_error = None;
            }
            Err(error) => {
                if let Some(character) = &self.character {
                    self.designer
                        .restore_applied(character.assembly.design.clone());
                }
                self.designer.rebuild_failed(&error);
                self.last_error = Some(format!("Could not customize Walker: {error}"));
            }
        }
    }

    fn activate_travel(&mut self, requested: TravelState) {
        let next = if self.travel == requested {
            TravelState::Ground
        } else {
            requested
        };
        match next {
            TravelState::Ground => {}
            TravelState::Mounted => {
                if self
                    .character
                    .as_ref()
                    .and_then(|character| character.assembly.mount.as_ref())
                    .is_none()
                {
                    self.last_error = Some("Select a Ride pet before mounting.".into());
                    return;
                }
            }
            TravelState::Flying => {
                if self
                    .character
                    .as_ref()
                    .and_then(|character| character.assembly.flight.as_ref())
                    .is_none()
                {
                    self.last_error = Some("Select a wing or fly-sword before flying.".into());
                    return;
                }
                if let Some(body) = &mut self.body {
                    body.begin_flight();
                }
            }
        }
        self.travel = next;
        self.gpu = None;
        self.gpu_failed = false;
        self.last_error = None;
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
        let mut captured = false;
        if let Some(action) = self.capture {
            if self.capture_wait_release {
                if captured_hotkey(keys).is_none() {
                    self.capture_wait_release = false;
                }
            } else if let Some(hotkey) = captured_hotkey(keys) {
                match action {
                    HotkeyAction::Flight => self.flight_hotkey = hotkey,
                    HotkeyAction::Mount => self.mount_hotkey = hotkey,
                }
                self.capture = None;
                captured = true;
            }
        }
        let flight_down = self.flight_hotkey.is_down(keys);
        let mount_down = self.mount_hotkey.is_down(keys);
        if captured {
            self.flight_was_down = flight_down;
            self.mount_was_down = mount_down;
        } else if input_enabled {
            if edge_triggered(flight_down, &mut self.flight_was_down) {
                self.activate_travel(TravelState::Flying);
            }
            if edge_triggered(mount_down, &mut self.mount_was_down) {
                self.activate_travel(TravelState::Mounted);
            }
        } else {
            self.flight_was_down = flight_down;
            self.mount_was_down = mount_down;
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
        let jump = jump_requested(space, self.jump_was_down, self.travel);
        self.jump_was_down = space;
        let rushing = input_enabled
            && self.travel == TravelState::Flying
            && (keys.contains(&KeyCode::ShiftLeft) || keys.contains(&KeyCode::ShiftRight))
            && self
                .character
                .as_ref()
                .and_then(|character| character.assembly.flight.as_ref())
                .and_then(|flight| flight.rush_speed)
                .is_some();
        let vertical = if input_enabled && self.travel == TravelState::Flying {
            (if space { 1.0 } else { 0.0 })
                - if keys.contains(&KeyCode::KeyC) {
                    1.0
                } else {
                    0.0
                }
        } else {
            0.0
        };
        let flight_speed = self.character.as_ref().and_then(|character| {
            character.assembly.flight.as_ref().map(|flight| {
                let range = if rushing {
                    flight.rush_speed.unwrap_or(flight.normal_speed)
                } else {
                    flight.normal_speed
                };
                character.assembly.class_speeds.fly + range.0
            })
        });
        let mount_speed = self
            .character
            .as_ref()
            .and_then(|character| character.assembly.mount.as_ref().map(|mount| mount.speed));
        let (movement_constants, swim_speed) = self.character.as_ref().map(|character| {
            let design = &character.assembly.design;
            (
                physics::MovementConstants::new(design.class, design.gender),
                character.assembly.class_speeds.swim,
            )
        })?;
        let water_vertical = if input_enabled && self.travel == TravelState::Ground {
            let ascend = keys.contains(&KeyCode::ArrowUp);
            let descend = keys.contains(&KeyCode::ArrowDown) || keys.contains(&KeyCode::KeyZ);
            let deep_space = space
                && self.body.as_ref().is_some_and(|body| {
                    let level =
                        physics::water_height(&self.water, body.position.x, body.position.z);
                    level.is_some()
                        && !physics::can_take_off_water(body.position, level, movement_constants)
                });
            (if ascend || deep_space { 1.0 } else { 0.0 }) - if descend { 1.0 } else { 0.0 }
        } else {
            0.0
        };

        let map = self.map.as_ref()?.clone();
        let body = self.body.as_mut()?;
        if input_enabled {
            match self.travel {
                TravelState::Ground => body.advance_with_water(
                    delta,
                    &self.settings.mechanics,
                    Input { direction, jump },
                    water_vertical,
                    swim_speed,
                    &map,
                    &self.water,
                    movement_constants,
                ),
                TravelState::Mounted => {
                    let mut mechanics = self.settings.mechanics;
                    mechanics.move_speed = mount_speed?;
                    body.advance_mounted(delta, &mechanics, Input { direction, jump }, &map);
                }
                TravelState::Flying => body.advance_flight(
                    delta,
                    FlightInput {
                        direction,
                        vertical,
                        speed: flight_speed?,
                    },
                    &map,
                ),
            }
        }
        if let Some(character) = &mut self.character {
            if let Err(error) =
                character.update(delta, body, self.facing, self.travel, rushing, vertical)
            {
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
        let rebuild = if let Some(assets) = self.assets.as_mut() {
            self.designer.ui(ui, assets)
        } else {
            None
        };
        if let Some(design) = rebuild {
            self.change_design(design);
        }

        ui.separator();
        ui.heading("Mounts and flight");
        ui.horizontal(|ui| {
            let flight = if self.travel == TravelState::Flying {
                "Land"
            } else {
                "Fly"
            };
            if ui.button(flight).clicked() {
                self.activate_travel(TravelState::Flying);
            }
            let mount = if self.travel == TravelState::Mounted {
                "Dismount"
            } else {
                "Mount"
            };
            if ui.button(mount).clicked() {
                self.activate_travel(TravelState::Mounted);
            }
            if ui.button("Ground").clicked() {
                self.activate_travel(self.travel);
            }
        });
        ui.horizontal(|ui| {
            let flight_label = if self.capture == Some(HotkeyAction::Flight) {
                "Flight: press a key…".to_owned()
            } else {
                format!("Flight: {}", self.flight_hotkey.label())
            };
            if ui.button(flight_label).clicked() {
                self.capture = Some(HotkeyAction::Flight);
                self.capture_wait_release = true;
            }
            let mount_label = if self.capture == Some(HotkeyAction::Mount) {
                "Mount: press a key…".to_owned()
            } else {
                format!("Mount: {}", self.mount_hotkey.label())
            };
            if ui.button(mount_label).clicked() {
                self.capture = Some(HotkeyAction::Mount);
                self.capture_wait_release = true;
            }
        });
        if let Some(character) = &self.character {
            if let Some(flight) = &character.assembly.flight {
                let base = character.assembly.class_speeds.fly;
                let normal = flight.normal_speed;
                let rush = flight.rush_speed;
                ui.label(
                    egui::RichText::new(format!(
                        "{} • base {base:.1} m/s • normal bonus +{:.1}–{:.1}{}",
                        flight.mode.name(),
                        normal.0,
                        normal.1,
                        rush.map_or_else(
                            || " • no rush-flight range".to_owned(),
                            |range| format!(" • rush bonus +{:.1}–{:.1}", range.0, range.1)
                        )
                    ))
                    .small()
                    .weak(),
                )
                .on_hover_text("Flight movement uses the selected range's lower bound. Hold Shift for rush flight when the authentic record provides it.");
            }
            if let Some(mount) = &character.assembly.mount {
                ui.label(
                    egui::RichText::new(format!(
                        "Ride level 1: {:.1} m/s • native per-level coefficient {:.3}",
                        mount.speed, mount.speed_per_level
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        ui.label(
            egui::RichText::new(
                "Fly: WASD, Space up, C down; hold Shift for rush. Mount: WASD + Space jump. Hotkeys trigger once per press.",
            )
            .small()
            .weak(),
        );

        ui.separator();
        ui.heading("Movement and camera");
        let mut speed = self.settings.mechanics.move_speed;
        if ui
            .add(egui::Slider::new(&mut speed, 0.5..=20.0).suffix(" m/s"))
            .changed()
        {
            self.settings.mechanics.set_move_speed(speed);
        }
        let native_run = self
            .character
            .as_ref()
            .map_or(physics::NATIVE_RUN_SPEED, |character| {
                character.assembly.class_speeds.run
            });
        ui.label(format!("Ground move speed (class native {native_run:.1} m/s)"))
            .on_hover_text("CECConfigs' profession run speed. Server-side equipment and buff modifiers are not present in this offline explorer.");

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
            .on_hover_text("Uses a stable bind-head eye anchor without animation bob. The body and mount remain visible; native head/face, right-hand weapon, and flight meshes are hidden.");
        if ui.button("Restore native movement defaults").clicked() {
            self.settings.mechanics = Mechanics::default();
            self.settings.mechanics.set_move_speed(native_run);
        }
        ui.separator();
        ui.label(
            egui::RichText::new(
                "Ground: WASD + Space jump/double jump • Water: WASD + Arrow Up/Down or Z, Space takeoff/ascend • Travel controls above • Mouse look",
            )
            .small(),
        );
        if let Some(body) = &self.body {
            ui.label(
                egui::RichText::new(format!(
                    "{}  •  jumps {}/{}  •  xyz {:.1}, {:.1}, {:.1}",
                    match self.travel {
                        TravelState::Flying => "Flying",
                        TravelState::Mounted => "Riding",
                        TravelState::Ground if body.swimming && !body.water_jumping => "Swimming",
                        TravelState::Ground if body.grounded => "Grounded",
                        TravelState::Ground => "Airborne",
                    },
                    body.jump_count,
                    if self.travel == TravelState::Mounted {
                        physics::MOUNTED_MAX_JUMPS
                    } else {
                        physics::MAX_JUMPS
                    },
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
        if !self.is_active() || self.gpu_failed {
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
            let result = (|| -> Result<Gpu, String> {
                let character = self.character.as_ref().ok_or("Walker character missing")?;
                let assets = self.assets.as_mut().ok_or("Walker assets missing")?;
                let models = render_models(character, &character.posed.meshes)?;
                let mut draws = Vec::with_capacity(models.len());
                let mut parts = Vec::with_capacity(models.len());
                for model in models {
                    parts.push(model.part);
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
                        model.asset,
                        model.poses,
                        frame,
                        |path| assets.read(path),
                    )?);
                }
                let decal_quads = character
                    .gfx
                    .quads(&character.assembly, &character.posed.pose)?;
                let gfx = gfx_decal_gpu::Draw::new(
                    device,
                    queue,
                    format,
                    Some((
                        wgpu::TextureFormat::Depth32Float,
                        wgpu::CompareFunction::Greater,
                    )),
                    &decal_quads,
                    |path| assets.read(path),
                )?;
                Ok(Gpu {
                    draws,
                    parts,
                    gfx,
                    _sampler: sampler,
                })
            })();
            match result {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(error) => {
                    self.last_error = Some(format!("Walker rendering: {error}"));
                    self.gpu_failed = true;
                    return;
                }
            }
        }
        let update = (|| -> Result<(), String> {
            let gpu = self.gpu.as_ref().ok_or("Walker GPU draws missing")?;
            let character = self.character.as_ref().ok_or("Walker character missing")?;
            let models = render_models(character, &character.posed.meshes)?;
            if models.len() != gpu.draws.len() {
                return Err("travel draw count changed without rebuilding GPU resources".into());
            }
            for (index, model) in models.into_iter().enumerate() {
                gpu.draws[index].update(queue, model.asset, model.poses, frame)?;
            }
            let decal_quads = character
                .gfx
                .quads(&character.assembly, &character.posed.pose)?;
            gpu.gfx
                .update(queue, frame.matrix, frame.clip_plane, &decal_quads)?;
            Ok(())
        })();
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
            for (draw, part) in gpu.draws.iter().zip(&gpu.parts) {
                match part {
                    ModelPart::Body if self.settings.first_person => {
                        // Skin 1 is the separate native head installed by
                        // CECPlayer. Keep every pose uploaded; only submission
                        // changes, so the full-mesh validation remains intact.
                        draw.draw_filtered(&mut pass, |(skin, _)| skin != 1);
                    }
                    ModelPart::Face | ModelPart::Flight if self.settings.first_person => {}
                    ModelPart::Weapon { right: true } if self.settings.first_person => {}
                    _ => draw.draw(&mut pass),
                }
            }
            gpu.gfx.draw_quads(&mut pass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_require_their_modifier_and_trigger_only_on_press_edges() {
        let hotkey = Hotkey {
            key: KeyCode::KeyF,
            control: true,
        };
        let mut keys = HashSet::from([KeyCode::KeyF]);
        assert!(!hotkey.is_down(&keys));
        keys.insert(KeyCode::ControlLeft);
        assert!(hotkey.is_down(&keys));

        let mut was_down = false;
        assert!(edge_triggered(true, &mut was_down));
        assert!(!edge_triggered(true, &mut was_down));
        assert!(!edge_triggered(false, &mut was_down));
        assert!(edge_triggered(true, &mut was_down));
    }

    #[test]
    fn capture_ignores_modifier_only_keys_and_retains_control_state() {
        assert_eq!(
            captured_hotkey(&HashSet::from([KeyCode::ControlLeft])),
            None
        );
        let captured =
            captured_hotkey(&HashSet::from([KeyCode::ControlRight, KeyCode::KeyH])).unwrap();
        assert_eq!(captured.key, KeyCode::KeyH);
        assert!(captured.control);
    }

    #[test]
    fn jump_press_edge_is_shared_by_ground_and_riding_but_not_flight() {
        assert!(jump_requested(true, false, TravelState::Ground));
        assert!(jump_requested(true, false, TravelState::Mounted));
        assert!(!jump_requested(true, false, TravelState::Flying));
        assert!(!jump_requested(true, true, TravelState::Ground));
        assert!(!jump_requested(true, true, TravelState::Mounted));
        assert!(!jump_requested(false, false, TravelState::Mounted));
    }
}
