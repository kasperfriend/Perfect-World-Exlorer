#![allow(dead_code)]
#[path = "../pck.rs"]
mod pck;
fn main() {
    let mut f = std::fs::File::open(
        std::path::PathBuf::from(std::env::args().nth(1).unwrap()).join("textures.pck"),
    )
    .unwrap();
    for e in pck::directory(&mut f).unwrap() {
        let n = e.name.to_lowercase();
        if [
            "textures/sky/clear/",
            "textures/sky/evening/",
            "textures/sky/night2/",
        ]
        .iter()
        .any(|p| n.starts_with(p))
            && ["01.bmp", "02.bmp", "roof.bmp"]
                .iter()
                .any(|p| n.ends_with(p))
        {
            let b = pck::read_entry(&mut f, &e).unwrap();
            let i = image::load_from_memory(&b).unwrap();
            println!("{} {}x{}", n, i.width(), i.height());
        }
    }
}
