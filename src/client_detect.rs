//! Identify known clients from their actual PE executable, not a folder name.
use std::{
    fs,
    path::{Path, PathBuf},
};

const PW142_SIZE: usize = 6_000_640;
const PW142_SHA256: &str = "a290998dc13cc5558b54bda3f65d099f95bc0702e54a0613c6d7669f35c6cdb0";

#[derive(Debug)]
pub struct Client {
    pub element: PathBuf,
    pub known_142: bool,
}
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn detect(selected: &Path) -> Result<Client, String> {
    let element = if selected.join("element/maps/world/world.trn2").is_file() {
        selected.join("element")
    } else {
        selected.to_path_buf()
    };
    for path in [
        "maps/world/world.trn2",
        "maps/world/world.ecwld",
        "maps/world/world.ecbsd",
        "elementclient.exe",
        "trees.pck",
        "building.pck",
        "textures.pck",
        "speedtreert.dll",
    ] {
        if !element.join(path).is_file() {
            return Err(format!("The selected folder is missing {path}. Select the extracted client root or its element folder."));
        }
    }
    let bytes = fs::read(element.join("elementclient.exe"))
        .map_err(|e| format!("Cannot read elementclient.exe: {e}"))?;
    let pe = goblin::pe::PE::parse(&bytes)
        .map_err(|e| format!("elementclient.exe is not a valid Windows PE executable: {e}"))?;
    if pe.header.coff_header.machine != 0x14c || pe.is_64 {
        return Err("elementclient.exe is not a 32-bit x86 game client".into());
    }
    let known_142 = bytes.len() == PW142_SIZE && digest(&bytes) == PW142_SHA256;
    Ok(Client { element, known_142 })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_reference() {
        assert_eq!(
            digest(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
    #[test]
    fn rejects_missing() {
        assert!(detect(Path::new("__not_a_client__")).is_err());
    }
}
