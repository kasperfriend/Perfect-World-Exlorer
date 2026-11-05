//! Module-owned archive access.
//!
//! Populator keeps its own `models.pck`/`textures.pck` handles and entry
//! tables, so enabling it adds no state or caching behaviour to the core
//! loaders and disabling it drops all of it.

use crate::{ecmodel, pck};
use std::{collections::HashMap, fs::File, path::Path};

pub struct Archive {
    file: File,
    entries: HashMap<String, pck::Entry>,
    label: String,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
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

    pub fn len(&self) -> usize {
        self.entries.len()
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
