//! Comparison rows aligned by semantic field name, rather than record offsets.
use super::database::Item;
use std::collections::BTreeMap;

pub fn rows(items: &[&Item]) -> BTreeMap<&'static str, Vec<Option<f64>>> {
    let mut rows = BTreeMap::new();
    for (column, item) in items.iter().enumerate() {
        for (name, value) in item.attributes() {
            let values = rows.entry(name).or_insert_with(|| vec![None; items.len()]);
            values[column] = Some(value);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_comparison_has_no_rows() {
        assert!(rows(&[]).is_empty());
    }
}
