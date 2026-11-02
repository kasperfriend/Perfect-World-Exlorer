#![allow(dead_code)]
#[path = "../music.rs"]
mod music;
#[path = "../pck.rs"]
mod pck;
use std::path::PathBuf;
fn main() {
    let e = PathBuf::from(std::env::args().nth(1).expect("element folder"));
    let db = music::Zones::open(&e.join("maps/world/precinct.clt")).unwrap();
    println!("{} zones", db.zones.len());
    for (x, z) in [
        (-755., -1352.),
        (2673., -4251.),
        (-2520., 4100.),
        (0., 0.),
        (4000., 4000.),
    ] {
        let zone = db.find(x, z).map(|i| &db.zones[i]);
        println!("{x:.0},{z:.0}: {:?}", zone.map(|x| (&x.name, &x.tracks)));
    }
    let mut p = music::MusicPlayer::new(&e);
    if std::env::var_os("PW_TEST_RAGING").is_some() {
        p.update(true, 0.5, 2673., -4251.);
        std::thread::sleep(std::time::Duration::from_secs(6));
        p.update(true, 0.5, 2673., -4251.);
        println!("Raging-first: {}", p.status());
        return;
    }
    p.update(true, 0.5, -755., -1352.);
    println!("Playback resolution: {}", p.status());
    #[cfg(windows)]
    {
        std::thread::sleep(std::time::Duration::from_millis(900));
        if std::env::var_os("PW_TEST_RAGING").is_some() {
            p.update(true, 0.5, 2673., -4251.);
            std::thread::sleep(std::time::Duration::from_secs(6));
            p.update(true, 0.5, 2673., -4251.);
            println!("Raging-first: {}", p.status());
            return;
        }
        p.update(true, 0.5, -755., -1352.);
        println!("First zone audio: {}", p.status());
        p.update(true, 0.3, -2520., 4100.);
        std::thread::sleep(std::time::Duration::from_millis(900));
        p.update(true, 0.3, -2520., 4100.);
        println!("Second zone audio: {}", p.status());
        std::thread::sleep(std::time::Duration::from_millis(600));
        p.update(true, 0.3, 2673., -4251.);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        p.update(true, 0.3, 2673., -4251.);
        println!("Raging Tides audio: {}", p.status());
        p.update(false, 0.3, 2673., -4251.);
        println!("Disabled: {}", p.status());
    }
}
