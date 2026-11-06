//! One `DrawIndexedIndirectArgs` record per batch, drawn as a single call.
//!
//! Far trees and grass both keep one vertex buffer, one index buffer and one
//! instance buffer for everything they draw and describe their batches as an
//! (index range, instance range) pair. That is exactly one indirect record per
//! batch: `first_index`/`index_count` pick the geometry and `first_instance`
//! picks the slice of the instance buffer, so the buffers are bound once and
//! `multi_draw_indexed_indirect` walks the whole list in one call.
//!
//! Both pieces of that are optional device features
//! (`MULTI_DRAW_INDIRECT` for the call, `INDIRECT_FIRST_INSTANCE` for a
//! non-zero `first_instance` inside an indirect record). Callers keep a
//! fallback — one draw per batch — for adapters that do not offer them.
use bytemuck::{Pod, Zeroable};

/// `DrawIndexedIndirectArgs`. The layout is fixed by Vulkan and WebGPU: five
/// 32-bit words, no padding, `instance_count` at offset 4.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct DrawIndexedArgs {
    pub(crate) index_count: u32,
    pub(crate) instance_count: u32,
    pub(crate) first_index: u32,
    pub(crate) base_vertex: i32,
    pub(crate) first_instance: u32,
}

/// Bytes of one record; the stride every indirect draw uses.
pub(crate) const DRAW_ARGS: u64 = std::mem::size_of::<DrawIndexedArgs>() as u64;

/// Can this device walk a whole record list in one call?
pub(crate) fn multi_draw(device: &wgpu::Device) -> bool {
    device
        .features()
        .contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE)
}

/// One record for a batch: its index range plus the slice of the instance
/// buffer it draws, as its base instance.
pub(crate) fn record(
    indices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
) -> DrawIndexedArgs {
    DrawIndexedArgs {
        index_count: indices.end - indices.start,
        instance_count: instances.end - instances.start,
        first_index: indices.start,
        base_vertex: 0,
        first_instance: instances.start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_the_layout_the_api_expects() {
        assert_eq!(DRAW_ARGS, 20);
        let record = record(7..13, 40..45);
        assert_eq!(record.index_count, 6);
        assert_eq!(record.instance_count, 5);
        assert_eq!(record.first_index, 7);
        assert_eq!(record.base_vertex, 0);
        assert_eq!(record.first_instance, 40);
        let bytes = bytemuck::bytes_of(&record);
        assert_eq!(bytes.len(), 20);
        // `instance_count` is the second word; the compute pass bumps it there.
        assert_eq!(u32::from_ne_bytes(bytes[4..8].try_into().unwrap()), 5);
    }

    #[test]
    fn an_empty_batch_still_produces_a_valid_record() {
        let record = record(0..0, 0..0);
        assert_eq!(record.index_count, 0);
        assert_eq!(record.instance_count, 0);
        assert_eq!(record.first_instance, 0);
    }
}
