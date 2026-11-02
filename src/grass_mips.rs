//! Independently filtered grass atlas rectangles. Preparation is performed by
//! the background scene streamer; the render thread only uploads ready levels.
pub struct Tile {
    level: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
pub fn prepare(
    pixels: &[u8],
    slots: &[(crate::texture_atlas::Slot, u8)],
    side: usize,
) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity(slots.len() * 7);
    for &(slot, alpha_ref) in slots {
        let (mut w, mut h) = (slot.w, slot.h);
        let mut raw = vec![0; w * h * 4];
        for y in 0..h {
            let src = ((slot.y + y) * side + slot.x) * 4;
            raw[y * w * 4..(y + 1) * w * 4].copy_from_slice(&pixels[src..src + w * 4]);
        }
        let coverage = crate::foliage_mips::coverage_at(&raw, alpha_ref);
        for level in 1..8 {
            raw = crate::foliage_mips::downsample(&raw, w, h, true);
            w /= 2;
            h /= 2;
            tiles.push(Tile {
                level,
                x: (slot.x >> level) as u32,
                y: (slot.y >> level) as u32,
                width: w as u32,
                height: h as u32,
                pixels: crate::foliage_mips::preserve_coverage_at(&raw, coverage, alpha_ref),
            });
        }
    }
    tiles
}
pub fn upload(
    queue: &wgpu::Queue,
    atlas: &wgpu::Texture,
    pixels: &[u8],
    tiles: &[Tile],
    side: usize,
) {
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: atlas,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some((side * 4) as u32),
            rows_per_image: Some(side as u32),
        },
        wgpu::Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
    );
    for t in tiles {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: atlas,
                mip_level: t.level,
                origin: wgpu::Origin3d {
                    x: t.x,
                    y: t.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &t.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(t.width * 4),
                rows_per_image: Some(t.height),
            },
            wgpu::Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
        );
    }
}
