//! Audit the actual world's transitive GFX light dependencies without rendering.
//! cargo run --bin gfx-light-audit -- /path/to/element
#![allow(dead_code)]
#[path = "../animation.rs"]
mod animation;
#[path = "../ecmodel.rs"]
mod ecmodel;
#[path = "../gfx_light_audit.rs"]
mod gfx_light_audit;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
};
struct Pack {
    file: File,
    entries: BTreeMap<String, pck::Entry>,
}
impl Pack {
    fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut entries = BTreeMap::new();
        for e in pck::directory(&mut file)? {
            let key = ecmodel::normalize(&e.name);
            if entries.insert(key.clone(), e).is_some() {
                return Err(format!("duplicate normalized archive name: {key}"));
            }
        }
        Ok(Self { file, entries })
    }
    fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("missing dependency {name}"))?;
        pck::read_entry(&mut self.file, e)
    }
}
fn gfx_path(path: &str) -> String {
    let p = ecmodel::normalize(path);
    if p.starts_with("gfx/") {
        p
    } else {
        format!("gfx/{p}")
    }
}
fn run(element: &Path) -> Result<(), String> {
    let scene = scene::Scene::load(&element.join("maps/world/world.ecwld"))?;
    let mut roots = BTreeSet::new();
    let mut placements = 0;
    let mut events = 0;
    for e in scene.blocks.iter().flat_map(|b| &b.effects) {
        roots.insert(gfx_path(&e.path));
        placements += 1;
    }
    let models: BTreeSet<_> = scene
        .blocks
        .iter()
        .flat_map(|b| &b.models)
        .map(|m| ecmodel::normalize(&m.path))
        .collect();
    let mut model_pack = Pack::open(&element.join("models.pck"))?;
    for path in models {
        let model = ecmodel::parse_ecm(&model_pack.read(&path)?)?;
        for event in model.events {
            if let Some(path) = event.gfx_path() {
                roots.insert(gfx_path(&path));
                events += 1;
            }
        }
    }
    let mut gfx_pack = Pack::open(&element.join("gfx.pck"))?;
    let mut pending: Vec<_> = roots.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut lights = Vec::new();
    let mut edges = 0;
    while let Some(path) = pending.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = gfx_pack.read(&path)?;
        let (text, _, bad) = encoding_rs::GBK.decode(&bytes);
        if bad {
            return Err(format!("invalid GBK GFX: {path}"));
        }
        let audit = gfx_light_audit::parse(&text).map_err(|e| format!("{path}: {e}"))?;
        if audit.lights != 0 {
            lights.push((path.clone(), audit.lights));
        }
        for child in audit.children {
            pending.push(gfx_path(&child));
            edges += 1;
        }
    }
    println!("Scene effects: {placements}; model event references: {events}; unique roots: {}; transitive GFX: {}; nested edges: {edges}; missing/invalid: 0; light-bearing files: {}.",roots.len(),seen.len(),lights.len());
    for (path, n) in &lights {
        println!("GFX lights: {n} in {path}");
    }
    if !lights.is_empty() {
        return Err("runtime GFX light playback requires separate implementation/validation; this is a presence audit, not lifecycle support".into());
    }
    println!("No GFX light elements in this world's resolved dependency graph. This does not prove runtime-light support for other assets/maps.");
    Ok(())
}
fn main() {
    let Some(root) = std::env::args_os().nth(1) else {
        eprintln!("usage: gfx-light-audit ELEMENT_DIRECTORY");
        std::process::exit(2);
    };
    if let Err(e) = run(Path::new(&root)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
