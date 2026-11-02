//! Read the supplied retail archive; decode and execute every text light
//! element without claiming general GFX graph/visual playback.
//! cargo run --bin gfx-light-runtime-survey -- /path/to/element/gfx.pck
#![allow(dead_code)]
#[path = "../gfx_color_track.rs"]
mod gfx_color_track;
#[path = "../gfx_light.rs"]
mod gfx_light;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene_lights.rs"]
mod scene_lights;
use std::{fs::File, path::Path};
fn run(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let entries = pck::directory(&mut file)?;
    let mut texts = 0;
    let mut files = 0;
    let mut elements = 0;
    let mut active_samples = 0;
    let identity = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    for entry in entries
        .iter()
        .filter(|e| e.name.to_ascii_lowercase().ends_with(".gfx"))
    {
        let bytes = pck::read_entry(&mut file, entry)?;
        if !bytes.starts_with(b"MOXT") {
            continue;
        }
        texts += 1;
        let (text, _, bad) = encoding_rs::GBK.decode(&bytes);
        if bad {
            return Err(format!("{}: invalid GBK", entry.name));
        }
        let has_light = text.lines().any(|l| {
            l.split_once(':')
                .is_some_and(|(k, v)| k.trim() == "GFXELEMENTID" && v.trim() == "130")
        });
        if !has_light {
            continue;
        }
        let definitions = gfx_light::parse(&text).map_err(|e| format!("{}: {e}", entry.name))?;
        files += 1;
        elements += definitions.len();
        for definition in definitions {
            let mut emitter = gfx_light::Emitter::new(definition);
            for dt in [0, 1, 17, 83, 299, 600, 1000, 2000, 5000, 60000] {
                if let Some(light) = emitter.tick(dt, true, 4, identity, None)? {
                    if light
                        .position
                        .iter()
                        .chain(&light.color)
                        .any(|x| !x.is_finite())
                    {
                        return Err(format!("{}: nonfinite runtime light", entry.name));
                    }
                    active_samples += 1;
                }
            }
        }
    }
    println!("Scanned {texts} text GFX files; decoded {files} light-bearing files / {elements} elements; {active_samples} active source-clock samples finite.");
    println!("Scope: point-light definitions/controllers only. Owner graph, attachments, placement/clock resolution and visual playback are not validated by this survey.");
    Ok(())
}
fn main() {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: gfx-light-runtime-survey GFX_PCK");
        std::process::exit(2);
    };
    if let Err(e) = run(Path::new(&path)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
