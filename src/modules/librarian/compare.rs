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

/// Some numeric fields are identifiers, not quantities with a better value.
pub fn best(name: &str, values: &[Option<f64>]) -> Option<f64> {
    if matches!(name, "Pet ID" | "Skill ID" | "Skill level" | "Dye count") {
        return None;
    }
    let smaller = matches!(
        name,
        "Strength"
            | "Dexterity"
            | "Magic"
            | "Vitality"
            | "Required level"
            | "Buy price"
            | "Repair price"
            | "Cooldown (ms)"
            | "Attack interval (s)"
    );
    if smaller {
        values.iter().flatten().copied().reduce(f64::min)
    } else {
        values.iter().flatten().copied().reduce(f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_comparison_has_no_rows() {
        assert!(rows(&[]).is_empty());
    }
    #[test]
    fn aligns_fields_across_equipment_types() {
        use super::super::database::{Category, Item};
        let make = |section, category, size| Item {
            id: section as u32,
            section,
            category,
            major: None,
            subtype: None,
            name: String::new(),
            icon: String::new(),
            model: String::new(),
            attack_interval: None,
            record: vec![0; size],
        };
        let weapon = make(3, Category::Weapons, 1412);
        let armor = make(6, Category::Armor, 1120);
        let rows = rows(&[&weapon, &armor]);
        assert_eq!(rows["Grade"], vec![Some(0.), Some(0.)]);
        assert_eq!(rows["Physical attack min"], vec![Some(0.), None]);
        assert_eq!(rows["Physical defense min"], vec![None, Some(0.)]);
    }
    #[test]
    fn best_direction_matches_the_attribute() {
        let values = [Some(10.), Some(20.), None];
        assert_eq!(best("Required level", &values), Some(10.));
        assert_eq!(best("Sell price", &values), Some(20.));
        assert_eq!(best("Pet ID", &values), None);
    }
}
