//! Borrowed authored DDS block/mip data for model textures. Keep every authored
//! level; silently regenerating a new mip chain changes alpha and light edges.
//! This parser does not implement texture shaders, cube maps, DX10 or decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Dxt1,
    Dxt3,
    Dxt5,
}
#[derive(Debug)]
pub struct Mip<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
}
#[derive(Debug)]
pub struct Dds<'a> {
    pub format: Format,
    pub mips: Vec<Mip<'a>>,
}
impl<'a> Dds<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let header = bytes.get(..128).ok_or("truncated model DDS header")?;
        let word = |i| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
        if &header[..4] != b"DDS " || word(4) != 124 || word(76) != 32 {
            return Err("invalid model DDS header".into());
        }
        if word(80) & 4 == 0 {
            return Err("non-FourCC model DDS is not implemented".into());
        }
        if word(112) != 0 || word(24) > 1 {
            return Err("cube/volume model DDS is not implemented".into());
        }
        let (format, block_bytes) = match &header[84..88] {
            b"DXT1" => (Format::Dxt1, 8usize),
            b"DXT3" => (Format::Dxt3, 16),
            b"DXT5" => (Format::Dxt5, 16),
            tag => return Err(format!("unsupported model DDS format {tag:?}")),
        };
        let (width, height) = (word(16), word(12));
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err("model DDS dimensions out of bounds".into());
        }
        let count = word(28).max(1);
        let max_count = 32 - width.max(height).leading_zeros();
        if count > max_count {
            return Err("model DDS mip count exceeds dimensions".into());
        }
        let mut mips = Vec::new();
        let mut offset = 128usize;
        for i in 0..count {
            let w = (width >> i).max(1);
            let h = (height >> i).max(1);
            let size = ((w + 3) / 4) as usize * ((h + 3) / 4) as usize * block_bytes;
            let end = offset.checked_add(size).ok_or("model DDS size overflow")?;
            let data = bytes
                .get(offset..end)
                .ok_or_else(|| format!("truncated model DDS mip {i}"))?;
            mips.push(Mip {
                width: w,
                height: h,
                bytes: data,
            });
            offset = end;
        }
        if offset != bytes.len() {
            return Err("unrecognized model DDS trailing payload".into());
        }
        Ok(Self { format, mips })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(tag: &[u8; 4], unit: usize) -> Vec<u8> {
        let mut b = vec![0; 128];
        b[..4].copy_from_slice(b"DDS ");
        for (o, v) in [(4, 124u32), (12, 8), (16, 8), (28, 4), (76, 32), (80, 4)] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[84..88].copy_from_slice(tag);
        for (i, blocks) in [4, 1, 1, 1].into_iter().enumerate() {
            b.extend(vec![i as u8; blocks * unit]);
        }
        b
    }
    #[test]
    fn retains_all_authored_levels_and_sub_block_mips() {
        for (tag, format, unit) in [
            (b"DXT1", Format::Dxt1, 8),
            (b"DXT3", Format::Dxt3, 16),
            (b"DXT5", Format::Dxt5, 16),
        ] {
            let bytes = fixture(tag, unit);
            let d = Dds::parse(&bytes).unwrap();
            assert_eq!(d.format, format);
            assert_eq!(d.mips.len(), 4);
            for (i, m) in d.mips.iter().enumerate() {
                assert_eq!((m.width, m.height), (8 >> i, 8 >> i));
                assert!(m.bytes.iter().all(|b| *b == i as u8));
                assert_eq!(m.bytes.len(), if i == 0 { 4 * unit } else { unit });
            }
        }
    }
    #[test]
    fn rejects_truncated_extra_and_unsupported_data() {
        let bytes = fixture(b"DXT1", 8);
        for i in 0..bytes.len() {
            assert!(Dds::parse(&bytes[..i]).is_err(), "accepted length {i}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Dds::parse(&extra).is_err());
        for (offset, value) in [
            (4, 0u32),
            (12, 0),
            (16, 20000),
            (28, u32::MAX),
            (76, 0),
            (80, 0),
            (112, 0x200),
            (24, 8),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Dds::parse(&bad).is_err());
        }
        assert!(Dds::parse(&fixture(b"DX10", 8)).is_err());
    }
    #[test]
    fn absent_mip_count_means_one_level_not_a_guessed_full_chain() {
        let mut b = fixture(b"DXT1", 8);
        b[28..32].fill(0);
        b.truncate(128 + 32);
        assert_eq!(Dds::parse(&b).unwrap().mips.len(), 1);
    }
}
