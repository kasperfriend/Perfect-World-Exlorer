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
        let set = self
            .tracks
            .get(action)
            .ok_or("model action index out of range")?;
        let tracks = match set {
            Some(t) => &t.tracks,
            None if !self.skeleton.embedded_tracks.is_empty() => &self.skeleton.embedded_tracks,
            None => return Err("action has neither external nor embedded bone tracks".into()),
        };
        let pose = skeleton_pose::evaluate(&self.skeleton, tracks, seconds, world)?;
        let mut meshes = Vec::new();
        for (skin, asset) in self.skins.iter().enumerate() {
            let palette = skeleton_pose::palette(&asset.data, &self.skeleton, &pose)?;
            for (mesh, data) in asset.data.meshes.iter().enumerate() {
                // Rigid vertices have different binding-space semantics. Do not
                // pretend a weighted bind-palette transform implements them.
                if data.rigid_bone.is_some() {
                    return Err(format!("{}: rigid pose path not implemented", asset.path));
                }
                let mut vertices = Vec::with_capacity(data.vertices.len());
                for v in &data.vertices {
                    vertices.push(skin_pose::deform(
                        v.position,
                        v.normal,
                        [v.weights[0], v.weights[1], v.weights[2]],
                        v.joints,
                        &palette,
                    )?);
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
    #[test]
    fn read_failure_preserves_normalized_dependency_name() {
        let result = ModelAsset::load_with("Models\\a.ecm", |_| Err("not present".into()));
        assert!(matches!(result,Err(ref e) if e=="models/a.ecm: not present"));
    }
}
