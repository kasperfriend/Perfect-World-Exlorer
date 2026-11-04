//! Compact material residency for the native half-open terrain block window.
//! Array entries keep global area indices; only mask payloads are windowed.
/// One area of safety covers boundary vertices, rounding and filtering without
/// ever asking a cold area to borrow another area's layer records.
pub fn area_window(
    [x0, y0, x1, y1]: [usize; 4],
    block_cells: usize,
    mask_grid: usize,
    cols: usize,
    rows: usize,
) -> [usize; 4] {
    assert!(mask_grid > 0);
    [
        (x0 * block_cells / mask_grid).saturating_sub(1).min(cols),
        (y0 * block_cells / mask_grid).saturating_sub(1).min(rows),
        ((x1 * block_cells).div_ceil(mask_grid) + 1).min(cols),
        ((y1 * block_cells).div_ceil(mask_grid) + 1).min(rows),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn border_halo_is_clamped_to_world() {
        assert_eq!(area_window([0, 0, 15, 15], 32, 128, 32, 44), [0, 0, 5, 5]);
        assert_eq!(
            area_window([113, 161, 128, 176], 32, 128, 32, 44),
            [27, 39, 32, 44]
        );
    }
    #[test]
    fn every_resident_cell_has_its_original_area() {
        for x0 in 0..128 {
            let x1 = (x0 + 15).min(128);
            let [lo, _, hi, _] = area_window([x0, 4, x1, 19], 32, 128, 32, 44);
            for cell in x0 * 32..x1 * 32 {
                assert!((lo..hi).contains(&(cell / 128)));
            }
        }
    }
    #[test]
    fn native_window_does_not_expand_to_whole_world() {
        let [x0, y0, x1, y1] = area_window([48, 64, 63, 79], 32, 128, 32, 44);
        assert!((x1 - x0) * (y1 - y0) <= 49);
        assert!((x1 - x0) * (y1 - y0) < 32 * 44 / 20);
    }
}
