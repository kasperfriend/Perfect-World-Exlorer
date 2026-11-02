//! Retained ordinary model geometry and animation data for a receiving-model
//! draw. No automatic ECM scheduling, textures/material draws or GFX playback
//! are implied by successfully loading this asset.
use crate::{animation, ecmodel, skeleton, skeleton_pose, skin, skin_pose};
use glam::Mat4;

pub struct SkinAsset {
    pub path: String,
    pub data: skin::Skin,
}
impl SkinAsset {
    /// Retail 0x794952–0x794add tries <ski-dir>/Tex_<ski-stem>/
    /// first, then <ski-dir>/Textures/. It retries on texture-load failure,
    /// not only when the preferred file is absent.
    pub fn texture_paths(&self, index: usize) -> Result<[String; 2], String> {
        let texture = self
            .data
            .textures
            .get(index)
            .ok_or("skin texture index out of range")?;
        let path = ecmodel::normalize(&self.path);
        let (parent, file) = path.rsplit_once('/').unwrap_or(("", &path));
        let stem = file.rsplit_once('.').map_or(file, |x| x.0);
        Ok([
            ecmodel::normalize(&format!("{parent}/tex_{stem}/{texture}")),
            ecmodel::normalize(&format!("{parent}/textures/{texture}")),
        ])
    }
    pub fn load_texture_with<T>(
        &self,
        index: usize,
        mut load: impl FnMut(&str) -> Result<T, String>,
    ) -> Result<T, String> {
        let paths = self.texture_paths(index)?;
        let mut failures = Vec::new();
        for path in paths {
            match load(&path) {
                Ok(t) => return Ok(t),
                Err(e) => failures.push(format!("{path}: {e}")),
            }
        }
        Err(failures.join("; "))
    }
}
pub struct ModelAsset {
    pub descriptor: ecmodel::EcModel,
    pub model: ecmodel::SkinModel,
    pub skeleton: skeleton::Skeleton,
    pub skins: Vec<SkinAsset>,
    /// Same order as model.actions. None denotes the older embedded BON data,
    /// not permission to silently substitute another action's external tracks.
    pub tracks: Vec<Option<animation::TrackSet>>,
}
/// World matrix of a named skeleton hook for an evaluated pose (the native
/// combination order is documented on `ModelAsset::hook_matrix`).
pub fn hook_matrix(
    skeleton: &skeleton::Skeleton,
    pose: &skeleton_pose::Pose,
    name: &str,
) -> Result<Mat4, String> {
    let hook = skeleton
        .hooks
        .iter()
        .find(|h| h.name == name)
        .ok_or_else(|| format!("model has no hook named {name:?}"))?;
    let local = Mat4::from_cols_array(&hook.matrix);
    if !local.is_finite() {
        return Err(format!("hook {name:?} has a non-finite matrix"));
    }
    if hook.bone < 0 {
        return Ok(pose.model * local);
    }
    let bone = usize::try_from(hook.bone)
        .ok()
        .and_then(|i| pose.absolute.get(i))
        .ok_or_else(|| format!("hook {name:?} binds missing bone {}", hook.bone))?;
    Ok(*bone * local)
}
pub struct PosedMesh {
    pub skin: usize,
    pub mesh: usize,
    pub vertices: Vec<skin_pose::PosedVertex>,
}
fn relative(descriptor: &str, reference: &str) -> String {
    let reference = ecmodel::normalize(reference);
    if reference.starts_with("models/") {
        reference
    } else {
        let parent = descriptor.rsplit_once('/').map_or("", |p| p.0);
        ecmodel::normalize(&format!("{parent}/{reference}"))
    }
}
impl ModelAsset {
    /// The caller owns archive I/O and caching. Every read receives a normalized
    /// package path. Errors retain the actual dependency path, including GBK
    /// names, instead of replacing missing geometry with a placeholder.
    pub fn load_with(
        descriptor_path: &str,
        mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Self, String> {
        let descriptor_path = ecmodel::normalize(descriptor_path);
        let mut bytes = |p: &str| read(p).map_err(|e| format!("{p}: {e}"));
        let descriptor = ecmodel::parse_ecm(&bytes(&descriptor_path)?)
            .map_err(|e| format!("{descriptor_path}: {e}"))?;
        let model = ecmodel::parse_smd(&bytes(&descriptor.skin_model_path)?)
            .map_err(|e| format!("{}: {e}", descriptor.skin_model_path))?;
        let skeleton_path = relative(&descriptor.skin_model_path, &model.skeleton);
        let skeleton = skeleton::Skeleton::parse(&bytes(&skeleton_path)?)
            .map_err(|e| format!("{skeleton_path}: {e}"))?;
        let mut skins = Vec::new();
        for reference in &model.skins {
            let path = relative(&descriptor.skin_model_path, reference);
            let data = skin::Skin::parse(&bytes(&path)?).map_err(|e| format!("{path}: {e}"))?;
            skins.push(SkinAsset { path, data });
        }
        let mut tracks = Vec::new();
        for action in &model.actions {
            let set =
                if let Some(path) = model.action_track_path(&descriptor.skin_model_path, action) {
                    let t = animation::TrackSet::parse(&bytes(&path)?)?;
                    t.validate_joint_count(skeleton.joints.len())?;
                    Some(t)
                } else {
                    None
                };
            tracks.push(set);
        }
        for combined in &descriptor.actions {
            for base in &combined.bases {
                if !model.actions.iter().any(|a| a.name == base.name) {
                    return Err(format!(
                        "{descriptor_path}: {} -> missing SMD action {}",
                        combined.name, base.name
                    ));
                }
            }
        }
        Ok(Self {
            descriptor,
            model,
            skeleton,
            skins,
            tracks,
        })
    }
    /// Absolute track time in seconds. Intentionally NOT an elapsed-time
    /// scheduler: action-loop duration, event timing, combined/direct namespace
    /// priority and random-loop selection must be supplied by verified playback.
    pub fn pose_action_at_time(
        &self,
        action: usize,
        seconds: f32,
        world: Mat4,
    ) -> Result<Vec<PosedMesh>, String> {
        let pose = self.action_pose(action, seconds, world)?;
        self.meshes_from_pose(&pose)
    }
    /// The evaluated pose itself, for consumers that need bone/hook matrices
    /// (an attached effect resolves its hook from this pose, not from the
    /// deformed vertices).
    pub fn action_pose(
        &self,
        action: usize,
        seconds: f32,
        world: Mat4,
    ) -> Result<skeleton_pose::Pose, String> {
        let set = self
            .tracks
            .get(action)
            .ok_or("model action index out of range")?;
        let tracks = match set {
            Some(t) => &t.tracks,
            None if !self.skeleton.embedded_tracks.is_empty() => &self.skeleton.embedded_tracks,
            None => return Err("action has neither external nor embedded bone tracks".into()),
        };
        skeleton_pose::evaluate(&self.skeleton, tracks, seconds, world)
    }
    /// No active action: use authored local bone transforms, not the first
    /// named animation. This is also useful for inspecting scene-play failures.
    pub fn pose_bind(&self, world: Mat4) -> Result<Vec<PosedMesh>, String> {
        let pose = self.bind_pose(world)?;
        self.meshes_from_pose(&pose)
    }
    pub fn bind_pose(&self, world: Mat4) -> Result<skeleton_pose::Pose, String> {
        skeleton_pose::evaluate(&self.skeleton, &[], 0., world)
    }
    /// World matrix of a named skeleton hook for an evaluated pose.
    ///
    /// 0x7b25e6 combines the authored hook matrix with the posed bone matrix for
    /// a bone-bound hook; 0x7b2543 combines it with the model matrix when the
    /// hook's bind index is negative. The client stores row-vector matrices, so
    /// its `hook * bone` product is `bone * hook` in this column-vector reader.
    /// A hook name the model does not own is an explicit error, never the model
    /// origin.
    pub fn hook_matrix(&self, pose: &skeleton_pose::Pose, name: &str) -> Result<Mat4, String> {
        hook_matrix(&self.skeleton, pose, name)
    }
    pub fn meshes_from_pose(&self, pose: &skeleton_pose::Pose) -> Result<Vec<PosedMesh>, String> {
        let mut meshes = Vec::new();
        for (skin, asset) in self.skins.iter().enumerate() {
            // Bind/validate all SKI names even for a rigid-only skin. Rigid
            // vertices below do not use this remapped weighted palette.
            let palette = skeleton_pose::palette(&asset.data, &self.skeleton, pose)?;
            for (mesh, data) in asset.data.meshes.iter().enumerate() {
                let rigid = data
                    .rigid_bone
                    .map(|bone| skeleton_pose::rigid_matrix(pose, bone))
                    .transpose()?;
                let mut vertices = Vec::with_capacity(data.vertices.len());
                for v in &data.vertices {
                    vertices.push(if let Some(matrix) = rigid {
                        skin_pose::deform_rigid(v.position, v.normal, matrix)?
                    } else {
                        skin_pose::deform(
                            v.position,
                            v.normal,
                            [v.weights[0], v.weights[1], v.weights[2]],
                            v.joints,
                            &palette,
                        )?
                    });
                }
                meshes.push(PosedMesh {
                    skin,
                    mesh,
                    vertices,
                });
            }
        }
        Ok(meshes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn skin() -> SkinAsset {
        SkinAsset {
            path: "Models/A/test.extra.ski".into(),
            data: skin::Skin {
                version: 9,
                bones: Vec::new(),
                textures: vec!["IMAGE.DDS".into()],
                materials: Vec::new(),
                meshes: Vec::new(),
            },
        }
    }
    #[test]
    fn texture_order_matches_skin_specific_then_generic_directory() {
        let s = skin();
        assert_eq!(
            s.texture_paths(0).unwrap(),
            [
                "models/a/tex_test.extra/image.dds",
                "models/a/textures/image.dds"
            ]
        );
        let mut visits = Vec::new();
        let result = s
            .load_texture_with(0, |p| {
                visits.push(p.to_owned());
                Ok(7)
            })
            .unwrap();
        assert_eq!(result, 7);
        assert_eq!(visits.len(), 1);
        assert!(s.texture_paths(1).is_err());
    }
    #[test]
    fn texture_decoder_failure_retries_fallback_and_keeps_both_errors() {
        let s = skin();
        let mut visits = Vec::new();
        let result = s
            .load_texture_with(0, |p| {
                visits.push(p.to_owned());
                if p.contains("/textures/") {
                    Ok(9)
                } else {
                    Err("bad DDS".into())
                }
            })
            .unwrap();
        assert_eq!(result, 9);
        assert_eq!(visits.len(), 2);
        let error = s
            .load_texture_with::<()>(0, |_| Err("bad image".into()))
            .unwrap_err();
        assert!(error.contains("tex_test.extra/image.dds: bad image"));
        assert!(error.contains("textures/image.dds: bad image"));
    }
    #[test]
    fn package_references_keep_rooted_paths_and_resolve_local_names() {
        assert_eq!(
            relative("models/a/test.smd", "test.bon"),
            "models/a/test.bon"
        );
        assert_eq!(
            relative("models/a/test.smd", "Models\\other\\test.ski"),
            "models/other/test.ski"
        );
    }
    fn hook_file() -> skeleton::Skeleton {
        let mut s = skeleton::Skeleton {
            version: 6,
            start: 0,
            end: 0,
            fps: 0,
            bones: Vec::new(),
            joints: Vec::new(),
            hooks: Vec::new(),
            embedded_tracks: Vec::new(),
        };
        s.hooks.push(skeleton::Hook {
            name: "weapon".into(),
            kind: 0,
            bone: 0,
            matrix: Mat4::from_translation(glam::Vec3::new(1., 0., 0.)).to_cols_array(),
        });
        s.hooks.push(skeleton::Hook {
            name: "root".into(),
            kind: 0,
            bone: -1,
            matrix: Mat4::from_translation(glam::Vec3::new(2., 0., 0.)).to_cols_array(),
        });
        s
    }

    fn hook_pose() -> skeleton_pose::Pose {
        skeleton_pose::Pose {
            absolute: vec![
                Mat4::from_translation(glam::Vec3::new(0., 2., 0.)),
                Mat4::from_translation(glam::Vec3::new(0., 4., 0.)),
            ],
            skin: Vec::new(),
            model: Mat4::from_translation(glam::Vec3::new(5., 0., 0.)),
        }
    }

    #[test]
    fn hook_matrix_binds_the_posed_bone_and_composes_the_hook_record() {
        let s = hook_file();
        let pose = hook_pose();
        let world = hook_matrix(&s, &pose, "weapon").unwrap();
        let point = world.transform_point3(glam::Vec3::ZERO);
        assert!(
            (point - glam::Vec3::new(1., 2., 0.)).length() < 1e-6,
            "{point}"
        );
    }

    #[test]
    fn negative_bind_index_uses_the_model_matrix() {
        let s = hook_file();
        let pose = hook_pose();
        let world = hook_matrix(&s, &pose, "root").unwrap();
        let point = world.transform_point3(glam::Vec3::ZERO);
        assert!(
            (point - glam::Vec3::new(7., 0., 0.)).length() < 1e-6,
            "{point}"
        );
    }

    #[test]
    fn hook_world_follows_a_moving_bone_or_model_across_frames() {
        let s = hook_file();
        // Bone-bound hook: 0x7b25e6 composes bone * hook, so a bone that moves
        // by d moves the attached root by exactly d on the next evaluated pose.
        let mut pose = hook_pose();
        let first = hook_matrix(&s, &pose, "weapon")
            .unwrap()
            .transform_point3(glam::Vec3::ZERO);
        pose.absolute[0] = Mat4::from_translation(glam::Vec3::new(3., 2., 0.));
        let second = hook_matrix(&s, &pose, "weapon")
            .unwrap()
            .transform_point3(glam::Vec3::ZERO);
        assert!(
            (second - first - glam::Vec3::new(3., 0., 0.)).length() < 1e-6,
            "{first} -> {second}"
        );
        // Model-bound hook (negative bind index): 0x7b2543 composes
        // model * hook and applies the model matrix once, not per frame.
        let mut pose = hook_pose();
        let first = hook_matrix(&s, &pose, "root")
            .unwrap()
            .transform_point3(glam::Vec3::ZERO);
        pose.model = Mat4::from_translation(glam::Vec3::new(9., 0., 0.));
        let second = hook_matrix(&s, &pose, "root")
            .unwrap()
            .transform_point3(glam::Vec3::ZERO);
        assert!(
            (second - first - glam::Vec3::new(4., 0., 0.)).length() < 1e-6,
            "{first} -> {second}"
        );
        assert!((second - glam::Vec3::new(11., 0., 0.)).length() < 1e-6);
    }

    #[test]
    fn unknown_hook_and_missing_bone_are_explicit_errors() {
        let s = hook_file();
        let pose = hook_pose();
        let error = hook_matrix(&s, &pose, "shield").unwrap_err();
        assert!(error.contains("no hook named"), "{error}");
        let mut broken = hook_file();
        broken.hooks[0].bone = 9;
        let error = hook_matrix(&broken, &pose, "weapon").unwrap_err();
        assert!(error.contains("binds missing bone 9"), "{error}");
        let mut bad = hook_file();
        bad.hooks[0].matrix[0] = f32::NAN;
        let error = hook_matrix(&bad, &pose, "weapon").unwrap_err();
        assert!(error.contains("non-finite matrix"), "{error}");
    }

    #[test]
    fn read_failure_preserves_normalized_dependency_name() {
        let result = ModelAsset::load_with("Models\\a.ecm", |_| Err("not present".into()));
        assert!(matches!(result,Err(ref e) if e=="models/a.ecm: not present"));
    }
}
