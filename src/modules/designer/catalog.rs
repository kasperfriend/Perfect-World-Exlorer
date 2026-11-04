//! PW 1.4.2 (`elements.data` v27) records used by the character designer.
//!
//! The parser deliberately walks all 131 sections and requires exact end-of-file
//! consumption. A record layout from another client version must never shift the
//! face, equipment, or preset tables into silently plausible garbage.

use crate::ecmodel;
use std::collections::HashMap;

pub const ELEMENTS_V27: u32 = 0x3000_001b;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum Gender {
    Male = 0,
    Female = 1,
}
impl Gender {
    pub const ALL: [Self; 2] = [Self::Male, Self::Female];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Male => "Male",
            Self::Female => "Female",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Self::Male => "男",
            Self::Female => "女",
        }
    }
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Male),
            1 => Some(Self::Female),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
pub enum Class {
    Blademaster = 0,
    Wizard = 1,
    Psychic = 2,
    Venomancer = 3,
    Barbarian = 4,
    Assassin = 5,
    Archer = 6,
    Cleric = 7,
}
impl Class {
    pub const ALL: [Self; 8] = [
        Self::Blademaster,
        Self::Wizard,
        Self::Psychic,
        Self::Venomancer,
        Self::Barbarian,
        Self::Assassin,
        Self::Archer,
        Self::Cleric,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn bit(self) -> u32 {
        1 << self.index()
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Blademaster => "Blademaster",
            Self::Wizard => "Wizard",
            Self::Psychic => "Psychic",
            Self::Venomancer => "Venomancer",
            Self::Barbarian => "Barbarian",
            Self::Assassin => "Assassin",
            Self::Archer => "Archer",
            Self::Cleric => "Cleric",
        }
    }
    pub fn allows(self, gender: Gender) -> bool {
        match self {
            Self::Venomancer => gender == Gender::Female,
            Self::Barbarian => gender == Gender::Male,
            _ => true,
        }
    }
    pub fn default_gender(self) -> Gender {
        if self == Self::Venomancer {
            Gender::Female
        } else {
            Gender::Male
        }
    }
    /// `CECPlayer::_body_skin` / `_head_skin` body/head folders. These are
    /// profession-specific even though `CECFace::_face_skin` shares the two
    /// human face rigs below.
    pub fn body_folder(self, gender: Gender) -> &'static str {
        match (self, gender) {
            (Self::Blademaster, Gender::Male) => "武侠男",
            (Self::Blademaster, Gender::Female) => "武侠女",
            (Self::Wizard, Gender::Male) => "法师男",
            (Self::Wizard, Gender::Female) => "法师女",
            (Self::Psychic, Gender::Male) => "巫师男",
            (Self::Psychic, Gender::Female) => "巫师女",
            (Self::Venomancer, Gender::Female) => "妖精",
            (Self::Barbarian, Gender::Male) => "妖兽男",
            (Self::Assassin, Gender::Male) => "刺客男",
            (Self::Assassin, Gender::Female) => "刺客女",
            (Self::Archer, Gender::Male) => "羽芒男",
            (Self::Archer, Gender::Female) => "羽芒女",
            (Self::Cleric, Gender::Male) => "羽灵男",
            (Self::Cleric, Gender::Female) => "羽灵女",
            _ => unreachable!("invalid class/gender combination"),
        }
    }
    /// `CECFace::_face_skin` stems. Face sharing is independent of the
    /// profession-specific body/head folders above.
    pub fn face_stem(self, gender: Gender) -> &'static str {
        match (self, gender) {
            (Self::Blademaster | Self::Wizard, Gender::Male) => "武侠男标准脸",
            (Self::Blademaster | Self::Wizard, Gender::Female) => "法师女标准脸",
            (Self::Psychic, Gender::Male) => "巫师男标准脸",
            (Self::Psychic, Gender::Female) => "巫师女标准脸",
            (Self::Venomancer, Gender::Female) => "妖精标准脸",
            (Self::Barbarian, Gender::Male) => "妖兽男标准脸",
            (Self::Assassin, Gender::Male) => "刺客男标准脸",
            (Self::Assassin, Gender::Female) => "刺客女标准脸",
            (Self::Archer, Gender::Male) => "羽芒男标准脸",
            (Self::Archer, Gender::Female) => "羽芒女标准脸",
            (Self::Cleric, Gender::Male) => "羽灵男标准脸",
            (Self::Cleric, Gender::Female) => "羽灵女标准脸",
            _ => unreachable!("invalid class/gender combination"),
        }
    }
    pub fn numbered_body(self) -> bool {
        matches!(
            self,
            Self::Psychic | Self::Venomancer | Self::Barbarian | Self::Assassin
        )
    }
    fn generic_equipment_prefix(self, gender: Gender) -> &'static str {
        match self {
            Self::Venomancer => "妖精",
            Self::Barbarian => "妖兽",
            _ => match gender {
                Gender::Male => "男通用",
                Gender::Female => "女通用",
            },
        }
    }
    fn default_equipment_prefix(self, gender: Gender) -> &'static str {
        match self {
            Self::Venomancer => "妖精",
            Self::Barbarian => "妖兽",
            _ => self.body_folder(gender),
        }
    }
    pub fn default_ini(self, gender: Gender) -> String {
        format!(
            "userdata/character/character{}{}.ini",
            self.index(),
            gender.index()
        )
    }
    pub fn body_model(self, gender: Gender) -> String {
        let folder = self.body_folder(gender);
        format!("models/players/形象/{folder}/躯干/{folder}.ecm")
    }
    pub fn body_skin(self, gender: Gender, body_id: u16) -> String {
        let folder = self.body_folder(gender);
        let suffix = if self.numbered_body() {
            (body_id + 1).to_string()
        } else {
            String::new()
        };
        format!("models/players/形象/{folder}/躯干/{folder}{suffix}.ski")
    }
    pub fn head_skin(self, gender: Gender, body_id: u16) -> String {
        let folder = self.body_folder(gender);
        let suffix = if self.numbered_body() {
            (body_id + 1).to_string()
        } else {
            String::new()
        };
        format!("models/players/形象/{folder}/头/{folder}头{suffix}.ski")
    }
    pub fn face_asset(self, gender: Gender, body_id: u16, extension: &str) -> String {
        let stem = self.face_stem(gender);
        let suffix = if self.numbered_body() {
            (body_id + 1).to_string()
        } else {
            String::new()
        };
        format!("models/face/{stem}/{stem}{suffix}.{extension}")
    }
    /// Equipment LOD path. `LoadPlayerSkin` appends 一级/二级/三级 by LOD;
    /// the essence's item level is unrelated to this suffix.
    pub fn equipment_skin(self, gender: Gender, realname: &str, lod: i32) -> String {
        let level = match lod {
            0 => "一级",
            1 => "二级",
            2 => "三级",
            _ => "",
        };
        let prefix = self.generic_equipment_prefix(gender);
        let gender = gender.folder();
        ecmodel::normalize(&format!(
            "models/players/装备/{gender}/{realname}/{prefix}{realname}{level}.ski"
        ))
    }
    pub fn default_armor_skin(self, gender: Gender, slot: ArmorSlot) -> Option<String> {
        let name = match slot {
            ArmorSlot::Upper => "默认上衣",
            ArmorSlot::Lower => "默认裤子",
            ArmorSlot::Wrist => "默认护腕",
            ArmorSlot::Foot => "默认靴子",
            ArmorSlot::Head => return None,
        };
        let prefix = self.default_equipment_prefix(gender);
        let gender = gender.folder();
        Some(ecmodel::normalize(&format!(
            "models/players/装备/{gender}/{name}/{prefix}{name}一级.ski"
        )))
    }
    pub fn default_fashion_skin(self, gender: Gender, slot: ArmorSlot) -> Option<String> {
        let name = match (slot, gender) {
            (ArmorSlot::Upper, Gender::Female) => "时装默认上衣",
            (ArmorSlot::Lower, _) => "时装默认裤子",
            (ArmorSlot::Foot, Gender::Female) => "时装默认鞋子",
            _ => return None,
        };
        let prefix = self.generic_equipment_prefix(gender);
        let gender_folder = gender.folder();
        Some(ecmodel::normalize(&format!(
            "models/players/装备/{gender_folder}/{name}/{prefix}{name}一级.ski"
        )))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArmorSlot {
    Head,
    Upper,
    Wrist,
    Lower,
    Foot,
}
impl ArmorSlot {
    pub const ALL: [Self; 5] = [
        Self::Head,
        Self::Upper,
        Self::Wrist,
        Self::Lower,
        Self::Foot,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Head => "Head",
            Self::Upper => "Upper body",
            Self::Wrist => "Wrist",
            Self::Lower => "Lower body",
            Self::Foot => "Feet",
        }
    }
    pub fn skin_index(self) -> Option<usize> {
        match self {
            Self::Head => None,
            Self::Upper => Some(1),
            Self::Wrist => Some(2),
            Self::Lower => Some(3),
            Self::Foot => Some(4),
        }
    }
    pub fn from_location(location: i32) -> Option<Self> {
        match location {
            0 | 9 => Some(Self::Head),
            1 | 5 | 6 | 8 => Some(Self::Upper),
            2 => Some(Self::Wrist),
            3 | 7 => Some(Self::Lower),
            4 => Some(Self::Foot),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WeaponItem {
    pub id: u32,
    pub subtype: u32,
    pub name: String,
    pub right_model: String,
    pub left_model: String,
    pub class_mask: u32,
    pub required_level: i32,
    pub item_level: i32,
    pub action_type: i32,
}
#[derive(Clone, Debug)]
pub struct ArmorItem {
    pub id: u32,
    pub subtype: u32,
    pub name: String,
    pub realname: String,
    pub slot: ArmorSlot,
    pub equip_location: i32,
    pub class_mask: u32,
    pub required_level: i32,
    pub item_level: i32,
    pub id_hair: u32,
    pub id_hair_texture: u32,
}
#[derive(Clone, Debug)]
pub struct FashionItem {
    pub id: u32,
    pub subtype: u32,
    pub name: String,
    pub realname: String,
    pub slot: ArmorSlot,
    pub equip_location: i32,
    pub required_level: i32,
    pub item_level: i32,
    pub dye_count: i32,
    pub gender: u32,
    pub id_hair: u32,
    pub id_hair_texture: u32,
}
#[derive(Clone, Debug)]
pub struct FaceTexture {
    pub id: u32,
    pub name: String,
    pub base: String,
    pub high: String,
    pub part: u32,
    pub class_mask: u32,
    pub gender: u32,
    pub visualize: u32,
    pub user_data: u32,
    pub facepill_only: u32,
}
#[derive(Clone, Debug)]
pub struct FaceShape {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub part: u32,
    pub class_mask: u32,
    pub gender: u32,
    pub visualize: u32,
    pub user_data: u32,
    pub facepill_only: u32,
}
#[derive(Clone, Debug)]
pub struct HairItem {
    pub id: u32,
    pub name: String,
    pub skin: String,
    pub model: String,
    pub class_mask: u32,
    pub gender: u32,
    pub visualize: u32,
    pub facepill_only: u32,
}
#[derive(Clone, Debug)]
pub struct ExtraSkin {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub class_mask: u32,
    pub gender: u32,
    pub visualize: u32,
    pub facepill_only: u32,
}
#[derive(Clone, Debug)]
pub struct Preset {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub class_mask: u32,
    pub gender: u32,
}

#[derive(Default)]
pub struct Catalog {
    pub weapons: Vec<WeaponItem>,
    pub armor: Vec<ArmorItem>,
    pub fashion: Vec<FashionItem>,
    pub face_textures: Vec<FaceTexture>,
    pub face_shapes: Vec<FaceShape>,
    pub hair: Vec<HairItem>,
    pub moustache: Vec<ExtraSkin>,
    pub faling: Vec<ExtraSkin>,
    pub presets: Vec<Preset>,
    pub weapon_actions: HashMap<u32, i32>,
    pub armor_equip_masks: HashMap<u32, u32>,
    pub fashion_equip_masks: HashMap<u32, u32>,
}
impl Catalog {
    pub fn parse_v27(bytes: &[u8]) -> Result<Self, String> {
        let mut cursor = Cursor { bytes, pos: 0 };
        let version = cursor.u32()?;
        if version != ELEMENTS_V27 {
            return Err(format!(
                "unsupported elements.data version 0x{version:08x}; expected PW 1.4.2 v27"
            ));
        }
        let _timestamp = cursor.u32()?;
        let mut catalog = Self::default();
        for (section, size) in SECTION_SIZES.iter().copied().enumerate() {
            match size {
                Size::Fixed(record_size) => {
                    let count = cursor.count(record_size)?;
                    if parses_section(section) {
                        for _ in 0..count {
                            let record = cursor.bytes(record_size)?;
                            parse_record(section, record, &mut catalog)?;
                        }
                    } else {
                        cursor.skip(count.checked_mul(record_size).ok_or("table overflow")?)?;
                    }
                }
                Size::ControlTimestamp => {
                    let _tag = cursor.u32()?;
                    let length = cursor.u32()? as usize;
                    cursor.skip(length)?;
                    let _timestamp = cursor.u32()?;
                }
                Size::ControlPlain => {
                    let _tag = cursor.u32()?;
                    let length = cursor.u32()? as usize;
                    cursor.skip(length)?;
                }
                Size::Talk => {
                    let count = cursor.count(1)?;
                    for _ in 0..count {
                        skip_talk(&mut cursor)?;
                    }
                }
            }
        }
        if cursor.pos != bytes.len() {
            return Err(format!(
                "elements.data has {} unread trailing bytes",
                bytes.len() - cursor.pos
            ));
        }
        Ok(catalog)
    }
    pub fn weapon(&self, id: u32) -> Option<&WeaponItem> {
        self.weapons.iter().find(|item| item.id == id)
    }
    pub fn armor_item(&self, id: u32) -> Option<&ArmorItem> {
        self.armor.iter().find(|item| item.id == id)
    }
    pub fn fashion_item(&self, id: u32) -> Option<&FashionItem> {
        self.fashion.iter().find(|item| item.id == id)
    }
    pub fn face_texture(&self, id: u32) -> Option<&FaceTexture> {
        self.face_textures.iter().find(|item| item.id == id)
    }
    pub fn face_shape(&self, id: u32) -> Option<&FaceShape> {
        self.face_shapes.iter().find(|item| item.id == id)
    }
    pub fn hair_item(&self, id: u32) -> Option<&HairItem> {
        self.hair.iter().find(|item| item.id == id)
    }
    pub fn moustache_item(&self, id: u32) -> Option<&ExtraSkin> {
        self.moustache.iter().find(|item| item.id == id)
    }
    pub fn faling_item(&self, id: u32) -> Option<&ExtraSkin> {
        self.faling.iter().find(|item| item.id == id)
    }
    pub fn texture_options(&self, class: Class, gender: Gender, part: u32) -> Vec<&FaceTexture> {
        let mut items: Vec<_> = self
            .face_textures
            .iter()
            .filter(|item| {
                item.part == part
                    && item.gender == gender as u32
                    && item.class_mask & class.bit() != 0
                    && item.facepill_only == 0
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        items
    }
    pub fn shape_options(&self, class: Class, gender: Gender, part: u32) -> Vec<&FaceShape> {
        let mut items: Vec<_> = self
            .face_shapes
            .iter()
            .filter(|item| {
                item.part == part
                    && item.gender == gender as u32
                    && item.class_mask & class.bit() != 0
                    && item.facepill_only == 0
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        items
    }
    pub fn hair_options(&self, class: Class, gender: Gender) -> Vec<&HairItem> {
        let mut items: Vec<_> = self
            .hair
            .iter()
            .filter(|item| {
                item.gender == gender as u32
                    && item.class_mask & class.bit() != 0
                    && item.facepill_only == 0
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        items
    }
    pub fn preset_options(&self, class: Class, gender: Gender) -> Vec<&Preset> {
        let mut items: Vec<_> = self
            .presets
            .iter()
            .filter(|item| item.class_mask & class.bit() != 0 && item.gender == gender as u32)
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        items
    }
}

fn parses_section(section: usize) -> bool {
    matches!(
        section,
        2 | 3 | 5 | 6 | 60 | 61 | 64 | 65 | 67 | 77 | 83 | 84
    )
}
fn parse_record(section: usize, record: &[u8], catalog: &mut Catalog) -> Result<(), String> {
    let id = u32_at(record, 0)?;
    match section {
        2 => {
            catalog.weapon_actions.insert(id, i32_at(record, 352)?);
        }
        3 => {
            let subtype = u32_at(record, 4)?;
            catalog.weapons.push(WeaponItem {
                id,
                subtype,
                name: wide(record, 12, 64),
                right_model: path(record, 80, 128),
                left_model: path(record, 208, 128),
                class_mask: u32_at(record, 608)?,
                required_level: i32_at(record, 612)?,
                item_level: i32_at(record, 616)?,
                action_type: catalog.weapon_actions.get(&subtype).copied().unwrap_or(-1),
            });
        }
        5 => {
            catalog.armor_equip_masks.insert(id, u32_at(record, 68)?);
        }
        6 => {
            let location = i32_at(record, 364)?;
            let Some(slot) = ArmorSlot::from_location(location) else {
                return Ok(());
            };
            catalog.armor.push(ArmorItem {
                id,
                subtype: u32_at(record, 4)?,
                name: wide(record, 12, 64),
                realname: string(record, 76, 32),
                slot,
                equip_location: location,
                class_mask: u32_at(record, 388)?,
                required_level: i32_at(record, 392)?,
                item_level: i32_at(record, 368)?,
                id_hair: u32_at(record, 1100)?,
                id_hair_texture: u32_at(record, 1104)?,
            });
        }
        60 => catalog.face_textures.push(FaceTexture {
            id,
            name: wide(record, 4, 64),
            base: path(record, 68, 128),
            high: path(record, 196, 128),
            part: u32_at(record, 452)?,
            class_mask: u32_at(record, 456)?,
            gender: u32_at(record, 460)?,
            visualize: u32_at(record, 464)?,
            user_data: u32_at(record, 468)?,
            facepill_only: u32_at(record, 472)?,
        }),
        61 => catalog.face_shapes.push(FaceShape {
            id,
            name: wide(record, 4, 64),
            path: path(record, 68, 128),
            part: u32_at(record, 324)?,
            class_mask: u32_at(record, 328)?,
            gender: u32_at(record, 332)?,
            visualize: u32_at(record, 336)?,
            user_data: u32_at(record, 340)?,
            facepill_only: u32_at(record, 344)?,
        }),
        64 => catalog.hair.push(HairItem {
            id,
            name: wide(record, 4, 64),
            skin: path(record, 68, 128),
            model: path(record, 196, 128),
            class_mask: u32_at(record, 452)?,
            gender: u32_at(record, 456)?,
            visualize: u32_at(record, 460)?,
            facepill_only: u32_at(record, 464)?,
        }),
        65 => catalog.moustache.push(ExtraSkin {
            id,
            name: wide(record, 4, 64),
            path: path(record, 68, 128),
            class_mask: u32_at(record, 324)?,
            gender: u32_at(record, 328)?,
            visualize: u32_at(record, 332)?,
            facepill_only: u32_at(record, 336)?,
        }),
        67 => catalog.presets.push(Preset {
            id,
            name: wide(record, 4, 64),
            path: path(record, 68, 128),
            class_mask: u32_at(record, 196)?,
            gender: u32_at(record, 200)?,
        }),
        77 => catalog.faling.push(ExtraSkin {
            id,
            name: wide(record, 4, 64),
            path: path(record, 68, 128),
            class_mask: u32_at(record, 324)?,
            gender: u32_at(record, 328)?,
            visualize: u32_at(record, 332)?,
            facepill_only: u32_at(record, 336)?,
        }),
        83 => {
            catalog.fashion_equip_masks.insert(id, u32_at(record, 68)?);
        }
        84 => {
            let location = i32_at(record, 364)?;
            let Some(slot) = ArmorSlot::from_location(location) else {
                return Ok(());
            };
            catalog.fashion.push(FashionItem {
                id,
                subtype: u32_at(record, 4)?,
                name: wide(record, 12, 64),
                realname: string(record, 76, 32),
                slot,
                equip_location: location,
                required_level: i32_at(record, 372)?,
                item_level: i32_at(record, 368)?,
                dye_count: i32_at(record, 376)?,
                gender: u32_at(record, 388)?,
                id_hair: u32_at(record, 392)?,
                id_hair_texture: u32_at(record, 396)?,
            });
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| format!("truncated v27 field at {offset}"))?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}
fn i32_at(bytes: &[u8], offset: usize) -> Result<i32, String> {
    Ok(u32_at(bytes, offset)? as i32)
}
fn nul(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
fn string(bytes: &[u8], offset: usize, length: usize) -> String {
    let Some(raw) = bytes.get(offset..offset + length) else {
        return String::new();
    };
    encoding_rs::GBK.decode(nul(raw)).0.into_owned()
}
fn wide(bytes: &[u8], offset: usize, length: usize) -> String {
    let Some(raw) = bytes.get(offset..offset + length) else {
        return String::new();
    };
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}
fn path(bytes: &[u8], offset: usize, length: usize) -> String {
    ecmodel::normalize(&string(bytes, offset, length))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Cursor<'_> {
    fn bytes(&mut self, length: usize) -> Result<&[u8], String> {
        let end = self
            .pos
            .checked_add(length)
            .ok_or("elements.data overflow")?;
        let result = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| format!("truncated elements.data at {}", self.pos))?;
        self.pos = end;
        Ok(result)
    }
    fn skip(&mut self, length: usize) -> Result<(), String> {
        self.bytes(length).map(|_| ())
    }
    fn u32(&mut self) -> Result<u32, String> {
        u32_at(self.bytes(4)?, 0)
    }
    fn count(&mut self, record_size: usize) -> Result<usize, String> {
        let count = self.u32()? as usize;
        let remaining = self.bytes.len() - self.pos;
        if record_size != 0 && count > remaining / record_size {
            return Err(format!("impossible elements.data record count {count}"));
        }
        Ok(count)
    }
}
fn skip_talk(cursor: &mut Cursor<'_>) -> Result<(), String> {
    let _id = cursor.u32()?;
    cursor.skip(128)?;
    let windows = cursor.count(1)?;
    for _ in 0..windows {
        let _id = cursor.u32()?;
        let _parent = cursor.u32()?;
        let talk_len = cursor.u32()? as usize;
        cursor.skip(talk_len.checked_mul(2).ok_or("talk text overflow")?)?;
        let options = cursor.count(136)?;
        cursor.skip(options.checked_mul(136).ok_or("talk options overflow")?)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Size {
    Fixed(usize),
    ControlTimestamp,
    ControlPlain,
    Talk,
}
use Size::{ControlPlain as Plain, ControlTimestamp as Timestamp, Fixed as F, Talk as T};
const SECTION_SIZES: [Size; 131] = [
    F(84),
    F(68),
    F(356),
    F(1412),
    F(68),
    F(72),
    F(1120),
    F(68),
    F(72),
    F(1164),
    F(68),
    F(68),
    F(376),
    F(68),
    F(68),
    F(368),
    F(68),
    F(364),
    F(68),
    F(624),
    Timestamp,
    F(68),
    F(348),
    F(524),
    F(488),
    F(348),
    F(348),
    F(352),
    F(348),
    F(208),
    F(888),
    F(68),
    F(892),
    F(68),
    F(340),
    F(68),
    F(436),
    F(84),
    F(196),
    F(1528),
    F(72),
    F(1224),
    F(72),
    F(72),
    F(200),
    F(200),
    F(1092),
    F(1092),
    F(644),
    F(584),
    F(72),
    F(460),
    F(328),
    F(72),
    F(68),
    F(1228),
    F(72),
    F(68),
    F(852),
    T,
    F(476),
    F(348),
    F(196),
    F(336),
    F(468),
    F(340),
    F(208),
    F(204),
    F(68),
    F(68),
    F(416),
    F(196),
    F(160),
    F(612),
    F(552),
    F(404),
    F(344),
    F(340),
    F(668),
    F(68),
    F(452),
    F(72),
    F(68),
    F(72),
    F(412),
    F(68),
    F(68),
    F(488),
    F(68),
    F(68),
    F(2412),
    F(292),
    F(68),
    F(344),
    F(68),
    F(480),
    F(628),
    F(360),
    F(344),
    F(480),
    F(344),
    Plain,
    F(148),
    F(1092),
    F(368),
    F(76),
    F(584),
    F(76),
    F(356),
    F(436),
    F(344),
    F(76),
    F(76),
    F(76),
    F(392),
    F(348),
    F(356),
    F(356),
    F(348),
    F(344),
    F(368),
    F(1356),
    F(68),
    F(392),
    F(352),
    F(360),
    F(384),
    F(344),
    F(644),
    F(2116),
    F(1092),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentic_paths_keep_class_specific_and_shared_assets_distinct() {
        assert_eq!(
            Class::Blademaster.body_skin(Gender::Female, 0),
            "models/players/形象/武侠女/躯干/武侠女.ski"
        );
        assert_eq!(
            Class::Wizard.body_model(Gender::Male),
            "models/players/形象/法师男/躯干/法师男.ecm"
        );
        assert_eq!(
            Class::Blademaster.head_skin(Gender::Male, 0),
            "models/players/形象/武侠男/头/武侠男头.ski"
        );
        assert_eq!(
            Class::Psychic.head_skin(Gender::Male, 3),
            "models/players/形象/巫师男/头/巫师男头4.ski"
        );
        assert_eq!(
            Class::Barbarian
                .default_armor_skin(Gender::Male, ArmorSlot::Upper)
                .unwrap(),
            "models/players/装备/男/默认上衣/妖兽默认上衣一级.ski"
        );
        assert_eq!(
            Class::Cleric.face_asset(Gender::Female, 0, "bon"),
            "models/face/羽灵女标准脸/羽灵女标准脸.bon"
        );
    }
    #[test]
    fn parses_authentic_v27_to_the_exact_last_byte() {
        let path = std::path::Path::new("google_drive/pw142/element/data/elements.data");
        if !path.exists() {
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let catalog = Catalog::parse_v27(&bytes).unwrap();
        assert_eq!(catalog.weapons.len(), 2020);
        assert_eq!(catalog.armor.len(), 1649);
        assert_eq!(catalog.face_textures.len(), 184);
        assert_eq!(catalog.face_shapes.len(), 78);
        assert_eq!(catalog.hair.len(), 185);
        assert_eq!(catalog.fashion.len(), 724);
        assert_eq!(catalog.presets.len(), 180);
        let preset = catalog.presets.iter().find(|item| item.id == 769).unwrap();
        assert_eq!(preset.name, "Exotic");
        assert!(preset.path.ends_with("羽灵女异域1.ini"));
        assert!(catalog.face_shape(117).is_none());
        assert!(catalog.face_shape(126).is_none());
        assert!(catalog.face_texture(106).is_none());
        assert!(catalog.face_texture(15776).is_none());
        assert!(catalog.face_texture(100).is_some());
    }
}
