//! Persistent character ECM EventType-100 effects.
//!
//! Only actionless records belong here: attack/drop events are owned by action
//! scheduling and must never play merely because a character is standing.
//! This first visual player executes the static element-100 roots accepted by
//! `gfx_decal`; other roots are named warnings, not approximated effects.

use crate::{
    ecmodel,
    gfx_decal::{Quad, Root},
    model_asset::ModelAsset,
    model_attachment,
    modules::designer::{
        assembly::{Character, CharacterPose, TravelState},
        assets::Assets,
    },
};
use glam::Mat4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Body,
    Face,
    Weapon(usize),
    Flight,
    Mount,
}

struct HookBinding {
    hook: String,
    local: Mat4,
}

struct Effect {
    owner: Owner,
    path: String,
    root: Root,
    hook: Option<HookBinding>,
    scale: f32,
    alpha: f32,
}

pub struct CharacterGfx {
    effects: Vec<Effect>,
    pub warnings: Vec<String>,
}

fn field<'a>(event: &'a ecmodel::EcmEvent, key: &str) -> Option<&'a str> {
    event
        .fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn number(event: &ecmodel::EcmEvent, key: &str, default: f32) -> Result<f32, String> {
    let value = match field(event, key) {
        Some(value) => value
            .parse::<f32>()
            .map_err(|_| format!("invalid event {key}: {value}"))?,
        None => default,
    };
    if !value.is_finite() {
        return Err(format!("non-finite event {key}"));
    }
    Ok(value)
}

fn root_path(path: &str) -> String {
    let path = ecmodel::normalize(path);
    if path.starts_with("gfx/") {
        path
    } else {
        format!("gfx/{path}")
    }
}

impl CharacterGfx {
    pub fn load(assembly: &Character, assets: &mut Assets) -> Self {
        let mut result = Self {
            effects: Vec::new(),
            warnings: Vec::new(),
        };
        let mut models = vec![
            (Owner::Body, &assembly.body, "body"),
            (Owner::Face, &assembly.face, "face"),
        ];
        models.extend(
            assembly
                .weapons
                .iter()
                .enumerate()
                .map(|(index, weapon)| (Owner::Weapon(index), &weapon.asset, "weapon")),
        );
        if let Some(flight) = &assembly.flight {
            models.push((Owner::Flight, &flight.asset, "flight"));
        }
        if let Some(mount) = &assembly.mount {
            models.push((Owner::Mount, &mount.asset, "mount"));
        }
        for (owner, asset, label) in models {
            for (event_index, event) in asset.descriptor.events.iter().enumerate() {
                if event.action.is_some() || event.kind != 100 {
                    continue;
                }
                let Some(path) = event.gfx_path() else {
                    continue;
                };
                let path = root_path(&path);
                let origin = format!("{label} persistent event {event_index} ({path})");
                match Effect::load(owner, asset, event, &path, assets) {
                    Ok(effect) => result.effects.push(effect),
                    Err(error) => result.warnings.push(format!("{origin}: {error}")),
                }
            }
        }
        result
    }

    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    /// Resolve against the exact pose used by the character meshes. Hidden
    /// travel models contribute no effects, just as they contribute no meshes.
    pub fn quads(&self, assembly: &Character, pose: &CharacterPose) -> Result<Vec<Quad>, String> {
        let mut quads = Vec::new();
        for effect in &self.effects {
            let (asset, owner_pose) = match effect.owner {
                Owner::Body => (&assembly.body, Some(&pose.body)),
                Owner::Face => (&assembly.face, Some(&pose.face)),
                Owner::Weapon(index) if pose.travel != TravelState::Mounted => {
                    match (assembly.weapons.get(index), pose.weapons.get(index)) {
                        (Some(weapon), Some(weapon_pose)) => (&weapon.asset, Some(weapon_pose)),
                        _ => return Err(format!("{} lost its weapon pose", effect.path)),
                    }
                }
                Owner::Weapon(_) => (&assembly.body, None),
                Owner::Flight => match (&assembly.flight, &pose.flight) {
                    (Some(flight), Some(flight_pose)) => (&flight.asset, Some(flight_pose)),
                    (Some(_), None) => (&assembly.body, None),
                    _ => return Err(format!("{} lost its flight asset/pose", effect.path)),
                },
                Owner::Mount => match (&assembly.mount, &pose.mount) {
                    (Some(mount), Some(mount_pose)) => (&mount.asset, Some(mount_pose)),
                    (Some(_), None) => (&assembly.body, None),
                    _ => return Err(format!("{} lost its mount asset/pose", effect.path)),
                },
            };
            let Some(owner_pose) = owner_pose else {
                continue;
            };
            let parent = if let Some(binding) = &effect.hook {
                let hook = asset
                    .hook_matrix(owner_pose, &binding.hook)
                    .map_err(|error| format!("{}: {error}", effect.path))?;
                model_attachment::world(hook, binding.local)?
            } else {
                owner_pose.model
            };
            quads.extend(
                effect
                    .root
                    .quads(parent, effect.scale, effect.alpha)
                    .map_err(|error| format!("{}: {error}", effect.path))?,
            );
        }
        Ok(quads)
    }
}

impl Effect {
    fn load(
        owner: Owner,
        asset: &ModelAsset,
        event: &ecmodel::EcmEvent,
        path: &str,
        assets: &mut Assets,
    ) -> Result<Self, String> {
        for (key, expected) in [
            ("BindParent", "1"),
            ("Once", "0"),
            ("StartTime", "0"),
            ("TimeSpan", "-1"),
        ] {
            if field(event, key) != Some(expected) {
                return Err(format!("unsupported persistent event {key}"));
            }
        }
        for key in ["GfxOuterPath", "GfxRelToECM"] {
            if field(event, key).is_some_and(|value| value != "0") {
                return Err(format!("unsupported persistent event {key}"));
            }
        }
        let scale = number(event, "GfxScale", 1.)?;
        let alpha = number(event, "GfxAlpha", 1.)?;
        let speed = number(event, "GfxSpeed", 1.)?;
        if alpha < 0. || speed < 0. {
            return Err("negative persistent GFX alpha/speed".into());
        }
        // This static root subset has no keypoint or texture clock, so GfxSpeed
        // has no sampled value to alter. It is still validated above.
        if field(event, "UseModelAlpha") == Some("1")
            && asset.descriptor.render_settings().original_color >> 24 != 255
        {
            return Err("model-alpha persistent GFX needs a non-opaque model clock".into());
        }
        let hook = model_attachment::from_fields(&event.fields)?
            .map(|record| {
                let local = record.local()?;
                Ok::<_, String>(HookBinding {
                    hook: record.hook.clone(),
                    local,
                })
            })
            .transpose()?;
        let bytes = assets.read(path)?;
        let (text, _, bad) = encoding_rs::GBK.decode(&bytes);
        if bad {
            return Err("invalid GBK GFX root".into());
        }
        let root = Root::parse(&text)?;
        Ok(Self {
            owner,
            path: path.into(),
            root,
            hook,
            scale,
            alpha,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::designer::catalog::{Class, Gender};
    use crate::modules::walker::character::Character as WalkerCharacter;

    #[test]
    fn authentic_static_weapon_event_loads_and_action_events_stay_excluded() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let wanted = "models/weapons/人物/拳套/短刃/噬魂刃/噬魂刃_右极品.ecm";
        let weapon = assets
            .catalog
            .weapons
            .iter()
            .find(|weapon| weapon.right_model == wanted)
            .unwrap_or_else(|| panic!("no catalog weapon uses {wanted}"));
        let id = weapon.id;
        let mut design = assets
            .default_design(Class::Blademaster, Gender::Male)
            .unwrap();
        design.equipment.weapon = id;
        let character = WalkerCharacter::load(&mut assets, design, Mat4::IDENTITY).unwrap();
        assert!(character.gfx.effect_count() > 0);
        let quads = character
            .gfx
            .quads(&character.assembly, &character.posed.pose)
            .unwrap();
        assert!(!quads.is_empty());
        assert!(quads
            .iter()
            .any(|quad| quad.texture == "gfx/textures/粒子/jianguang.dds"));
        let weapon = character
            .assembly
            .weapons
            .iter()
            .find(|weapon| !weapon.asset.descriptor.events.is_empty())
            .unwrap();
        assert!(weapon
            .asset
            .descriptor
            .events
            .iter()
            .any(|event| event.action.is_some()));
        let persistent = character
            .assembly
            .weapons
            .iter()
            .flat_map(|weapon| &weapon.asset.descriptor.events)
            .filter(|event| event.action.is_none() && event.kind == 100)
            .count();
        assert_eq!(character.gfx.effect_count(), persistent);
    }
}
