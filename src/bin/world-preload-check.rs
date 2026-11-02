#![allow(dead_code)]
#[path = "../bmd.rs"]
mod bmd;
// The baked draw-state table is shared with the production scenery path.
#[path = "../bmd_baked.rs"]
mod bmd_baked;
#[path = "../objects.rs"]
mod objects;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
#[path = "../texture_atlas.rs"]
mod texture_atlas;
#[path = "../world_objects.rs"]
mod world_objects;
mod gpu {
    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    pub(crate) struct Vertex {
        pub(crate) pos: [f32; 3],
        pub(crate) normal: [f32; 3],
        pub(crate) uv: [f32; 2],
        pub(crate) color: [f32; 4],
        pub(crate) kind: f32,
        pub(crate) diffuse: u32,
        pub(crate) night_diffuse: u32,
    }
}
fn main() {
    use std::sync::{atomic::AtomicBool, mpsc, Arc};
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("element folder"));
    let dir = root.join("maps/world");
    let world = scene::Scene::load(&dir.join("world.ecwld")).unwrap();
    let ornaments = world.load_ornaments(&dir.join("world.ecbsd")).unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let count = ornaments.len();
    std::thread::spawn(move || {
        world_objects::build(root, ornaments, tx, Arc::new(AtomicBool::new(false)))
    });
    let (mut pages, mut verts, mut tris) = (0, 0, 0);
    let mut atlas_bytes = 0usize;
    for message in rx {
        match message {
            world_objects::Message::Page(p) => {
                pages += 1;
                verts += p.vertices.len();
                atlas_bytes += p.atlas.len();
                tris += p.indices.len() / 3;
                println!("Page {pages}: {} placements, {verts} vertices", p.processed)
            }
            world_objects::Message::Done { processed, skipped } => {
                println!("Done {processed}/{count}, {pages} pages, {verts} vertices, {tris} triangles, {skipped} missing-texture meshes; atlas base {:.1} MiB, with mips ~{:.1} MiB",atlas_bytes as f64/1048576.,atlas_bytes as f64/1048576.*(1.-1./65536.)/0.75);
                return;
            }
            world_objects::Message::Error(e) => panic!("{e}"),
        }
    }
    panic!("worker ended prematurely")
}
