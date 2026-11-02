#![allow(dead_code)]
#[path = "../environment.rs"]
mod environment;
#[path = "../scene.rs"]
mod scene;
use std::{collections::BTreeMap, env, fs};
fn main() {
    let p: Vec<_> = env::args().collect();
    let s = scene::Scene::load(std::path::Path::new(&p[1])).unwrap();
    let bytes = fs::read(&p[2]).unwrap();
    let env =
        environment::Environments::load(&s, std::path::Path::new(&p[2]).parent().unwrap()).unwrap();
    let mut selected = 0;
    for x in (-4096..=4096).step_by(256) {
        for z in (-5600..=5600).step_by(256) {
            if environment::FogTransition::new(&env, [x as f32, 0., z as f32])
                .lighting(12.)
                .is_some()
            {
                selected += 1;
            }
        }
    }
    println!("region lit samples {selected}");
    let mut unique = BTreeMap::new();
    for b in &s.blocks {
        for r in &b.boxes {
            unique.insert(r.export_id, r.offset as usize);
        }
    }
    let mut hist = BTreeMap::new();
    for (id, off) in unique {
        let b = &bytes[off..off + 224];
        let ms = u32::from_le_bytes(b[0x9c..0xa0].try_into().unwrap());
        *hist.entry(ms).or_insert(0usize) += 1;
        if id < 20 {
            let floats = |off| f32::from_le_bytes(b[off..off + 4].try_into().unwrap());
            let color = |off| u32::from_le_bytes(b[off..off + 4].try_into().unwrap());
            println!(
                "id {id}, transition {ms}, sun {:?}, colors {:08x?}, scales {:?}",
                [floats(0x74), floats(0x78), floats(0x7c)],
                [color(0x80), color(0x84), color(0x88), color(0x8c)],
                [floats(0xb8), floats(0xbc)]
            );
        }
    }
    println!("transition histogram {hist:?}");
}
