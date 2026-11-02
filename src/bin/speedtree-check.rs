//! Verifies the actual client SpeedTree runtime exports and source format.
#[allow(dead_code)]
#[path = "../pck.rs"]
mod pck;
#[allow(dead_code)]
#[path = "../scene.rs"]
mod scene;
use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    path::Path,
};
const EXPORTS: &[(&str, &str)] = &[
    ("constructor", "??0CSpeedTreeRT@@QAE@XZ"),
    ("destructor", "??1CSpeedTreeRT@@QAE@XZ"),
    ("LoadTree memory", "?LoadTree@CSpeedTreeRT@@QAE_NPBEI@Z"),
    ("Compute", "?Compute@CSpeedTreeRT@@QAE_NPBMI_N@Z"),
    (
        "GetGeometry",
        "?GetGeometry@CSpeedTreeRT@@QAEXAAUSGeometry@1@KFFF@Z",
    ),
    (
        "GetTextures",
        "?GetTextures@CSpeedTreeRT@@QBEXAAUSTextures@1@@Z",
    ),
    ("SetCamera", "?SetCamera@CSpeedTreeRT@@SAXPBM0@Z"),
];
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 2 {
        return Err("usage: speedtree-check <client-root>".into());
    }
    let root = Path::new(&args[1]);
    let element = if root.join("element").is_dir() {
        root.join("element")
    } else {
        root.to_path_buf()
    };
    let dll = fs::read(element.join("speedtreert.dll")).map_err(|e| e.to_string())?;
    let pe = goblin::pe::PE::parse(&dll).map_err(|e| e.to_string())?;
    if pe.is_64 || pe.header.coff_header.machine != 0x14c {
        return Err("expected 32-bit x86 SpeedTree runtime".into());
    }
    println!("Runtime: 32-bit x86 PE, {} exports", pe.exports.len());
    let mut missing = false;
    for (label, name) in EXPORTS {
        let found = pe.exports.iter().any(|e| e.name == Some(*name));
        println!("{label}: {}", if found { "present" } else { "MISSING" });
        if !found {
            missing = true
        }
    }
    let scene = scene::Scene::load(&element.join("maps/world/world.ecwld"))?;
    let mut file = File::open(element.join("trees.pck")).map_err(|e| e.to_string())?;
    let entries: HashMap<_, _> = pck::directory(&mut file)?
        .into_iter()
        .map(|e| (e.name.to_lowercase(), e))
        .collect();
    let mut valid = 0;
    for ty in &scene.tree_types {
        let e = entries
            .get(&ty.tree.to_lowercase())
            .ok_or_else(|| format!("missing {}", ty.tree))?;
        let data = pck::read_entry(&mut file, e)?;
        if data.get(..20)
            == Some(&[
                0xe8, 0x03, 0x00, 0x00, 0x0c, 0, 0, 0, b'_', b'_', b'I', b'd', b'v', b'S', b'p',
                b't', b'_', b'0', b'2', b'_',
            ])
        {
            valid += 1;
        } else {
            eprintln!("unrecognized source {}", ty.tree)
        }
    }
    println!(
        "SPT 02 source signatures: {valid}/{}",
        scene.tree_types.len()
    );
    if missing || valid != scene.tree_types.len() {
        return Err("runtime or source mismatch".into());
    }
    println!("ABI audit only: no DLL code was executed and no geometry was generated.");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
