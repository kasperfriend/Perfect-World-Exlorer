//! Offscreen item-model preview. The target is independent of the world depth
//! buffer and follows the viewer's mirrored, reversed-depth camera convention.
use super::archive::LibrarianAssets;
use crate::{
    model_asset::ModelAsset,
    model_draw_gpu::{Draw, Frame},
    model_material::Lighting,
};
use glam::{Mat4, Vec3};

pub struct Preview {
    color: wgpu::Texture,
    depth: wgpu::Texture,
    pub texture_id: egui::TextureId,
    asset: Option<ModelAsset>,
    draw: Option<Draw>,
    loaded_path: String,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub turntable: bool,
    pub playing: bool,
    center: Vec3,
    pub pan: Vec3,
    radius: f32,
    updated: std::time::Instant,
    elapsed: f32,
    pub action: usize,
}
impl Preview {
    pub fn new(
        device: &wgpu::Device,
        renderer: &mut egui_wgpu::Renderer,
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            "Librarian preview",
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = texture(
            "Librarian preview depth",
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let texture_id = renderer.register_native_texture(
            device,
            &color.create_view(&Default::default()),
            wgpu::FilterMode::Linear,
        );
        Self {
            color,
            depth,
            texture_id,
            asset: None,
            draw: None,
            loaded_path: String::new(),
            error: Some("Select an item with a supported model.".into()),
            warnings: Vec::new(),
            yaw: 0.6,
            pitch: 0.2,
            zoom: 1.,
            turntable: false,
            playing: true,
            center: Vec3::ZERO,
            pan: Vec3::ZERO,
            radius: 1.,
            updated: std::time::Instant::now(),
            elapsed: 0.,
            action: 0,
        }
    }
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets: &mut LibrarianAssets,
        path: &str,
    ) {
        if self.loaded_path != path {
            self.loaded_path = path.to_owned();
            self.asset = None;
            self.draw = None;
            self.error = None;
            self.warnings.clear();
            if path.is_empty() || path == "0" {
                self.error = Some("No model is referenced by this item.".into());
            } else {
                let path = if path.starts_with("worn:") || path.starts_with("models/") {
                    path.to_owned()
                } else {
                    format!("models/{path}")
                };
                let result = if let Some(worn) = path.strip_prefix("worn:") {
                    load_worn(assets, worn).map(|asset| (asset, Vec::new()))
                } else if path.ends_with(".ecm") {
                    ModelAsset::load_unique_with(&path, |p| assets.read_model_resource(p))
                } else if path.ends_with(".smd") {
                    assets
                        .read_model_resource(&path)
                        .and_then(|bytes| crate::ecmodel::parse_smd(&bytes))
                        .and_then(|smd| {
                            let relative = |p: &str| {
                                let p = crate::ecmodel::normalize(p);
                                if p.starts_with("models/") {
                                    p
                                } else {
                                    format!("{}/{}", path.rsplit_once('/').map_or("", |v| v.0), p)
                                }
                            };
                            let skeleton = relative(&smd.skeleton);
                            let skins: Vec<_> = smd.skins.iter().map(|p| relative(p)).collect();
                            ModelAsset::load_static_skins_with(&skeleton, &skins, |p| {
                                assets.read_model_resource(p)
                            })
                            .map(|asset| (asset, Vec::new()))
                        })
                } else {
                    Err("This item does not reference a supported ECM or SMD model.".into())
                };
                match result.and_then(|(asset, warnings)| {
                    let poses = asset.pose_bind(Mat4::IDENTITY)?;
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for vertex in poses.iter().flat_map(|mesh| &mesh.vertices) {
                        let p = Vec3::from_array(vertex.position);
                        if p.is_finite() {
                            min = min.min(p);
                            max = max.max(p);
                        }
                    }
                    if !min.is_finite() || !max.is_finite() {
                        return Err("Model contains no finite geometry".into());
                    }
                    self.center = (min + max) * 0.5;
                    self.pan = Vec3::ZERO;
                    self.radius = ((max - min).length() * 0.5).max(0.1);
                    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("Librarian model sampler"),
                        mag_filter: wgpu::FilterMode::Linear,
                        min_filter: wgpu::FilterMode::Linear,
                        mipmap_filter: wgpu::FilterMode::Linear,
                        ..Default::default()
                    });
                    let draw = Draw::new(
                        device,
                        queue,
                        format,
                        Some((
                            wgpu::TextureFormat::Depth32Float,
                            wgpu::CompareFunction::Greater,
                        )),
                        true,
                        &sampler,
                        &asset,
                        &poses,
                        self.frame(),
                        |p| assets.read_model_resource(p),
                    )?;
                    Ok((asset, draw, warnings))
                }) {
                    Ok((asset, draw, warnings)) => {
                        self.warnings = warnings;
                        self.asset = Some(asset);
                        self.draw = Some(draw);
                        self.elapsed = 0.;
                        self.action = 0;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.updated).as_secs_f32().min(0.1);
        self.updated = now;
        if self.turntable {
            self.yaw += 0.6 * dt;
        }
        if self.playing {
            self.elapsed += dt;
        }
        if let (Some(asset), Some(draw)) = (&self.asset, &self.draw) {
            let poses = if let Some(clip) = asset.model.actions.get(self.action) {
                let fps = clip
                    .fps
                    .filter(|fps| *fps > 0)
                    .unwrap_or(asset.skeleton.fps)
                    .max(1) as f32;
                let start = clip.start / fps;
                let duration = ((clip.end - clip.start) / fps).max(0.001);
                asset
                    .pose_action_at_time(
                        self.action,
                        start + self.elapsed % duration,
                        Mat4::IDENTITY,
                    )
                    .or_else(|_| asset.pose_bind(Mat4::IDENTITY))
            } else {
                asset.pose_bind(Mat4::IDENTITY)
            };
            if let Ok(poses) = poses {
                let _ = draw.update(queue, asset, &poses, self.frame());
            }
        }
        let view = self.color.create_view(&Default::default());
        let depth = self.depth.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Librarian model preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.035,
                            g: 0.04,
                            b: 0.05,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(draw) = &self.draw {
                draw.draw(&mut pass);
            }
        }
        queue.submit(Some(encoder.finish()));
    }
    pub fn action_names(&self) -> Vec<String> {
        self.asset
            .as_ref()
            .map(|asset| {
                asset
                    .model
                    .actions
                    .iter()
                    .map(|action| action.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn pan_by(&mut self, delta: egui::Vec2) {
        let right = Vec3::new(self.yaw.cos(), 0., -self.yaw.sin());
        let up = Vec3::Y;
        self.pan += (-right * delta.x + up * delta.y) * self.radius * self.zoom * 0.004;
    }
    fn frame(&self) -> Frame {
        let matrix = camera_matrix(
            self.center + self.pan,
            self.radius,
            self.zoom,
            self.yaw,
            self.pitch,
        );
        Frame {
            blend_enabled: false,
            matrix: matrix.to_cols_array_2d(),
            lighting: Lighting {
                environment_ambient: [0.6, 0.6, 0.6, 1.],
                additional_ambient: [0.; 4],
                directional_diffuse: [0.7, 0.7, 0.7, 1.],
                point_ambient: [0.; 4],
                point_diffuse: [0.; 4],
                transparency: -1.,
                point_enabled: false,
            },
            direction: [0.3, -0.8, 0.5],
            point_position: [0.; 3],
            attenuation: [0., 0.4, 0.],
            eye_direction: [0., 0., 1.],
            directional_specular: [0.; 4],
            specular_variant: false,
            vertex_fog: 1.,
            fog_color: [0.; 4],
            fog_range: [0.; 4],
            clip_plane: [0.; 4],
            reverse_front: false,
            alpha_compare: 5,
            alpha_reference: 84. / 255.,
        }
    }
}

fn camera_matrix(target: Vec3, radius: f32, zoom: f32, yaw: f32, pitch: f32) -> Mat4 {
    let distance = radius * 3.5 * zoom;
    let eye = target
        + Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ) * distance;
    let near = (distance * 0.01).max(0.001);
    let far = distance + radius * 10.;
    Mat4::from_scale(Vec3::new(-1., 1., 1.))
        * Mat4::perspective_rh(45f32.to_radians(), 1., far, near)
        * Mat4::look_at_rh(eye, target, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_camera_mirrors_x_and_uses_reversed_depth() {
        let matrix = camera_matrix(Vec3::ZERO, 1., 1., 0., 0.);
        let projected = |p: Vec3| {
            let p = matrix * p.extend(1.);
            p.truncate() / p.w
        };
        assert!(projected(Vec3::X).x < 0.);
        assert!(projected(Vec3::new(0., 0., 2.)).z > projected(Vec3::new(0., 0., -2.)).z);
    }
}

fn load_worn(assets: &mut LibrarianAssets, request: &str) -> Result<ModelAsset, String> {
    use crate::modules::designer::catalog::{Class, Gender};
    let (class, remainder) = request.split_once(':').ok_or("invalid equipment preview")?;
    let (gender, realname) = remainder
        .split_once(':')
        .ok_or("invalid equipment preview")?;
    let class = class
        .parse::<usize>()
        .ok()
        .and_then(|index| Class::ALL.get(index))
        .copied()
        .ok_or("invalid mannequin class")?;
    let gender = if gender == "f" {
        Gender::Female
    } else {
        Gender::Male
    };
    if !class.allows(gender) {
        return Err("class and gender are incompatible".into());
    }
    let mut asset = ModelAsset::load_without_skins_with(&class.body_model(gender), |path| {
        assets.read_model_resource(path)
    })?;
    let paths = [
        class.body_skin(gender, 0),
        class.equipment_skin(gender, realname, 0),
    ];
    for path in paths {
        let data = crate::skin::Skin::parse(&assets.read_model_resource(&path)?)
            .map_err(|error| format!("{path}: {error}"))?;
        let index = asset.skins.len();
        if index == 1 {
            asset
                .diffuse_alpha_meshes
                .extend((0..data.meshes.len()).map(|mesh| (index, mesh)));
        }
        asset.model.skins.push(path.clone());
        asset
            .skins
            .push(crate::model_asset::SkinAsset { path, data });
    }
    Ok(asset)
}
