//! Module-owned models.pck access. The add-on keeps its own file and entry
//! table, so enabling Walker adds no state or caching behaviour to core loaders.

use crate::{ecmodel, pck};
use std::{collections::HashMap, fs::File, path::Path};

pub struct Archive {
    file: File,
    entries: HashMap<String, pck::Entry>,
}

impl Archive {
    pub fn open(element: &Path) -> Result<Self, String> {
        let path = element.join("models.pck");
        let mut file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = pck::directory(&mut file)?
            .into_iter()
            .map(|entry| (ecmodel::normalize(&entry.name), entry))
            .collect();
        Ok(Self { file, entries })
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let normalized = ecmodel::normalize(path);
        let entry = self
            .entries
            .get(&normalized)
            .ok_or_else(|| format!("missing models.pck entry: {normalized}"))?;
        pck::read_entry(&mut self.file, entry)
    }
}
