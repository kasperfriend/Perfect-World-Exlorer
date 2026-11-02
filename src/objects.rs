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
    pub fn update(&mut self, world_x: f32, world_z: f32, range: f32) -> Result<Vec<usize>, String> {
        let ids: Vec<usize> = self
            .ornaments
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                let dx = o.position[0] - world_x;
                let dz = o.position[2] - world_z;
                dx * dx + dz * dz < range * range
            })
            .map(|(i, _)| i)
            .collect();
        self.load_ids(&ids)?;
        Ok(ids)
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
