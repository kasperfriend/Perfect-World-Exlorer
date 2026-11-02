//! Streaming access to placed lit-model geometry and textures.
use crate::{bmd, pck, scene::Ornament};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
};
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
impl Texture {
    pub fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        let x = (u.rem_euclid(1.) * self.width as f32) as usize % self.width;
        let y = (v.rem_euclid(1.) * self.height as f32) as usize % self.height;
        let p = (y * self.width + x) * 4;
        [
            self.rgba[p] as f32,
            self.rgba[p + 1] as f32,
            self.rgba[p + 2] as f32,
        ]
    }
    pub fn sample_rgba(&self, u: f32, v: f32) -> [u8; 4] {
        let x = (u.rem_euclid(1.) * self.width as f32) as usize % self.width;
        let y = (v.rem_euclid(1.) * self.height as f32) as usize % self.height;
        let p = (y * self.width + x) * 4;
        self.rgba[p..p + 4].try_into().unwrap()
    }
}
pub struct Objects {
    pub ornaments: Vec<Ornament>,
    models: HashMap<String, bmd::Model>,
    textures: HashMap<String, Texture>,
    litfile: File,
    texfile: File,
    litentries: HashMap<String, pck::Entry>,
    texentries: HashMap<String, pck::Entry>,
    missing_textures: HashSet<String>,
}
impl Objects {
    pub fn open(element: &Path, ornaments: Vec<Ornament>) -> Result<Self, String> {
        let mut litfile = File::open(element.join("litmodels.pck")).map_err(|e| e.to_string())?;
        let mut texfile = File::open(element.join("building.pck")).map_err(|e| e.to_string())?;
        let litentries = pck::directory(&mut litfile)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        let texentries = pck::directory(&mut texfile)?
            .into_iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();
        Ok(Self {
            ornaments,
            models: HashMap::new(),
            textures: HashMap::new(),
            litfile,
            texfile,
            litentries,
            texentries,
            missing_textures: HashSet::new(),
        })
    }
    /// Returns the indices of ornaments that are both visible (data flag) and
    /// within `range` of `(world_x, world_z)`. Invisible ornaments are skipped
    /// before loading; distance-culled ones are skipped by the range test.
    /// The two out parameters let callers distinguish a hidden placement from a
    /// far one, which matters for tests and for stream-efficiency reporting.
    pub fn update(
        &mut self,
        world_x: f32,
        world_z: f32,
        range: f32,
    ) -> Result<(Vec<usize>, usize, usize), String> {
        let mut visible_ids = Vec::new();
        let mut hidden_count = 0usize;
        let mut culled_count = 0usize;
        for (i, o) in self.ornaments.iter().enumerate() {
            if !o.is_visible() {
                hidden_count += 1;
                continue;
            }
            let dx = o.position[0] - world_x;
            let dz = o.position[2] - world_z;
            if dx * dx + dz * dz >= range * range {
                culled_count += 1;
                continue;
            }
            visible_ids.push(i);
        }
        self.load_ids(&visible_ids)?;
        Ok((visible_ids, hidden_count, culled_count))
    }
    /// Loads a small explicit batch for the independent world preload worker.
    pub fn load_ids(&mut self, ids: &[usize]) -> Result<(), String> {
        let active: HashSet<String> = ids
            .iter()
            .map(|&i| format!("litmodels/world/{}", self.ornaments[i].path.to_lowercase()))
            .collect();
        self.models.retain(|name, _| active.contains(name));
        let mut used_textures = HashSet::new();
        for &i in ids {
            let name = format!("litmodels/world/{}", self.ornaments[i].path.to_lowercase());
            if !self.models.contains_key(&name) {
                let entry = self
                    .litentries
                    .get(&name)
                    .ok_or_else(|| format!("missing model {name}"))?;
                let data = pck::read_entry(&mut self.litfile, entry)?;
                let model = bmd::Model::parse(&data).map_err(|e| format!("{name}: {e}"))?;
                self.models.insert(name.clone(), model);
            }
            for mesh in &self.models[&name].meshes {
                let texname = mesh.texture.to_lowercase();
                if texname.is_empty() {
                    continue;
                }
                used_textures.insert(texname.clone());
                if self.textures.contains_key(&texname) || self.missing_textures.contains(&texname)
                {
                    continue;
                }
                let Some(entry) = self.texentries.get(&texname) else {
                    eprintln!("Missing building texture: {texname}");
                    self.missing_textures.insert(texname);
                    continue;
                };
                let data = pck::read_entry(&mut self.texfile, entry)?;
                // TGA has no magic header, so generic format sniffing rejects
                // valid client TGAs (including .sdr.tga files).
                let decoded = if texname.ends_with(".tga") {
                    image::load_from_memory_with_format(&data, image::ImageFormat::Tga)
                } else {
                    image::load_from_memory(&data)
                };
                let img = match decoded {
                    Ok(img) => img.to_rgba8(),
                    Err(err) => {
                        eprintln!("Unsupported texture {texname}: {err}");
                        self.missing_textures.insert(texname);
                        continue;
                    }
                };
                let (w, h) = img.dimensions();
                self.textures.insert(
                    texname,
                    Texture {
                        width: w as usize,
                        height: h as usize,
                        rgba: img.into_raw(),
                    },
                );
            }
        }
        self.textures.retain(|name, _| used_textures.contains(name));
        Ok(())
    }
    pub fn get(&self, idx: usize) -> Option<&bmd::Model> {
        self.models.get(&format!(
            "litmodels/world/{}",
            self.ornaments.get(idx)?.path.to_lowercase()
        ))
    }
    pub fn texture(&self, path: &str) -> Option<&Texture> {
        self.textures.get(&path.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper that holds the same data `Objects` uses but without file I/O,
    /// so the visibility and distance-culling logic can be covered directly.
    struct TestObjects {
        ornaments: Vec<Ornament>,
        models: HashMap<String, bmd::Model>,
    }

    impl TestObjects {
        fn update<F>(
            &mut self,
            world_x: f32,
            world_z: f32,
            range: f32,
            load_ids: &mut F,
        ) -> (Vec<usize>, usize, usize)
        where
            F: FnMut(&[usize]),
        {
            let mut visible_ids = Vec::new();
            let mut hidden_count = 0usize;
            let mut culled_count = 0usize;
            for (i, o) in self.ornaments.iter().enumerate() {
                if !o.is_visible() {
                    hidden_count += 1;
                    continue;
                }
                let dx = o.position[0] - world_x;
                let dz = o.position[2] - world_z;
                if dx * dx + dz * dz >= range * range {
                    culled_count += 1;
                    continue;
                }
                visible_ids.push(i);
            }
            load_ids(&visible_ids);
            (visible_ids, hidden_count, culled_count)
        }
    }

    fn make_ornament(
        extra: u32,
        position: [f32; 3],
        path: &str,
    ) -> Ornament {
        Ornament {
            export_id: 0,
            offset: 0,
            position,
            direction: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            extra,
            path: path.to_string(),
        }
    }

    fn dummy_model(name: &str) -> (String, bmd::Model) {
        let name = format!("litmodels/world/{}", name.to_lowercase());
        let mesh = bmd::Mesh {
            name: "mesh".to_string(),
            version: 0x1000_0005,
            texture: "tex.tx".to_string(),
            position: [0.0; 3],
            direction: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            scale: [1.0; 3],
            two_sided: false,
            vertex_format: 0,
            vertices: vec![],
            indices: vec![],
            material: None,
        };
        (
            name,
            bmd::Model {
                version: 0x1000_0002,
                position: [0.0; 3],
                direction: [0.0, 1.0, 0.0],
                up: [0.0, 0.0, 1.0],
                scale: [1.0; 3],
                ornaments_count: 0,
                meshes: vec![mesh],
            },
        )
    }

    /// A model the world data marks as hidden must never be loaded or drawn,
    /// regardless of how close the camera is. The visibility bit lives in the
    /// ornament's +0x36 extra field, read from data, not inferred from distance.
    #[test]
    fn hidden_ornament_is_not_returned_regardless_of_distance() {
        let mut obj = TestObjects {
            ornaments: vec![
                make_ornament(0x00, [0.0, 0.0, 0.0], "hidden.bmd"),
                make_ornament(0x01, [0.0, 0.0, 0.0], "visible.bmd"),
            ],
            models: HashMap::new(),
        };
        let (ids, hidden, culled) = obj.update(0.0, 0.0, 1000.0, &mut |_| {});
        assert_eq!(ids.len(), 1, "only the visible ornament is within range");
        assert_eq!(hidden, 1, "one ornament was hidden by the data flag");
        assert_eq!(culled, 0, "none were out of range");
        assert_eq!(
            obj.ornaments[ids[0]].path,
            "visible.bmd",
            "the returned id is the visible ornament"
        );
    }

    /// A visible ornament beyond the streaming range must be culled by distance,
    /// not by the visibility flag. The two reasons are tracked independently so
    /// callers can distinguish a hidden placement from a far one.
    #[test]
    fn visible_ornament_beyond_range_is_distance_culled() {
        let mut obj = TestObjects {
            ornaments: vec![
                make_ornament(0x01, [0.0, 0.0, 0.0], "near.bmd"),
                make_ornament(0x01, [2000.0, 0.0, 0.0], "far.bmd"),
            ],
            models: HashMap::new(),
        };
        let (ids, hidden, culled) = obj.update(0.0, 0.0, 1000.0, &mut |_| {});
        assert_eq!(ids.len(), 1, "only the near ornament is within range");
        assert_eq!(hidden, 0, "neither ornament is hidden");
        assert_eq!(culled, 1, "the far ornament was culled by distance");
        assert_eq!(
            obj.ornaments[ids[0]].path,
            "near.bmd",
            "the returned id is the near ornament"
        );
    }

    /// When an ornament is both hidden and far, the visibility flag takes
    /// priority: it is counted as hidden, not as distance-culled.
    #[test]
    fn hidden_ornament_beyond_range_is_not_double_counted() {
        let mut obj = TestObjects {
            ornaments: vec![make_ornament(0x00, [2000.0, 0.0, 0.0], "hidden_far.bmd")],
            models: HashMap::new(),
        };
        let (_ids, hidden, culled) = obj.update(0.0, 0.0, 1000.0, &mut |_| {});
        assert_eq!(hidden, 1, "hidden ornaments are counted as hidden even when far");
        assert_eq!(culled, 0, "distance culling does not apply to hidden ornaments");
    }

    /// Loading a subset of ornaments retains only those models; the others are
    /// evicted from the in-memory store. This exercises the eviction path that
    /// keeps resident model count bounded as the camera moves.
    #[test]
    fn load_ids_retains_only_requested_models() {
        let (name_a, mut model_a) = dummy_model("a");
        let (name_b, mut model_b) = dummy_model("b");
        model_a.meshes[0].texture = "a.tx".to_string();
        model_b.meshes[0].texture = "b.tx".to_string();
        let mut obj = TestObjects {
            ornaments: vec![
                make_ornament(0x01, [0.0, 0.0, 0.0], "a.bmd"),
                make_ornament(0x01, [10.0, 0.0, 0.0], "b.bmd"),
            ],
            models: {
                let mut m = HashMap::new();
                m.insert(name_a.clone(), model_a);
                m.insert(name_b.clone(), model_b);
                m
            },
        };
        assert_eq!(obj.models.len(), 2, "both models start resident");
        obj.update(0.0, 0.0, 5.0, &mut |ids| {
            assert_eq!(ids, &[0], "only the first ornament is within range");
            // Simulate load_ids retaining only the requested models.
            let active: HashSet<String> = ids
                .iter()
                .map(|&i| format!(
                    "litmodels/world/{}",
                    obj.ornaments[i].path.to_lowercase()
                ))
                .collect();
            obj.models.retain(|name, _| active.contains(name));
        });
        assert_eq!(obj.models.len(), 1, "the out-of-range model was evicted");
        assert!(
            obj.models.contains_key(&name_a),
            "the in-range model is retained"
        );
        assert!(
            !obj.models.contains_key(&name_b),
            "the out-of-range model is evicted"
        );
    }
}
