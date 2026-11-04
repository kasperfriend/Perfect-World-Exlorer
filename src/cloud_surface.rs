//! Bounds-checked reader for the native world cloud-surface resource (`MOXB`).
//!
//! The native `0x645150` cell parser consumes a count of 36-byte vertex
//! records, followed by a count of variable-length surface groups. Each group
//! has four floats, an index count, and that many 32-bit vertex indices. The
//! same 20-byte group header also explains the small records in empty cells.

#[derive(Clone, Debug, PartialEq)]
pub struct CloudVertex {
    pub position: [f32; 3],
    pub tags: [u32; 2],
    pub colors: [u32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudGroup {
    /// Four native group parameters; their precise render-state meaning is
    /// retained as data rather than assigned speculative names.
    pub parameters: [f32; 4],
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudCell {
    pub index: u32,
    pub vertices: Vec<CloudVertex>,
    pub groups: Vec<CloudGroup>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudSurface {
    pub width: u32,
    pub height: u32,
    /// Indexed by `row * width + column`; absent cells stay `None`.
    pub cells: Vec<Option<CloudCell>>,
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or("cloud offset overflow")?;
    let word: [u8; 4] = bytes
        .get(at..end)
        .ok_or_else(|| format!("truncated cloud dword at {at}"))?
        .try_into()
        .unwrap();
    Ok(u32::from_le_bytes(word))
}

fn f32_at(bytes: &[u8], at: usize) -> Result<f32, String> {
    let value = f32::from_bits(u32_at(bytes, at)?);
    if !value.is_finite() {
        return Err(format!("non-finite cloud float at {at}"));
    }
    Ok(value)
}

fn vertex_at(bytes: &[u8], at: usize) -> Result<CloudVertex, String> {
    let end = at.checked_add(36).ok_or("cloud vertex offset overflow")?;
    let src = bytes
        .get(at..end)
        .ok_or_else(|| format!("truncated cloud vertex at {at}"))?;
    Ok(CloudVertex {
        position: [f32_at(src, 0)?, f32_at(src, 4)?, f32_at(src, 8)?],
        tags: [u32_at(src, 12)?, u32_at(src, 16)?],
        colors: [
            u32_at(src, 20)?,
            u32_at(src, 24)?,
            u32_at(src, 28)?,
            u32_at(src, 32)?,
        ],
    })
}

fn parse_payload(index: u32, bytes: &[u8]) -> Result<CloudCell, String> {
    if bytes.len() < 8 {
        return Err(format!(
            "cloud cell {index} payload is shorter than two counts"
        ));
    }
    let vertex_count = u32_at(bytes, 0)? as usize;
    let vertex_bytes = vertex_count
        .checked_mul(36)
        .ok_or("cloud vertex count overflow")?;
    let groups_at = 4usize
        .checked_add(vertex_bytes)
        .ok_or("cloud vertex block overflow")?;
    let group_count = u32_at(bytes, groups_at)? as usize;
    let mut cursor = groups_at + 4;
    let mut vertices = Vec::with_capacity(vertex_count);
    for n in 0..vertex_count {
        vertices.push(vertex_at(bytes, 4 + n * 36)?);
    }

    // Each group needs at least its 16-byte parameter block and index count.
    if group_count > (bytes.len() - cursor) / 20 {
        return Err(format!("cloud cell {index} group directory is truncated"));
    }
    let mut groups = Vec::with_capacity(group_count);
    for group_index in 0..group_count {
        let parameters = [
            f32_at(bytes, cursor)?,
            f32_at(bytes, cursor + 4)?,
            f32_at(bytes, cursor + 8)?,
            f32_at(bytes, cursor + 12)?,
        ];
        let index_count = u32_at(bytes, cursor + 16)? as usize;
        cursor = cursor
            .checked_add(20)
            .ok_or("cloud group offset overflow")?;
        let index_bytes = index_count
            .checked_mul(4)
            .ok_or("cloud index count overflow")?;
        let index_end = cursor
            .checked_add(index_bytes)
            .ok_or("cloud index block overflow")?;
        let raw = bytes.get(cursor..index_end).ok_or_else(|| {
            format!("cloud cell {index} group {group_index} index list is truncated")
        })?;
        let mut indices = Vec::with_capacity(index_count);
        for word in raw.chunks_exact(4) {
            let value = u32::from_le_bytes(word.try_into().unwrap());
            if value as usize >= vertex_count {
                return Err(format!(
                    "cloud cell {index} group {group_index} index {value} exceeds {vertex_count} vertices"
                ));
            }
            indices.push(value);
        }
        cursor = index_end;
        groups.push(CloudGroup {
            parameters,
            indices,
        });
    }
    if cursor != bytes.len() {
        return Err(format!(
            "{} unparsed bytes after cloud cell {index} groups",
            bytes.len() - cursor
        ));
    }
    Ok(CloudCell {
        index,
        vertices,
        groups,
    })
}

/// Decode the native `MOXB` cell grid with checked lengths, unique cell IDs,
/// group records, and per-group vertex indices.
pub fn parse(bytes: &[u8]) -> Result<CloudSurface, String> {
    if bytes.get(..4) != Some(&b"MOXB"[..]) {
        return Err("invalid cloud surface signature (expected MOXB)".into());
    }
    if bytes.len() < 16 {
        return Err("truncated MOXB cloud surface header".into());
    }
    let width = u32_at(bytes, 4)?;
    let height = u32_at(bytes, 8)?;
    let record_count = u32_at(bytes, 12)? as usize;
    let cell_count = (width as usize)
        .checked_mul(height as usize)
        .filter(|&n| n > 0 && n <= 1_000_000)
        .ok_or("invalid MOXB grid dimensions")?;
    if record_count > cell_count {
        return Err("MOXB record count exceeds its grid".into());
    }
    let mut cells: Vec<Option<CloudCell>> = vec![None; cell_count];
    let mut cursor = 16usize;
    for _ in 0..record_count {
        let index = u32_at(bytes, cursor)?;
        let size = u32_at(bytes, cursor + 4)? as usize;
        cursor = cursor
            .checked_add(8)
            .ok_or("cloud record offset overflow")?;
        if index as usize >= cell_count {
            return Err(format!(
                "cloud cell index {index} exceeds grid size {cell_count}"
            ));
        }
        if cells[index as usize].is_some() {
            return Err(format!("duplicate cloud cell index {index}"));
        }
        let end = cursor
            .checked_add(size)
            .ok_or("cloud payload length overflow")?;
        let payload = bytes
            .get(cursor..end)
            .ok_or_else(|| format!("cloud cell {index} payload is truncated"))?;
        cells[index as usize] = Some(parse_payload(index, payload)?);
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(format!(
            "{} trailing bytes after MOXB records",
            bytes.len() - cursor
        ));
    }
    Ok(CloudSurface {
        width,
        height,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn put(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn vertex(bytes: &mut Vec<u8>, x: f32) {
        for value in [x, 0.0, 2.0] {
            put(bytes, value.to_bits());
        }
        put(bytes, 3);
        put(bytes, 4);
        for color in [0xff00_0000, 0xff00_0000, 0xff80_8080, 0xff80_8080] {
            put(bytes, color);
        }
    }

    #[test]
    fn parses_native_grid_vertices_and_variable_index_groups() {
        let mut bytes = b"MOXB".to_vec();
        put(&mut bytes, 1); // width
        put(&mut bytes, 1); // height
        put(&mut bytes, 1); // cell records
        put(&mut bytes, 0); // cell index
        put(&mut bytes, 4 + 36 + 4 + 20 + 4); // payload size
        put(&mut bytes, 1); // vertex count
        vertex(&mut bytes, 1.0);
        put(&mut bytes, 1); // group count
        for value in [10.0f32, 20.0, 30.0, 50.0] {
            put(&mut bytes, value.to_bits());
        }
        put(&mut bytes, 1); // index count
        put(&mut bytes, 0); // vertex index
        let cloud = parse(&bytes).unwrap();
        assert_eq!((cloud.width, cloud.height), (1, 1));
        let cell = cloud.cells[0].as_ref().unwrap();
        assert_eq!(cell.vertices.len(), 1);
        assert_eq!(cell.vertices[0].position, [1.0, 0.0, 2.0]);
        assert_eq!(cell.groups.len(), 1);
        assert_eq!(cell.groups[0].parameters, [10.0, 20.0, 30.0, 50.0]);
        assert_eq!(cell.groups[0].indices, [0]);
    }

    #[test]
    fn parses_zero_vertex_group_records_and_rejects_bad_indices() {
        let mut empty = b"MOXB".to_vec();
        put(&mut empty, 1);
        put(&mut empty, 1);
        put(&mut empty, 1);
        put(&mut empty, 0);
        put(&mut empty, 28);
        put(&mut empty, 0); // vertex count
        put(&mut empty, 1); // group count
        for value in [0.0f32, 1.0, 2.0, 3.0] {
            put(&mut empty, value.to_bits());
        }
        put(&mut empty, 0); // no indices
        let cell = parse(&empty).unwrap().cells[0].as_ref().unwrap().clone();
        assert!(cell.vertices.is_empty());
        assert_eq!(cell.groups.len(), 1);
        assert!(cell.groups[0].indices.is_empty());

        let mut bad = b"MOXB".to_vec();
        put(&mut bad, 1);
        put(&mut bad, 1);
        put(&mut bad, 1);
        put(&mut bad, 0);
        put(&mut bad, 4 + 36 + 4 + 20 + 4);
        put(&mut bad, 1);
        vertex(&mut bad, 0.0);
        put(&mut bad, 1);
        for value in [0.0f32; 4] {
            put(&mut bad, value.to_bits());
        }
        put(&mut bad, 1);
        put(&mut bad, 1); // invalid for a one-vertex array
        assert!(parse(&bad).is_err());
    }
}
