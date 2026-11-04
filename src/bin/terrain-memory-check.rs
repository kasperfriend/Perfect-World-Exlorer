//! Asset-free RAM residency stress test of the production terrain page store.
#![allow(dead_code)]
#[path = "../terrain_store.rs"]
mod terrain_store;
fn height(x: usize, y: usize) -> f32 {
    ((x as i32 - 2048) * 11 + (y as i32 - 2816) * 3) as f32 / 17.
}
fn normal(x: usize, y: usize) -> [i8; 3] {
    [(x % 255) as i8, 127, (y % 255) as i8]
}
fn main() {
    let full = std::env::args().any(|a| a == "--full-world");
    let (width, height_count) = if full { (4096, 5632) } else { (512, 768) };
    let root = std::env::temp_dir().join(format!("pw-terrain-memory-check-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut builder = terrain_store::Builder::new(&root, width, height_count, 32).unwrap();
    for by in 0..height_count / 32 {
        for bx in 0..width / 32 {
            let mut heights = Vec::with_capacity(1024);
            let mut normals = Vec::with_capacity(1024);
            for y in 0..32 {
                for x in 0..32 {
                    heights.push(height(bx * 32 + x, by * 32 + y));
                    normals.push(normal(bx * 32 + x, by * 32 + y));
                }
            }
            builder.block(bx, by, &heights, &normals).unwrap();
        }
    }
    let budget = if full { 2 * 1024 * 1024 } else { 64 * 1024 };
    let (heights, normals) = builder.finish(budget).unwrap();
    let mut reader = heights.reader();
    // Sweep all pages, repeatedly crossing page edges and forcing evictions.
    // Check the exact GPU far-grid sample positions, then a revisit/teleport.
    for y in (0..height_count).step_by(4) {
        for x in (0..width).step_by(4) {
            let at = y * width + x;
            assert_eq!(reader.height(at).to_bits(), height(x, y).to_bits());
            assert_eq!(reader.normal(at), normal(x, y));
        }
    }
    for &(x, y) in &[
        (0, 0),
        (31, 31),
        (32, 32),
        (width - 1, height_count - 1),
        (width / 2, height_count / 2),
    ] {
        assert_eq!(
            reader.height(y * width + x).to_bits(),
            height(x, y).to_bits()
        );
    }
    let stats = heights.stats();
    assert!(stats.evictions > 0);
    assert!(stats.peak_bytes <= budget);
    println!("Lossless terrain: {}x{} source grids {:.2} MiB; decoded hot-set peak {:.3} MiB / {:.3} MiB budget; metadata {:.3} MiB; {} page reads, {} evictions; all source float/normal comparisons exact",
        width,height_count,(width*height_count*7) as f64/1048576.,stats.peak_bytes as f64/1048576.,budget as f64/1048576.,stats.metadata_bytes as f64/1048576.,stats.reads,stats.evictions);
    drop(reader);
    heights.trim();
    assert_eq!(heights.stats().resident_bytes, 0);
    drop((heights, normals));
    std::fs::remove_dir_all(root).unwrap();
}
