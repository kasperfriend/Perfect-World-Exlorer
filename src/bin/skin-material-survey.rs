//! Validate every skin in a private models.pck with the production parser.
//! No client data is embedded. A nonempty failure census returns a failing exit
//! status; successful parsing does not imply rendering/animation completeness.
#![allow(dead_code)]
#[path = "../model_material.rs"]
mod model_material;
#[path = "../pck.rs"]
mod pck;
#[path = "../skin.rs"]
mod skin;
use std::{collections::BTreeMap, fs::File};
fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: skin-material-survey models.pck")?;
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut entries = pck::directory(&mut f)?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut versions = BTreeMap::<u32, usize>::new();
    let (mut skins, mut rigid, mut skinned, mut materials, mut two_sided) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    for e in entries
        .iter()
        .filter(|e| e.name.to_lowercase().ends_with(".ski"))
    {
        let result = pck::read_entry(&mut f, e).and_then(|b| skin::Skin::parse(&b));
        match result {
            Ok(s) => {
                skins += 1;
                *versions.entry(s.version).or_default() += 1;
                for m in &s.meshes {
                    if m.rigid_bone.is_some() {
                        rigid += 1
                    } else {
                        skinned += 1
                    }
                }
                for m in &s.materials {
                    materials += 1;
                    two_sided += usize::from(m.two_sided);
                    // Verify the production parser-to-binding bridge for both variants.
                    for point_enabled in [false, true] {
                        let c = m.light_constants(model_material::Lighting {
                            environment_ambient: [0.2, 0.3, 0.4, 0.],
                            additional_ambient: [0.; 4],
                            directional_diffuse: [0.8, 0.7, 0.6, 1.],
                            point_ambient: [0.1, 0.2, 0.3, 0.],
                            point_diffuse: [0.4, 0.3, 0.2, 1.],
                            transparency: 0.,
                            point_enabled,
                        });
                        if !c
                            .ambient
                            .iter()
                            .chain(c.directional_diffuse.iter())
                            .chain(c.point_ambient.iter())
                            .chain(c.point_diffuse.iter())
                            .all(|v| v.is_finite())
                            || c.point_diffuse[3] != 0.
                        {
                            failures.push(format!("{}: invalid material binding", e.name));
                        }
                    }
                }
            }
            Err(err) => failures.push(format!("{}: {err}", e.name)),
        }
    }
    println!("{skins} parsed skins; versions {versions:?}; {skinned} skinned and {rigid} rigid meshes; {materials} materials ({two_sided} two-sided)");
    for failure in &failures {
        eprintln!("{failure}");
    }
    if skins == 0 {
        return Err("archive contains no successfully parsed skins".into());
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} unresolved skin/material records",
            failures.len()
        ));
    }
    println!("No unresolved skin/material records. World receiver/animation integration is a separate gate.");
    Ok(())
}
