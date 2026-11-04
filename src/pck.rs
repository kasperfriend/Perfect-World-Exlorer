//! Reader for the retail client's v2 PCK directory and entries.
//! Layout informed by GameForge's GPL-2.0-or-later Angelica importer.
use flate2::read::ZlibDecoder;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
const MASK: u32 = 0xa8937462;
const CHECK: u32 = 0x59374231;
fn u32le(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4).ok_or("truncated")?.try_into().unwrap(),
    ))
}
fn at(f: &mut File, p: u64, n: usize) -> Result<Vec<u8>, String> {
    let mut b = vec![0; n];
    f.seek(SeekFrom::Start(p))
        .and_then(|_| f.read_exact(&mut b))
        .map_err(|e| e.to_string())?;
    Ok(b)
}
fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(expected.min(1024 * 1024));
    ZlibDecoder::new(bytes)
        .take((expected + 1) as u64)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if out.len() != expected {
        return Err(format!(
            "bad decompressed size: {} != {expected}",
            out.len()
        ));
    }
    Ok(out)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub compressed: u32,
}
pub fn directory(f: &mut File) -> Result<Vec<Entry>, String> {
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    if size < 292 {
        return Err("archive too small".into());
    }
    let safe = at(f, 0, 12)?;
    if u32le(&safe, 0)? != 0x4dca23ef || u32le(&safe, 8)? != 0x56a089b7 {
        return Err("unsupported PCK safe header".into());
    }
    let end = u32le(&safe, 4)? as u64;
    if end > size || end < 292 {
        return Err("bad package end offset (split packages unsupported)".into());
    }
    let header = at(f, end - 280, 280)?;
    if u32le(&header, 0)? != 0xfdfdfeee || u32le(&header, 268)? != 0xf00dbeef {
        return Err("unsupported PCK guard keys".into());
    }
    let version = u32le(&header, 276)?;
    if version != 0x20002 && version != 0x20001 {
        return Err(format!("unsupported PCK version {version:08x}"));
    };
    if u32le(&header, 12)? & 0x80000000 != 0 {
        return Err("encrypted package unsupported".into());
    }
    let count = u32le(&header, 272)? as usize;
    if count > 1_000_000 {
        return Err("entry count exceeds limit".into());
    }
    let mut pos = (u32le(&header, 8)? ^ MASK) as u64;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 8 > end - 280 {
            return Err("entry table overlaps footer".into());
        }
        let h = at(f, pos, 8)?;
        let len = (u32le(&h, 0)? ^ MASK) as usize;
        if u32le(&h, 4)? ^ MASK ^ CHECK != len as u32
            || len == 0
            || len > 276
            || pos + 8 + len as u64 > end - 280
        {
            return Err("invalid entry table record".into());
        }
        let raw = at(f, pos + 8, len)?;
        let data = if len == 276 { raw } else { inflate(&raw, 276)? };
        let zero = data[..260].iter().position(|&x| x == 0).unwrap_or(260);
        let (decoded, _, _) = encoding_rs::GBK.decode(&data[..zero]);
        let name = decoded.replace('\\', "/");
        let offset = u32le(&data, 260)?;
        let usize_ = u32le(&data, 264)?;
        let compressed = u32le(&data, 268)?;
        if offset as u64 + compressed as u64 > end
            || usize_ > 512 * 1024 * 1024
            || compressed > usize_
        {
            return Err(format!("invalid entry bounds: {name}"));
        }
        result.push(Entry {
            name,
            offset,
            size: usize_,
            compressed,
        });
        pos += 8 + len as u64;
    }
    Ok(result)
}

pub fn read_entry(f: &mut File, e: &Entry) -> Result<Vec<u8>, String> {
    if e.compressed < e.size {
        // Decode directly from a bounded file reader. The old path retained a
        // complete compressed member alongside its expanded output.
        f.seek(SeekFrom::Start(e.offset as u64))
            .map_err(|err| err.to_string())?;
        let mut out = Vec::with_capacity((e.size as usize).min(1024 * 1024));
        ZlibDecoder::new(f.take(e.compressed as u64))
            .take(e.size as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|err| err.to_string())?;
        if out.len() != e.size as usize {
            return Err(format!(
                "bad decompressed size: {} != {}",
                out.len(),
                e.size
            ));
        }
        Ok(out)
    } else {
        at(f, e.offset as u64, e.compressed as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn bounded_streaming_decode_matches_the_original_member() {
        let expected = vec![37u8; 128 * 1024];
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&expected).unwrap();
        let packed = encoder.finish().unwrap();
        let path = std::env::temp_dir().join(format!(
            "pw-pck-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        f.write_all(&[0; 17]).unwrap();
        f.write_all(&packed).unwrap();
        f.write_all(&[99; 100]).unwrap(); // next member is not part of the stream
        let mut entry = Entry {
            name: "test".into(),
            offset: 17,
            size: expected.len() as u32,
            compressed: packed.len() as u32,
        };
        let decoded = read_entry(&mut f, &entry);
        entry.size -= 1;
        let invalid = read_entry(&mut f, &entry);
        drop(f);
        std::fs::remove_file(path).unwrap();
        assert_eq!(decoded.unwrap(), expected);
        assert!(invalid.is_err());
    }
}
