//! Initial v27 item index. Record interpretation remains deliberately separate
//! from the archive layer so the database can be tested with synthetic files.
use super::elements::visit_v27_records;
use encoding_rs::GBK;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    Weapons,
    Armor,
    Ornaments,
    Fashion,
    Flight,
    PetsAndMounts,
    Genies,
    Consumables,
    GemsBooksAmmo,
    Materials,
    QuestAndPacks,
}

#[derive(Debug)]
pub struct Item {
    pub id: u32,
    pub section: usize,
    pub category: Category,
    pub major: Option<u32>,
    pub subtype: Option<u32>,
    pub name: String,
    pub icon: String,
    pub model: String,
    /// Raw v27 fields are retained for subsequent typed interpretation.
    pub record: Vec<u8>,
}

#[derive(Default)]
pub struct Database {
    pub items: Vec<Item>,
    pub by_id: HashMap<u32, usize>,
    pub category_names: HashMap<(usize, u32), String>,
    pub addons: HashMap<u32, Vec<u8>>,
    pub suites: HashMap<u32, Vec<u8>>,
    pub pets: HashMap<u32, Vec<u8>>,
}

fn number(record: &[u8], offset: usize) -> u32 {
    record.get(offset..offset + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}
fn wide(record: &[u8], offset: usize) -> String {
    let Some(bytes) = record.get(offset..offset + 64) else { return String::new() };
    let units: Vec<_> = bytes.chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|c| *c != 0).collect();
    String::from_utf16_lossy(&units)
}
fn path(record: &[u8], offset: usize) -> String {
    let Some(bytes) = record.get(offset..offset + 128) else { return String::new() };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    crate::ecmodel::normalize(&GBK.decode(&bytes[..end]).0)
}

// (category, major offset, subtype offset, name offset, icon offset, model offset)
fn layout(section: usize) -> Option<(Category, Option<usize>, Option<usize>, usize, usize, Option<usize>)> {
    use Category::*;
    let item = match section {
        3 => (Weapons, Some(4), Some(8), 12, 464, Some(80)),
        6 => (Armor, Some(4), Some(8), 12, 236, None),
        9 => (Ornaments, Some(4), Some(8), 12, 332, Some(76)),
        84 => (Fashion, None, Some(4), 12, 236, None),
        12 => (Consumables, Some(4), Some(8), 12, 204, None),
        15 => (Materials, Some(4), Some(8), 12, 204, None),
        87 | 90 => (Consumables, Some(4), Some(8), 12, 204, None),
        17 | 19 | 22 | 34 | 36 => (GemsBooksAmmo, None, Some(4), 8, 200, None),
        32 => (GemsBooksAmmo, None, Some(4), 8, 328, Some(72)),
        123 => (Genies, None, Some(4), 8, 200, None),
        23 | 24 => (Flight, None, None, 4, 324, Some(68)),
        29 => (QuestAndPacks, None, None, 4, 68, None),
        30 => (Materials, None, None, 4, 324, Some(68)),
        75 | 76 | 110 | 126 | 127 => (QuestAndPacks, None, None, 4, 196, None),
        96 | 97 | 98 => (PetsAndMounts, None, None, 4, 196, None),
        121 => (Genies, None, None, 4, 708, Some(68)),
        124 => (Genies, None, None, 4, 196, None),
        125 => (QuestAndPacks, None, None, 4, 200, None),
        25 | 27 | 28 | 99 | 100 | 108 | 109 | 114 | 115 | 116 | 117 | 118 | 119 | 120 =>
            (Consumables, None, None, 4, 196, None),
        _ => return None,
    };
    Some(item)
}

impl Database {
    pub fn parse_v27(bytes: &[u8]) -> Result<Self, String> {
        let mut db = Self::default();
        visit_v27_records(bytes, |section, record| {
            let id = number(record, 0);
            match section {
                0 => { db.addons.insert(id, record.to_vec()); }
                91 => { db.suites.insert(id, record.to_vec()); }
                95 => { db.pets.insert(id, record.to_vec()); }
                1 | 2 | 4 | 5 | 7 | 8 | 10 | 11 | 13 | 14 | 21 | 31 | 35 | 82 | 83 | 94 | 122 => {
                    db.category_names.insert((section, id), wide(record, 4));
                }
                _ => {}
            }
            if let Some((category, major, subtype, name, icon, model)) = layout(section) {
                let item = Item {
                    id, section, category,
                    major: major.map(|offset| number(record, offset)),
                    subtype: subtype.map(|offset| number(record, offset)),
                    name: wide(record, name), icon: path(record, icon),
                    model: model.map(|offset| path(record, offset)).unwrap_or_default(),
                    record: record.to_vec(),
                };
                db.by_id.entry(id).or_insert(db.items.len());
                db.items.push(item);
            }
            Ok(())
        })?;
        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_and_truncated_files() {
        assert!(Database::parse_v27(&[]).is_err());
        assert!(Database::parse_v27(&0x3000_001au32.to_le_bytes()).is_err());
        let mut data = 0x3000_001bu32.to_le_bytes().to_vec();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(Database::parse_v27(&data).is_err());
    }
    #[test]
    fn known_layouts_use_correct_name_and_icon_offsets() {
        let (category, _, _, name, icon, _) = layout(3).unwrap();
        assert_eq!(category, Category::Weapons);
        assert_eq!((name, icon), (12, 464));
        assert!(layout(1).is_none());
    }
}

impl Category {
    pub const ALL: [Self; 11] = [
        Self::Weapons, Self::Armor, Self::Ornaments, Self::Fashion, Self::Flight,
        Self::PetsAndMounts, Self::Genies, Self::Consumables, Self::GemsBooksAmmo,
        Self::Materials, Self::QuestAndPacks,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Weapons => "Weapons", Self::Armor => "Armor", Self::Ornaments => "Ornaments",
            Self::Fashion => "Fashion", Self::Flight => "Flight", Self::PetsAndMounts => "Pets & Mounts",
            Self::Genies => "Genies", Self::Consumables => "Consumables",
            Self::GemsBooksAmmo => "Gems, Books & Ammo", Self::Materials => "Materials",
            Self::QuestAndPacks => "Quest & Packs",
        }
    }
}

impl Item {
    /// Only fields whose interpretation is known are exposed. Unknown fields
    /// are deliberately not guessed from their raw binary representation.
    pub fn attributes(&self) -> Vec<(&'static str, f64)> {
        let fields: &[(&str, usize)] = match self.section {
            3 => &[("Strength",592),("Dexterity",596),("Magic",600),("Vitality",604),
                ("Required level",612),("Grade",616),("Physical attack min",624),
                ("Physical attack max",628),("Magic attack min",636),("Magic attack max",640),
                ("Durability min",656),("Durability max",660),("Price",672),("Shop price",676),("Reputation",680)],
            6 => &[("Grade",368),("Strength",372),("Dexterity",376),("Magic",380),("Vitality",384),
                ("Required level",392),("Physical defense min",400),("Physical defense max",404),
                ("Durability min",472),("Durability max",476),("Price",488),("Shop price",492),("Reputation",496)],
            9 => &[("Grade",460),("Strength",464),("Dexterity",468),("Magic",472),("Vitality",476),
                ("Required level",484),("Physical attack min",492),("Physical attack max",496),
                ("Magic attack min",500),("Magic attack max",504),("Physical defense min",508),
                ("Physical defense max",512),("Price",580),("Shop price",584),("Reputation",588)],
            84 => &[("Grade",368),("Price",380),("Shop price",384),("Stack limit",388)],
            12 => &[("Required level",332),("Cooldown (ms)",336),("HP recovery",340),
                ("MP recovery",348),("Price",356),("Shop price",360),("Stack limit",364)],
            15 => &[("Price",332),("Shop price",336),("Stack limit",356)],
            23 => &[("Price",452),("Shop price",456),("Grade",460),("Required level",464)],
            24 => &[("Price",452),("Shop price",456),("Required level",460)],
            36 => &[("Required level",328),("Price",336),("Shop price",340),("Stack limit",424)],
            123 => &[("Required level",332),("Price",372),("Shop price",376),("Stack limit",380)],
            _ => &[],
        };
        fields.iter().map(|(name, offset)| (*name, number(&self.record, *offset) as i32 as f64)).collect()
    }
}
