//! Read and validate the actual client scene-light records.
#![allow(dead_code)]
#[path = "../scene_lights.rs"]
mod scene_lights;
use std::{env, path::Path};
fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: scene-light-survey path/to/scenelights.dat");
    let lights =
        scene_lights::SceneLights::load(Path::new(&path)).expect("invalid scene-light file");
    let flagged = lights.lights.iter().filter(|l| l.flags & 1 != 0).count();
    let range = lights
        .lights
        .iter()
        .map(|l| l.range)
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(v), b.max(v))
        });
    println!(
        "{} scene lights; flag bit 0 set on {}; range {:.2}..{:.2}",
        lights.lights.len(),
        flagged,
        range.0,
        range.1
    );
    let mut grid = scene_lights::StaticGrid::default();
    let p = lights.lights[0].position;
    grid.update(&lights, p);
    let day = grid.query(p, 0.).expect("first light is in its own cell");
    let night = grid.query(p, 1.).unwrap();
    println!(
        "At first real light: selected {:?}, flags {:#x}, day {:?}, night {:?}",
        day.position, day.flags, day.diffuse, night.diffuse
    );
    assert_eq!(day.position, lights.lights[0].position);
    assert_eq!(night.stored_attenuation, [0., 0.4, 0.]);
    for l in lights.lights.iter().take(3) {
        println!(
            "position {:?}, range {}, diffuse {:?}, ambient {:?}, flags {:#x}",
            l.position, l.range, l.diffuse, l.ambient, l.flags
        );
    }
}
