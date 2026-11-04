//! Retail player race/model paths recovered from `CECPlayer` and `EC_Resource`.
//! Walker exposes races, with one representative unarmoured profession/gender
//! model for each. No substitute or generated character geometry is used.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Race {
    #[default]
    Human,
    Untamed,
    WingedElf,
    Tideborn,
}

impl Race {
    pub const ALL: [Self; 4] = [Self::Human, Self::Untamed, Self::WingedElf, Self::Tideborn];

    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "Human — Blademaster",
            Self::Untamed => "Untamed — Barbarian",
            Self::WingedElf => "Winged Elf — Cleric",
            Self::Tideborn => "Tideborn — Psychic",
        }
    }

    /// `res_PlayerModel` / `EC_Resource.cpp`: base ECM loaded without skins.
    pub fn model_path(self) -> &'static str {
        match self {
            Self::Human => "Models\\Players\\形象\\武侠男\\躯干\\武侠男.ecm",
            Self::Untamed => "Models\\Players\\形象\\妖兽男\\躯干\\妖兽男.ecm",
            Self::WingedElf => "Models\\Players\\形象\\羽灵男\\躯干\\羽灵男.ecm",
            Self::Tideborn => "Models\\Players\\形象\\巫师男\\躯干\\巫师男.ecm",
        }
    }

    /// `_body_skin`: professions with body variants use body ID 0 + 1.
    pub fn body_skin_path(self) -> &'static str {
        match self {
            Self::Human => "Models\\Players\\形象\\武侠男\\躯干\\武侠男.ski",
            Self::Untamed => "Models\\Players\\形象\\妖兽男\\躯干\\妖兽男1.ski",
            Self::WingedElf => "Models\\Players\\形象\\羽灵男\\躯干\\羽灵男.ski",
            Self::Tideborn => "Models\\Players\\形象\\巫师男\\躯干\\巫师男1.ski",
        }
    }

    /// `_head_skin`: the matching default head for body ID zero.
    pub fn head_skin_path(self) -> &'static str {
        match self {
            Self::Human => "Models\\Players\\形象\\武侠男\\头\\武侠男头.ski",
            Self::Untamed => "Models\\Players\\形象\\妖兽男\\头\\妖兽男头1.ski",
            Self::WingedElf => "Models\\Players\\形象\\羽灵男\\头\\羽灵男头.ski",
            Self::Tideborn => "Models\\Players\\形象\\巫师男\\头\\巫师男头1.ski",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_race_has_three_real_and_distinct_assets() {
        for race in Race::ALL {
            assert!(race.model_path().ends_with(".ecm"));
            assert!(race.body_skin_path().ends_with(".ski"));
            assert!(race.head_skin_path().ends_with(".ski"));
            assert_ne!(race.body_skin_path(), race.head_skin_path());
        }
        assert!(Race::Untamed.body_skin_path().ends_with("妖兽男1.ski"));
        assert!(Race::Untamed.head_skin_path().ends_with("妖兽男头1.ski"));
        assert!(Race::Tideborn.body_skin_path().ends_with("巫师男1.ski"));
        assert!(Race::Tideborn.head_skin_path().ends_with("巫师男头1.ski"));
    }
}
