//! Validate terrain layer references, compressed masks and source textures.
#[path = "../pck.rs"]
mod pck;
use flate2::read::ZlibDecoder;
use std::{
    env,
    fs::{self, File},
    io::Read,
    path::Path,
};
fn n(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4).ok_or("truncated")?.try_into().unwrap(),
    ))
}
fn read_names(b: &[u8], mut p: usize, count: usize) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for _ in 0..count {
        let len = n(b, p)? as usize;
        p += 4;
        if len > 4096 {
            return Err("string too long".into());
        }
        let bytes = b.get(p..p + len).ok_or("truncated string")?;
        p += len;
        let (v, _, bad) = encoding_rs::GBK.decode(bytes);
        if bad {
            return Err("invalid GBK".into());
        }
        out.push(v.replace('\\', "/"));
    }
    Ok(out)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 2 {
        return Err("Usage: material-check <client-root>".into());
    }
    let root = Path::new(&args[1]);
    let element = if root.join("element").is_dir() {
        root.join("element")
    } else {
        root.to_path_buf()
    };
    let map = element.join("maps/world");
    let trn = fs::read(map.join("world.trn2")).map_err(|e| e.to_string())?;
    let tex = read_names(&trn, n(&trn, 72)? as usize, n(&trn, 36)? as usize)?;
    let names = read_names(&trn, n(&trn, 80)? as usize + 4, n(&trn, 52)? as usize)?;
    let mut pck = File::open(element.join("textures.pck")).map_err(|e| e.to_string())?;
    let entries = pck::directory(&mut pck)?;
    let mut dimensions = std::collections::BTreeMap::new();
    let mut mask_sizes = std::collections::BTreeMap::new();
    let mut mask_hashes = std::collections::HashSet::new();
    let mut found = 0;
    let mut images = 0;
    for path in &tex {
        if let Some(entry) = entries.iter().find(|e| e.name.eq_ignore_ascii_case(path)) {
            found += 1;
            let data = pck::read_entry(&mut pck, entry)?;
            match image::load_from_memory_with_format(&data, image::ImageFormat::Dds) {
                Ok(img) => {
                    images += 1;
                    *dimensions
                        .entry((img.width(), img.height()))
                        .or_insert(0usize) += 1;
                }
                Err(e) => eprintln!("DDS decode failed {path}: {e}"),
            }
        } else {
            eprintln!("Missing texture: {path}");
        }
    }
    println!("Terrain source texture dimensions: {:?}", dimensions);
    let mut area_count = 0;
    let mut mask_count = 0;
    for name in &names {
        let b = fs::read(map.join(format!("{name}.t2mk"))).map_err(|e| e.to_string())?;
        if n(&b, 0)? != 0x544d4b42 || n(&b, 4)? != 2 {
            return Err(format!("{name}: unsupported mask format"));
        }
        let areas = n(&b, 8)? as usize;
        let layers = n(&b, 12)? as usize;
        if areas > 65536 || layers > 4096 {
            return Err("mask counts too large".into());
        };
        let layerstart = 20 + areas * 4;
        for ai in 0..areas {
            let p = n(&b, 20 + ai * 4)? as usize;
            let count = n(&b, p)? as usize;
            if count > 256 {
                return Err("area layers too many".into());
            }
            area_count += 1;
            for li in 0..count {
                let r = p + 4 + li * 12;
                let index = n(&b, r)? as usize;
                if index >= layers {
                    return Err("invalid layer index".into());
                }
                let lp = layerstart + index * 28;
                let texidx = n(&b, lp)? as usize;
                if texidx >= tex.len() {
                    return Err(format!("{name}: invalid texture index {texidx}"));
                }
                if li == 0 {
                    continue;
                }
                let mask_size = n(&b, lp + 8)? as usize;
                *mask_sizes.entry(mask_size).or_insert(0usize) += 1;
                if mask_size == 0 || mask_size > 8192 {
                    return Err("invalid mask dimensions".into());
                }
                let off = n(&b, r + 4)? as usize;
                let len = n(&b, r + 8)? as usize;
                let data = b.get(off..off + len).ok_or("mask out of range")?;
                let mut decoded = Vec::new();
                if n(&b, 16)? != 0 {
                    ZlibDecoder::new(data)
                        .take((mask_size * mask_size + 1) as u64)
                        .read_to_end(&mut decoded)
                        .map_err(|e| e.to_string())?;
                } else {
                    decoded.extend_from_slice(data)
                }
                if decoded.len() != mask_size * mask_size {
                    return Err(format!("{name} area {ai}: wrong mask size"));
                }
                {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    decoded.hash(&mut h);
                    mask_hashes.insert(h.finish());
                }
                mask_count += 1;
            }
        }
    }
    println!(
        "Terrain texture references: {} ({} present in textures.pck, {} DDS decoded)",
        tex.len(),
        found,
        images
    );
    println!(
        "Verified {} sub-terrains, {} mask areas, {} layer masks",
        names.len(),
        area_count,
        mask_count
    );
    if found != tex.len() || images != tex.len() {
        return Err("some terrain textures are missing or could not be decoded".into());
    }
    println!(
        "Mask dimensions: {:?}; distinct mask hashes: {}",
        mask_sizes,
        mask_hashes.len()
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
