#![allow(dead_code)]
#[path = "../animation.rs"]
mod animation;
#[path = "../ecmodel.rs"]
mod ecmodel;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
};
fn main() -> Result<(), String> {
    let e = std::path::PathBuf::from(std::env::args().nth(1).ok_or("provide element folder")?);
    let s = scene::Scene::load(&e.join("maps/world/world.ecwld"))?;
    let mut f = File::open(e.join("models.pck")).map_err(|e| e.to_string())?;
    let entries: HashMap<_, _> = pck::directory(&mut f)?
        .into_iter()
        .map(|x| (x.name.to_lowercase(), x))
        .collect();
    let mut placed = HashSet::new();
    let mut ecms = HashSet::new();
    let mut smds = HashSet::new();
    let mut skins = HashSet::new();
    let mut missing = Vec::new();
    let mut versions = BTreeMap::new();
    for m in s.blocks.iter().flat_map(|b| &b.models) {
        if !placed.insert(m.export_id) {
            continue;
        }
        let key = ecmodel::normalize(&m.path);
        if !ecms.insert(key.clone()) {
            continue;
        }
        let entry = match entries.get(&key) {
            Some(e) => e,
            None => {
                missing.push(key);
                continue;
            }
        };
        let ec = ecmodel::parse_ecm(&pck::read_entry(&mut f, entry)?)?;
        let path = ec.skin_model_path;
        if !smds.insert(path.clone()) {
            continue;
        }
        let ent = match entries.get(&path) {
            Some(e) => e,
            None => {
                missing.push(path);
                continue;
            }
        };
        let smd = ecmodel::parse_smd(&pck::read_entry(&mut f, ent)?)?;
        *versions.entry(smd.version).or_insert(0usize) += 1;
        let parent = path.rsplit_once('/').map(|x| x.0).unwrap_or("");
        for refname in std::iter::once(smd.skeleton).chain(smd.skins) {
            if refname.is_empty() {
                continue;
            }
            let target = ecmodel::normalize(&format!("{parent}/{refname}"));
            if !entries.contains_key(&target) {
                missing.push(target.clone())
            }
            if target.ends_with(".ski") {
                skins.insert(target);
            }
        }
    }
    println!(
        "{} placements, {} ECMs, {} SMDs, {} unique SKIs; SMD versions {:?}",
        placed.len(),
        ecms.len(),
        smds.len(),
        skins.len(),
        versions
    );
    println!("{} unresolved dependencies", missing.len());
    for x in missing {
        println!("  {x}")
    }
    Ok(())
}
