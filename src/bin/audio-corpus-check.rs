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

#[cfg(all(test, windows))]
mod tests {
    use rodio::{Decoder, Source};
    use std::io::Cursor;

    fn wave(format: u16, rate: u32, channels: u16) -> Vec<u8> {
        let bits = if format == 3 { 32u16 } else { 16u16 };
        let samples = 64 * channels as u32;
        let data_len = samples * u32::from(bits / 8);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&format.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        let align = channels * bits / 8;
        bytes.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        bytes.extend_from_slice(&align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..samples {
            if format == 3 {
                bytes.extend_from_slice(&(if i % 2 == 0 { 0.5f32 } else { -0.5 }).to_le_bytes());
            } else {
                bytes
                    .extend_from_slice(&(if i % 2 == 0 { 12000i16 } else { -12000 }).to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn decodes_pcm_and_float_wav_specs_seen_in_client() {
        for (format, rate, channels) in [
            (1, 22050, 1),
            (1, 44100, 1),
            (1, 48000, 1),
            (1, 22050, 2),
            (3, 22050, 1),
        ] {
            let decoder = Decoder::new(Cursor::new(wave(format, rate, channels))).unwrap();
            assert_eq!(decoder.sample_rate(), rate);
            assert_eq!(decoder.channels(), channels);
            let decoded: Vec<_> = decoder.collect();
            assert_eq!(decoded.len(), 64 * channels as usize);
            assert!(decoded.iter().any(|&v| v > 0));
            assert!(decoded.iter().any(|&v| v < 0));
        }
    }

    #[test]
    fn decodes_generated_mp3_and_vorbis_fixtures() {
        for bytes in [
            &include_bytes!("../../tools/fixtures/audio/tone.mp3")[..],
            &include_bytes!("../../tools/fixtures/audio/tone.ogg")[..],
        ] {
            let decoder = Decoder::new(Cursor::new(bytes.to_vec())).unwrap();
            assert_eq!(decoder.sample_rate(), 22050);
            assert_eq!(decoder.channels(), 1);
            let samples: Vec<_> = decoder.collect();
            assert!(!samples.is_empty());
            assert!(samples.iter().any(|&v| v != 0));
        }
    }
}
