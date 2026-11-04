//! Decode client audio with the same rodio backend as the Windows player.
//! Usage: audio-corpus-check <element directory>
#[cfg(windows)]
#[path = "../pck.rs"]
mod pck;

#[cfg(windows)]
fn main() {
    use rodio::Decoder;
    use std::{fs, io::Cursor, path::Path};

    fn audio(path: &str) -> bool {
        let name = path.to_ascii_lowercase();
        [".wav", ".ogg", ".mp3"]
            .iter()
            .any(|ext| name.ends_with(ext))
    }
    fn decode(bytes: Vec<u8>) -> Result<usize, String> {
        let decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let count = decoder.count();
        if count == 0 {
            Err("no decoded samples".into())
        } else {
            Ok(count)
        }
    }
    fn loose(dir: &Path, checked: &mut usize, failed: &mut usize) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("{}: {e}", dir.display());
                *failed += 1;
                return;
            }
        };
        for item in entries {
            let entry = match item {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("directory entry: {e}");
                    *failed += 1;
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    *failed += 1;
                    continue;
                }
            };
            if kind.is_dir() {
                loose(&path, checked, failed);
            } else if kind.is_file() && audio(&path.to_string_lossy()) {
                *checked += 1;
                if let Err(e) = fs::read(&path).map_err(|e| e.to_string()).and_then(decode) {
                    eprintln!("{}: {e}", path.display());
                    *failed += 1;
                }
            }
        }
    }
    let root = match std::env::args_os().nth(1) {
        Some(root) => std::path::PathBuf::from(root),
        None => {
            eprintln!("usage: audio-corpus-check <element directory>");
            std::process::exit(2);
        }
    };
    let mut checked = 0;
    let mut failed = 0;
    let archive = root.join("sfx.pck");
    match fs::File::open(&archive)
        .map_err(|e| e.to_string())
        .and_then(|mut file| {
            let entries = pck::directory(&mut file)?;
            for entry in entries.iter().filter(|e| audio(&e.name)) {
                checked += 1;
                if let Err(e) = pck::read_entry(&mut file, entry).and_then(decode) {
                    eprintln!("{}: {e}", entry.name);
                    failed += 1;
                }
            }
            Ok(())
        }) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{}: {e}", archive.display());
            failed += 1;
        }
    }
    loose(&root.join("music"), &mut checked, &mut failed);
    println!("Audio entries checked: {checked}; failures: {failed}");
    if failed != 0 || checked == 0 {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("audio-corpus-check requires Windows and the viewer's rodio backend");
    std::process::exit(1);
}
