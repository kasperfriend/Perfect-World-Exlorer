//! Retain ECWLD model placements and shared model dependencies. Loading an
//! asset does not choose an action namespace or invent an animation schedule.
use crate::{
    ecmodel,
    model_asset::{ModelAsset, TrackCache},
    scene::SceneModel,
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

pub struct Instance {
    pub placement: SceneModel,
    pub asset: usize,
    /// Effective combined action after the source idle fallback; None is
    /// a non-playing model, not a direct-SMD-name dispatch.
    pub action: Option<usize>,
    pub playback: Result<crate::model_playback::Playback, String>,
}
#[derive(Default)]
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
        let mut tracks = crate::model_asset::TrackCache::default();
        for placement in placements {
            let path = ecmodel::normalize(&placement.path);
            let asset = if let Some(index) = cache.get(&path) {
                *index
            } else {
                let index = match ModelAsset::load_cached(&path, &mut read, &mut tracks) {
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
            return Ok(Self::default());
        }
        let archive = element.join("models.pck");
        let mut index_file =
            File::open(&archive).map_err(|e| format!("models.pck: {e}"))?;
        let mut entries = HashMap::new();
        for entry in crate::pck::directory(&mut index_file)? {
            let name = ecmodel::normalize(&entry.name);
            if entries.insert(name.clone(), entry).is_some() {
                return Err(format!("duplicate normalized models.pck entry: {name}"));
            }
        }
        let mut seen = HashSet::new();
        let paths: Vec<String> = placements
            .iter()
            .map(|placement| ecmodel::normalize(&placement.path))
            .filter(|path| seen.insert(path.clone()))
            .collect();
        let entry_index = Arc::new(entries);
        let next = AtomicUsize::new(0);
        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(4)
            .min(paths.len());
        let (send, receive) = std::sync::mpsc::sync_channel(worker_count * 2);
        let loaded = std::thread::scope(|scope| -> Result<Vec<_>, String> {
            for _ in 0..worker_count {
                let send = send.clone();
                let archive = &archive;
                let entries = Arc::clone(&entry_index);
                let next = &next;
                let paths = &paths;
                scope.spawn(move || {
                    let mut file = File::open(archive);
                    let open_error = file.as_ref().err().map(ToString::to_string);
                    let mut tracks = TrackCache::default();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(index) else {
                            break;
                        };
                        let asset = if let Some(error) = &open_error {
                            Err(format!("models.pck: {error}"))
                        } else {
                            ModelAsset::load_cached(
                                path,
                                |dependency| {
                                    let entry = entries.get(dependency).ok_or_else(|| {
                                        format!("missing models.pck entry: {dependency}")
                                    })?;
                                    crate::pck::read_entry(file.as_mut().unwrap(), entry)
                                },
                                &mut tracks,
                            )
                        };
                        if send.send((index, asset)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(send);
            let mut loaded: Vec<Option<Result<ModelAsset, String>>> =
                (0..paths.len()).map(|_| None).collect();
            for _ in 0..paths.len() {
                let (index, asset) = receive
                    .recv()
                    .map_err(|_| "world-model workers stopped early".to_string())?;
                loaded[index] = Some(asset);
            }
            Ok(loaded
                .into_iter()
                .map(|asset| asset.expect("each unique model path is assigned once"))
                .collect())
        })?;
        let loaded_by_path = paths.into_iter().zip(loaded).collect();
        Ok(Self::from_loaded(placements, loaded_by_path))
    }

    fn from_loaded(
        placements: Vec<SceneModel>,
        mut loaded: HashMap<String, Result<ModelAsset, String>>,
    ) -> Self {
        let mut result = Self::default();
        let mut cache: HashMap<String, Option<usize>> = HashMap::new();
        for placement in placements {
            let path = ecmodel::normalize(&placement.path);
            let asset = if let Some(index) = cache.get(&path) {
                *index
            } else {
                let index = match loaded.remove(&path).unwrap_or_else(|| {
                    Err(format!("missing parallel world-model result: {path}"))
                }) {
                    Ok(asset) => {
                        let index = result.assets.len();
                        result.assets.push(asset);
                        Some(index)
                    }
                    Err(error) => {
                        result.failures.push(error);
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
}
/// 1.4.2 0x6d4304 -> 0x7b0d60 -> 0x788960. Normalize forward/up
/// separately, then their cross product; do NOT silently orthogonalize up.
pub fn transform(placement: &SceneModel) -> Result<glam::Mat4, String> {
    let position = glam::Vec3::from_array(placement.position);
    let direction = glam::Vec3::from_array(placement.direction);
    let up = glam::Vec3::from_array(placement.up);
    if !position.is_finite()
        || !direction.is_finite()
        || !up.is_finite()
        || direction.length_squared() < 1e-12
        || up.length_squared() < 1e-12
    {
        return Err("invalid world model placement basis".into());
    }
    let direction = direction.normalize();
    let up = up.normalize();
    let right = up.cross(direction);
    if right.length_squared() < 1e-12 {
        return Err("parallel world model placement axes".into());
    }
    Ok(glam::Mat4::from_cols(
        right.normalize().extend(0.),
        up.extend(0.),
        direction.extend(0.),
        position.extend(1.),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_basis_normalizes_axes_but_preserves_nonorthogonal_up() {
        let p = SceneModel {
            export_id: 1,
            position: [10., 2., -3.],
            direction: [0., 0., 2.],
            up: [0., 2., 2.],
            path: String::new(),
            action: String::new(),
        };
        let m = transform(&p).unwrap();
        assert_eq!(m.w_axis.to_array(), [10., 2., -3., 1.]);
        for (value, expected) in m.x_axis.to_array().into_iter().zip([1., 0., 0., 0.]) {
            assert!((value - expected).abs() < 1e-6);
        }
        assert_eq!(m.z_axis.to_array(), [0., 0., 1., 0.]);
        assert!((m.y_axis.z - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        let mut p = p;
        p.up = p.direction;
        assert!(transform(&p).is_err());
        p.up = [0., 1., 0.];
        p.position[0] = f32::NAN;
        assert!(transform(&p).is_err());
    }
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
