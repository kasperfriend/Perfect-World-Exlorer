//! Lossless authored terrain DDS uploads. No encoder or image resampler is
//! involved: the original BC blocks, including the tiny tail mips, reach wgpu.

/// Native array groups: BC1 256/512/1024, then BC2 256/512/1024.
/// Incomplete chains and non-square/other formats use the decoded fallback.
pub fn native_group(source: &[u8], enabled: bool) -> Option<usize> {
    if !enabled {
        return None;
    }
    let info = crate::terrain_sampling::parse_terrain_dds(source).ok()?;
    if info.width != info.height
        || ![256, 512, 1024].contains(&info.width)
        || info.authored_mips != info.width.ilog2() + 1
    {
        return None;
    }
    let format = match info.format {
        crate::terrain_sampling::DxtFormat::Dxt1 => 0,
        crate::terrain_sampling::DxtFormat::Dxt3 => 1,
        _ => return None,
    };
    Some(format * 3 + info.width.ilog2() as usize - 8)
}

pub fn upload(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    source: &[u8],
    index: u32,
) -> Result<(), String> {
    let info = crate::terrain_sampling::parse_terrain_dds(source)?;
    let mut offset = 128;
    for mip in 0..info.authored_mips {
        let width = (info.width >> mip).max(1);
        let height = (info.height >> mip).max(1);
        let blocks_x = width.div_ceil(4);
        let blocks_y = height.div_ceil(4);
        let row = blocks_x * info.format.block_bytes() as u32;
        let len = (row * blocks_y) as usize;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: mip,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: index,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &source[offset..offset + len],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(blocks_y),
            },
            wgpu::Extent3d {
                // BC copies use physical block extents even for 2x2/1x1 mips.
                width: width.div_ceil(4) * 4,
                height: height.div_ceil(4) * 4,
                depth_or_array_layers: 1,
            },
        );
        offset += len;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(side: u32, mips: u32, format: &[u8; 4]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        for (o, v) in [
            (4, 124u32),
            (12, side),
            (16, side),
            (28, mips),
            (76, 32),
            (80, 4),
        ] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[84..88].copy_from_slice(format);
        let block = if format == b"DXT1" { 8 } else { 16 };
        for mip in 0..mips {
            let blocks = (side >> mip).max(1).div_ceil(4) as usize;
            b.resize(b.len() + blocks * blocks * block, 0);
        }
        b
    }
    #[test]
    fn groups_preserve_native_resolution_format_and_complete_mips() {
        for (size, side) in [256u32, 512, 1024].into_iter().enumerate() {
            for (format, name) in [b"DXT1", b"DXT3"].into_iter().enumerate() {
                let b = source(side, side.ilog2() + 1, name);
                assert_eq!(native_group(&b, true), Some(format * 3 + size));
                assert_eq!(native_group(&b, false), None);
            }
        }
        assert_eq!(native_group(&source(256, 1, b"DXT1"), true), None);
        assert_eq!(native_group(&source(256, 9, b"DXT5"), true), None);
        assert_eq!(native_group(&[], true), None);
    }
}
