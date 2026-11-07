//! Module-owned models.pck access. The add-on keeps its own file and entry
//! table, so enabling Walker adds no state or caching behaviour to core loaders.

use crate::{ecmodel, pck};
use std::{collections::HashMap, fs::File, path::Path};

pub struct Archive {
    file: File,
    entries: HashMap<String, pck::Entry>,
    label: String,
    /// A second archive consulted when the first has no such member.
    ///
    /// `models.pck` holds meshes and model textures; the effect roots the
    /// gallery plays name `gfx/textures/...`, and those live in `gfx.pck`
    /// instead. Reading only `models.pck` made every effect texture miss, which
    /// the effect draw used to turn into a discarded bucket — the gallery drew
    /// nothing while reporting no error. The client keeps one `Gfx\` root for
    /// exactly this reason.
    secondary: Option<Box<Archive>>,
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
            secondary: None,
        })
    }

    /// Open `models.pck` with `gfx.pck` chained behind it, so one reader serves
    /// both the models and the effect roots and their textures.
    pub fn open_with_gfx(element: &Path) -> Result<Self, String> {
        let mut archive = Self::open(element)?;
        // A client without gfx.pck is a reported absence, not a refused module:
        // characters still load, only the effect maps fall back to the
        // placeholder the draw object carries.
        if let Ok(gfx) = Self::open_archive(&element.join("gfx.pck")) {
            archive.secondary = Some(Box::new(gfx));
        }
        Ok(archive)
    }

    pub fn contains(&self, path: &str) -> bool {
        let normalized = ecmodel::normalize(path);
        self.entries.contains_key(&normalized)
            || self
                .secondary
                .as_ref()
                .is_some_and(|next| next.contains(path))
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let normalized = ecmodel::normalize(path);
        match self.entries.get(&normalized) {
            Some(entry) => pck::read_entry(&mut self.file, entry),
            // Chains: the module's own archive first, the chained one second.
            None => match self.secondary.as_mut() {
                Some(next) => next.read(&normalized),
                None => Err(format!("missing {} entry: {normalized}", self.label)),
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const MASK: u32 = 0xa8937462;
    const CHECK: u32 = 0x59374231;

    /// The chained reader must answer from whichever archive holds the member,
    /// and must still answer from the module's own archive first.
    ///
    /// The defect this pins: the module opened only `models.pck`, so every
    /// `gfx/textures/...` lookup an effect root made came back "missing", and
    /// the effect draw turned that into a discarded bucket — a gallery that
    /// reported no error and drew no sprites at all.
    #[test]
    fn the_chained_reader_falls_through_to_the_second_archive() {
        let dir = std::env::temp_dir().join(format!("walker-archive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("models.pck");
        let second = dir.join("gfx.pck");
        std::fs::write(&first, pack(&[("models/a.bmp", b"model")])).unwrap();
        std::fs::write(&second, pack(&[("gfx/textures/x.dds", b"effect")])).unwrap();
        let mut archive = Archive::open_archive(&first).unwrap();
        archive.secondary = Some(Box::new(Archive::open_archive(&second).unwrap()));
        assert_eq!(archive.read("models/a.bmp").unwrap(), b"model");
        assert_eq!(archive.read("gfx/textures/x.dds").unwrap(), b"effect");
        // Case and separator folding applies on the chained side too.
        assert_eq!(archive.read("GFX\\Textures\\X.DDS").unwrap(), b"effect");
        assert!(archive.contains("gfx/textures/x.dds"));
        // A member in neither archive is still an error, not a silent success.
        assert!(archive.read("gfx/textures/absent.dds").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Build a real v2 PCK, stored (uncompressed) so the fixture needs no zlib.
    ///
    /// The layout is the one `crate::pck::directory` reads, which is why this
    /// earns its place in the test rather than mocking the reader: a fixture in
    /// an invented format would pass here and fail against every real archive.
    fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let table = 12;
        let mut records = Vec::new();
        let mut data = Vec::new();
        let data_start = table + entries.len() * 8 + entries.iter().map(|_| 276).sum::<usize>();
        for (name, bytes) in entries {
            let mut field = [0u8; 276];
            let raw = name.as_bytes();
            field[..raw.len()].copy_from_slice(raw);
            let offset = data_start + data.len();
            field[260..264].copy_from_slice(&(offset as u32).to_le_bytes());
            field[264..268].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            field[268..272].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            // Record: masked length, then the length itself obfuscated with the
            // archive's two keys, then the 276-byte fixed field.
            records.extend_from_slice(&(276u32 ^ MASK).to_le_bytes());
            records.extend_from_slice(&(276u32 ^ MASK ^ CHECK).to_le_bytes());
            records.extend_from_slice(&field);
            data.extend_from_slice(bytes);
        }
        // `end` is the package end offset: the guard footer sits immediately
        // before it, so it is the whole file, not the end of the payload.
        let end = data_start + data.len() + 280;
        let mut out = Vec::new();
        out.extend_from_slice(&0x4dca23efu32.to_le_bytes());
        out.extend_from_slice(&(end as u32).to_le_bytes());
        out.extend_from_slice(&0x56a089b7u32.to_le_bytes());
        out.extend_from_slice(&records);
        out.extend_from_slice(&data);
        // 280-byte guard header ending the package: entry-table offset at 8,
        // entry count at 272, version at 276.
        let mut guard = vec![0u8; 280];
        guard[0..4].copy_from_slice(&0xfdfdfeeeu32.to_le_bytes());
        guard[8..12].copy_from_slice(&((table as u32) ^ MASK).to_le_bytes());
        guard[12..16].copy_from_slice(&0u32.to_le_bytes());
        guard[268..272].copy_from_slice(&0xf00dbeefu32.to_le_bytes());
        guard[272..276].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        guard[276..280].copy_from_slice(&0x20002u32.to_le_bytes());
        out.extend_from_slice(&guard);
        out
    }
}
