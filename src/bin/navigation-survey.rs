#![allow(dead_code)]
#[path = "../navigation.rs"]
mod navigation;
#[path = "../pck.rs"]
mod pck;
fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("element folder"));
    let d = navigation::destinations(&root.join("maps/world/world_targets.txt")).unwrap();
    println!(
        "{} main-world targets, {} distinct names",
        d.len(),
        d.iter()
            .map(|p| &p.name)
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    let m = navigation::MapSpace {
        width: 4096,
        height: 5632,
        spacing: 2.,
        heights: std::sync::Arc::from(vec![0.; 4096 * 5632]),
    };
    for name in [
        "City of a Thousand Streams",
        "Archosaur, North District",
        "City of Raging Tides",
    ] {
        let p = d.iter().find(|p| p.name == name).unwrap();
        let [u, v] = m.to_uv(p.position[0], p.position[2]);
        println!(
            "{name} {:?} -> tile row {} col {}, local {:.3},{:.3}",
            p.position,
            (v * 11.) as u32,
            (u * 8.) as u32,
            (u * 8.).fract(),
            (v * 11.).fract()
        );
    }
    let image = navigation::original_map(&root).unwrap();
    println!(
        "Original map atlas {}x{} from 88 client DDS tiles",
        image.width(),
        image.height()
    );
}
