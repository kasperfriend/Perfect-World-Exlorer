//! Module-owned models.pck access. The add-on keeps its own file and entry
//! table, so enabling Walker adds no state or caching behaviour to core loaders.

use crate::{ecmodel, pck};
use std::{collections::HashMap, fs::File, path::Path};

pub struct Archive {
    file: File,
    entries: HashMap<String, pck::Entry>,
    label: String,
}

impl Archive {
    pub fn open(element: &Path) -> Result<Self, String> {
        let path = element.join("models.pck");
        Self::open_archive(&path)
    }

    pub fn open_archive(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = pck::directory(&mut file)?
            .into_iter()
            .map(|entry| (ecmodel::normalize(&entry.name), entry))
            .collect();
        Ok(Self {
            file,
            entries,
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("archive")
                .to_owned(),
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(&ecmodel::normalize(path))
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let normalized = ecmodel::normalize(path);
        let entry = self
            .entries
            .get(&normalized)
            .ok_or_else(|| format!("missing {} entry: {normalized}", self.label))?;
        pck::read_entry(&mut self.file, entry)
    }
}
