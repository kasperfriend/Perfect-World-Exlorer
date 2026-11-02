//! Upload authored model DDS mips without regenerating levels or applying an
//! sRGB sampling conversion. Material multiplication still operates in encoded
//! color space. This is a texture stage, not a complete model material shader.
use crate::model_dds::{Dds, Format};
use wgpu::util::DeviceExt;
#[derive(Clone, Copy)]
pub enum UploadMode {
    Auto,
    Decoded,
}

fn decode_mip(
    bytes: &[u8],
    format: Format,
    mip: &crate::model_dds::Mip<'_>,
) -> Result<Vec<u8>, String> {
    // image's block decoder expects dimensions divisible by four. Decode the
    // complete stored blocks, then crop; tiny authored mips are not discarded.
    let width = (mip.width + 3) / 4 * 4;
    let height = (mip.height + 3) / 4 * 4;
    let mut single = bytes[..128].to_vec();
    for (offset, value) in [
        (12, height),
        (16, width),
        (20, mip.bytes.len() as u32),
        (28, 1),
    ] {
        single[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    single.extend_from_slice(mip.bytes);
    let mut image = image::load_from_memory_with_format(&single, image::ImageFormat::Dds)
        .map_err(|e| format!("model DDS mip decode: {e}"))?
        .to_rgba8();
    // Some image decoder versions expose DXT1 as RGB, losing its one-bit alpha.
    // Restore that authored BC1 mode even when the decoded image is RGB-only.
    if format == Format::Dxt1 {
        let blocks_x = width / 4;
        for (block, data) in mip.bytes.chunks_exact(8).enumerate() {
            let c0 = u16::from_le_bytes([data[0], data[1]]);
            let c1 = u16::from_le_bytes([data[2], data[3]]);
            if c0 > c1 {
                continue;
            }
            let selectors = u32::from_le_bytes(data[4..8].try_into().unwrap());
            let x = (block as u32 % blocks_x) * 4;
            let y = (block as u32 / blocks_x) * 4;
            for p in 0..16 {
                if (selectors >> (p * 2)) & 3 == 3 {
                    image.get_pixel_mut(x + p % 4, y + p / 4).0[3] = 0;
                }
            }
        }
    }
    Ok(
        image::imageops::crop_imm(&image, 0, 0, mip.width, mip.height)
            .to_image()
            .into_raw(),
    )
}

pub fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    mode: UploadMode,
) -> Result<wgpu::Texture, String> {
    let d = Dds::parse(bytes)?;
    let first = &d.mips[0];
    if first.width.max(first.height) > device.limits().max_texture_dimension_2d {
        return Err("model DDS exceeds device texture dimensions".into());
    }
    let native = matches!(mode, UploadMode::Auto)
        && device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        && first.width % 4 == 0
        && first.height % 4 == 0;
    let format = if native {
        match d.format {
            Format::Dxt1 => wgpu::TextureFormat::Bc1RgbaUnorm,
            Format::Dxt3 => wgpu::TextureFormat::Bc2RgbaUnorm,
            Format::Dxt5 => wgpu::TextureFormat::Bc3RgbaUnorm,
        }
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let decoded;
    let data = if native {
        &bytes[128..]
    } else {
        decoded = d
            .mips
            .iter()
            .map(|m| decode_mip(bytes, d.format, m))
            .collect::<Result<Vec<_>, _>>()?
            .concat();
        &decoded
    };
    Ok(device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("authored model DDS mip chain"),
            size: wgpu::Extent3d {
                width: first.width,
                height: first.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: d.mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::MipMajor,
        data,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiny_bc1_alpha_mip_keeps_its_authored_transparency() {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        for (o, v) in [
            (4, 124u32),
            (8, 0x81007),
            (12, 1),
            (16, 1),
            (20, 8),
            (76, 32),
            (80, 4),
            (108, 0x1000),
        ] {
            bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(b"DXT1");
        bytes.extend_from_slice(&[0, 0, 0, 248, 255, 255, 255, 255]);
        let d = Dds::parse(&bytes).unwrap();
        let rgba = decode_mip(&bytes, d.format, &d.mips[0]).unwrap();
        assert_eq!(rgba.len(), 4);
        assert_eq!(rgba[3], 0);
    }
}
