//! Audit ECWLD sound placements against sfx.pck; no audio device required.
#[allow(dead_code)]
#[path = "../pck.rs"]
mod pck;
#[allow(dead_code)]
#[path = "../scene.rs"]
mod scene;

use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    path::Path,
};

fn run() -> Result<(), String> {
    let arg = std::env::args_os()
        .nth(1)
        .ok_or("usage: scene-sound-check <element directory>")?;
    let root = Path::new(&arg);
    let scene = scene::Scene::load(&root.join("maps/world/world.ecwld"))?;
    let mut archive = File::open(root.join("sfx.pck")).map_err(|e| e.to_string())?;
    let paths: HashSet<_> = pck::directory(&mut archive)?
        .into_iter()
        .map(|e| e.name.to_lowercase())
        .collect();
    let mut total = 0usize;
    let mut missing = BTreeMap::<String, usize>::new();
    let mut distinct = HashSet::new();
    let mut modes = BTreeMap::<i32, usize>::new();
    let mut invalid_distances = 0;
    let mut reserved_nonzero = 0;
    for sound in scene.blocks.iter().flat_map(|b| &b.sounds) {
        total += 1;
        let path = format!("sfx/{}", sound.path.replace('\\', "/").to_lowercase());
        distinct.insert(path.clone());
        *modes.entry(sound.mode).or_default() += 1;
        if sound.min_distance < 0. || sound.max_distance < sound.min_distance {
            invalid_distances += 1;
        }
        if sound.reserved != 0 {
            reserved_nonzero += 1;
        }
        if !paths.contains(&path) {
            *missing.entry(path).or_default() += 1;
        }
    }
    println!(
        "Sound placements: {total}; distinct paths: {}; modes: {modes:?}",
        distinct.len()
    );
    println!("Invalid distances: {invalid_distances}; nonzero reserved words: {reserved_nonzero}");
    println!(
        "Missing paths: {}; affected placements: {}",
        missing.len(),
        missing.values().sum::<usize>()
    );
    for (path, count) in &missing {
        eprintln!("Missing {path} ({count} placements)");
    }
    if !missing.is_empty() || invalid_distances != 0 {
        return Err("sound census failed".into());
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
