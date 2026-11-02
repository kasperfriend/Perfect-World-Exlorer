//! Extraction shared by viewer automatic setup and diagnostics.
use crate::{pck, scene};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
pub fn extract(root: &Path, out: &Path) -> Result<(), String> {
    let element = if root.join("element").is_dir() {
        root.join("element")
    } else {
        root.to_path_buf()
    };
    if out.exists() {
        return Err("output path already exists; choose a new directory".into());
    }
    let scene = scene::Scene::load(&element.join("maps/world/world.ecwld"))?;
    let mut pkg = File::open(element.join("trees.pck")).map_err(|e| e.to_string())?;
    let entries: HashMap<_, _> = pck::directory(&mut pkg)?
        .into_iter()
        .map(|e| (e.name.to_lowercase(), e))
        .collect();
    // Check all paths before creating files; never derive an output path from a client-provided name.
    for t in &scene.tree_types {
        for name in [&t.tree, &t.composite] {
            if !entries.contains_key(&name.to_lowercase()) {
                return Err(format!("tree source asset missing: {name}"));
            }
        }
    }
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut manifest =
        BufWriter::new(File::create(out.join("tree-types.tsv")).map_err(|e| e.to_string())?);
    writeln!(
        manifest,
        "type_id\tspt_file\tcomposite_file\tclient_spt_path\tclient_composite_path"
    )
    .map_err(|e| e.to_string())?;
    for (i, t) in scene.tree_types.iter().enumerate() {
        let spt = format!("tree-{i:03}.spt");
        let dds = format!("tree-{i:03}.dds");
        for (name, dst) in [(&t.tree, &spt), (&t.composite, &dds)] {
            let entry = &entries[&name.to_lowercase()];
            let data = pck::read_entry(&mut pkg, entry)?;
            fs::write(out.join(dst), data).map_err(|e| e.to_string())?;
        }
        writeln!(
            manifest,
            "{i}\t{spt}\t{dds}\t{}\t{}",
            escape(&t.tree),
            escape(&t.composite)
        )
        .map_err(|e| e.to_string())?;
    }
    let mut placements =
        BufWriter::new(File::create(out.join("tree-placements.tsv")).map_err(|e| e.to_string())?);
    writeln!(placements, "type_id\tx\ty\tz\tblock_row\tblock_col").map_err(|e| e.to_string())?;
    let mut count = 0;
    for block in &scene.blocks {
        for tree in &block.trees {
            writeln!(
                placements,
                "{}\t{}\t{}\t{}\t{}\t{}",
                tree.tree_type,
                tree.position[0],
                tree.position[1],
                tree.position[2],
                block.row,
                block.col
            )
            .map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    manifest.flush().map_err(|e| e.to_string())?;
    placements.flush().map_err(|e| e.to_string())?;
    println!(
        "Extracted {} SpeedTree sources, composite maps and {count} world placements",
        scene.tree_types.len()
    );
    Ok(())
}
