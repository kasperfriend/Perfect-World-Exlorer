//! Retain ECWLD model placements and shared model dependencies. Loading an
//! asset does not choose an action namespace or invent an animation schedule.
use crate::{ecmodel, model_asset::ModelAsset, scene::SceneModel};
use std::{collections::HashMap, fs::File, path::Path};

pub struct Instance {
    pub placement: SceneModel,
    pub asset: usize,
    /// Effective combined action after the source idle fallback; None is
    /// a non-playing model, not a direct-SMD-name dispatch.
    pub action: Option<usize>,
    pub playback: Result<crate::model_playback::Playback, String>,
}
pub struct Assets {
    pub assets: Vec<ModelAsset>,
    pub instances: Vec<Instance>,
    pub failures: Vec<String>,
    pub unloaded: Vec<SceneModel>,
}
impl Assets {
    /// Failed dependencies remain visible as named failures, not substituted
    /// meshes. A failed shared asset is read once, not retried per placement.
    pub fn load_with(
        placements: Vec<SceneModel>,
        mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Self {
        let mut result = Self {
            assets: vec![],
            instances: vec![],
            failures: vec![],
            unloaded: vec![],
        };
        let mut cache: HashMap<String, Option<usize>> = HashMap::new();
        for placement in placements {
            let path = ecmodel::normalize(&placement.path);
            let asset = if let Some(index) = cache.get(&path) {
                *index
            } else {
                let index = match ModelAsset::load_with(&path, &mut read) {
                    Ok(asset) => {
                        let index = result.assets.len();
                        result.assets.push(asset);
                        Some(index)
                    }
                    Err(e) => {
                        result.failures.push(e);
                        None
                    }
                };
                cache.insert(path, index);
                index
            };
            if let Some(asset) = asset {
                let action = result.assets[asset]
                    .descriptor
                    .scene_action(&placement.action);
                let playback = crate::model_playback::Playback::new(&result.assets[asset], action);
                result.instances.push(Instance {
                    placement,
                    asset,
                    action,
                    playback,
                });
            } else {
                result.unloaded.push(placement);
            }
        }
        result
    }
    pub fn open(element: &Path, placements: Vec<SceneModel>) -> Result<Self, String> {
        if placements.is_empty() {
            return Ok(Self::load_with(placements, |_| unreachable!()));
        }
        let mut file =
            File::open(element.join("models.pck")).map_err(|e| format!("models.pck: {e}"))?;
        let mut entries = HashMap::new();
        for entry in crate::pck::directory(&mut file)? {
            let name = ecmodel::normalize(&entry.name);
            if entries.insert(name.clone(), entry).is_some() {
                return Err(format!("duplicate normalized models.pck entry: {name}"));
            }
        }
        Ok(Self::load_with(placements, |path| {
            let entry = entries
                .get(path)
                .ok_or_else(|| format!("missing models.pck entry: {path}"))?;
            crate::pck::read_entry(&mut file, entry)
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_shared_assets_are_read_once_and_named() {
        let mut reads = 0;
        let placement = SceneModel {
            export_id: 7,
            position: [0.; 3],
            direction: [0., 0., 1.],
            up: [0., 1., 0.],
            path: "Models\\missing.ecm".into(),
            action: "visible".into(),
        };
        let assets = Assets::load_with(vec![placement.clone(), placement], |path| {
            reads += 1;
            assert_eq!(path, "models/missing.ecm");
            Err("missing dependency".into())
        });
        assert_eq!(reads, 1);
        assert!(assets.instances.is_empty());
        assert_eq!(assets.failures.len(), 1);
        assert!(assets.failures[0].contains("models/missing.ecm: missing dependency"));
    }
}
