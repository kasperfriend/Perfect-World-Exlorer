//! Client EC model references and the string table in A3D skin model headers.
//! These are dependency parsers, not a skinning or animation renderer.
#[derive(Debug)]
pub struct EcModel {
    pub version: u32,
    pub skin_model_path: String,
    /// Optional in older descriptors; absence is not an invented render default.
    pub original_color: Option<u32>,
    pub source_blend: Option<u32>,
    pub destination_blend: Option<u32>,
    pub default_speed: Option<f32>,
    pub actions: Vec<CombinedAction>,
    pub events: Vec<EcmEvent>,
}
#[derive(Debug, Default)]
pub struct CombinedAction {
    pub name: String,
    pub loops: Option<i32>,
    pub speed: Option<f32>,
    pub bases: Vec<BaseAction>,
}
#[derive(Debug, Default)]
pub struct BaseAction {
    pub name: String,
    pub start_ms: Option<i32>,
    pub loop_min: Option<i32>,
    pub loop_max: Option<i32>,
}
/// Preserve event fields without claiming to implement their playback semantics.
#[derive(Debug)]
pub struct EcmEvent {
    pub kind: i32,
    pub action: Option<String>,
    pub fields: Vec<(String, String)>,
}
impl EcmEvent {
    pub fn gfx_path(&self) -> Option<String> {
        (self.kind == 100)
            .then(|| self.fields.iter().find(|x| x.0 == "FxFilePath"))
            .flatten()
            .map(|x| normalize(&x.1))
            .filter(|x| !x.is_empty())
    }
}
#[derive(Debug)]
pub struct SkinModel {
    pub version: u32,
    pub skeleton: String,
    pub skins: Vec<String>,
    pub physique: String,
    pub track_directory: String,
    pub actions: Vec<Action>,
    pub hangers: Vec<Hanger>,
    pub properties: Vec<(String, String)>,
}
#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub start: f32,
    pub end: f32,
    pub fps: Option<i32>,
    pub track_file: String,
    pub joints: Vec<JointAction>,
}
#[derive(Debug, Clone)]
pub struct JointAction {
    pub joint: i32,
    pub start: f32,
    pub end: f32,
    pub loops: i32,
    pub speed: f32,
}
#[derive(Debug)]
pub struct Hanger {
    pub kind: i32,
    pub name: String,
    pub target: String,
    pub on_bone: bool,
    pub file: String,
    pub child_hook: String,
}
impl SkinModel {
    /// External STCK paths are relative to the SMD's own directory, then its
    /// track-directory field; they are not relative to models.pck's root.
    pub fn action_track_path(&self, descriptor_path: &str, action: &Action) -> Option<String> {
        if action.track_file.is_empty() {
            return None;
        }
        let descriptor = normalize(descriptor_path);
        let parent = descriptor.rsplit_once('/').map_or("", |x| x.0);
        Some(normalize(&format!(
            "{parent}/{}/{}",
            self.track_directory, action.track_file
        )))
    }
}
fn gbk(bytes: &[u8]) -> Result<String, String> {
    let (s, _, bad) = encoding_rs::GBK.decode(bytes);
    if bad {
        Err("invalid GBK model path".into())
    } else {
        Ok(s.into_owned())
    }
}
pub fn normalize(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .split('/')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("/")
        .to_lowercase()
}
pub fn parse_ecm(bytes: &[u8]) -> Result<EcModel, String> {
    let text = gbk(bytes)?;
    let mut version = None;
    let mut path = None;
    let mut original_color = None;
    let mut source_blend = None;
    let mut destination_blend = None;
    let mut default_speed = None;
    let mut declared_actions = None;
    let mut actions = Vec::<CombinedAction>::new();
    let mut base_counts = Vec::<Option<usize>>::new();
    let mut events = Vec::<EcmEvent>::new();
    let mut header = true;
    let mut in_bases = false;
    let mut in_event = false;
    fn integer<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, String> {
        value
            .parse()
            .map_err(|_| format!("invalid ECM {key}: {value}"))
    }
    fn speed(key: &str, value: &str) -> Result<f32, String> {
        let v: f32 = integer(key, value)?;
        if !v.is_finite() || v <= 0. {
            return Err(format!("invalid ECM {key}: {value}"));
        }
        Ok(v)
    }
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "MOXTVersion" => version = Some(integer(key, value)?),
            "SkinModelPath" => path = Some(normalize(value)),
            "OrgColor" if header => {
                original_color =
                    Some(u32::from_str_radix(value, 16).map_err(|_| "invalid ECM OrgColor")?)
            }
            "SrcBlend" if header => source_blend = Some(integer(key, value)?),
            "DestBlend" if header => destination_blend = Some(integer(key, value)?),
            "DefSpeed" if header => default_speed = Some(speed(key, value)?),
            "ComActCount" => declared_actions = Some(integer::<usize>(key, value)?),
            "CombineActName" => {
                if value.is_empty() {
                    return Err("empty ECM combined-action name".into());
                }
                header = false;
                in_bases = false;
                in_event = false;
                actions.push(CombinedAction {
                    name: value.into(),
                    ..Default::default()
                });
                base_counts.push(None);
            }
            "EventType" => {
                header = false;
                in_bases = false;
                in_event = true;
                events.push(EcmEvent {
                    kind: integer(key, value)?,
                    action: actions.last().map(|a| a.name.clone()),
                    fields: Vec::new(),
                });
            }
            _ if in_event
                && !matches!(
                    key,
                    "AddiSkinCount" | "ChildCount" | "PhysFileName" | "ECMHookCount"
                ) =>
            {
                events
                    .last_mut()
                    .unwrap()
                    .fields
                    .push((key.into(), value.into()));
            }
            "BaseActCount" => {
                let count = integer::<usize>(key, value)?;
                *base_counts
                    .last_mut()
                    .ok_or("ECM base count without combined action")? = Some(count);
                in_bases = true;
            }
            "BaseActName" => {
                if !in_bases || value.is_empty() {
                    return Err("ECM base action outside base list or empty name".into());
                }
                actions
                    .last_mut()
                    .ok_or("ECM base action without combined action")?
                    .bases
                    .push(BaseAction {
                        name: value.into(),
                        ..Default::default()
                    });
            }
            "LoopCount" => {
                let loops = integer(key, value)?;
                if let Some(action) = actions.last_mut() {
                    if in_bases {
                        let base = action
                            .bases
                            .last_mut()
                            .ok_or("ECM loop count before base action")?;
                        base.loop_min = Some(loops);
                        base.loop_max = Some(loops);
                    } else {
                        action.loops = Some(loops);
                    }
                }
            }
            "PlaySpeed" => {
                if let Some(action) = actions.last_mut() {
                    action.speed = Some(speed(key, value)?);
                }
            }
            "ActStartTime" | "LoopMinNum" | "LoopMaxNum" if in_bases => {
                let base = actions
                    .last_mut()
                    .and_then(|a| a.bases.last_mut())
                    .ok_or("ECM base timing without base action")?;
                let v = Some(integer(key, value)?);
                match key {
                    "ActStartTime" => base.start_ms = v,
                    "LoopMinNum" => base.loop_min = v,
                    _ => base.loop_max = v,
                }
            }
            "EventCount" => {
                in_bases = false;
            }
            _ => {}
        }
    }
    if declared_actions.is_some_and(|n| n != actions.len()) {
        return Err("ECM combined-action count mismatch".into());
    }
    for (action, count) in actions.iter().zip(base_counts) {
        if count.is_some_and(|n| n != action.bases.len()) {
            return Err(format!("ECM base-action count mismatch: {}", action.name));
        }
        for base in &action.bases {
            if let (Some(min), Some(max)) = (base.loop_min, base.loop_max) {
                if min >= 0 && max >= 0 && min > max {
                    return Err(format!("ECM inverted loop bounds: {}", base.name));
                }
            }
        }
    }
    Ok(EcModel {
        version: version.ok_or("missing ECM version")?,
        skin_model_path: path
            .filter(|x| !x.is_empty())
            .ok_or("missing ECM skin model")?,
        original_color,
        source_blend,
        destination_blend,
        default_speed,
        actions,
        events,
    })
}
/// Parse A3D skin-model descriptors (v1–v9), including animation and attachments.
/// Layout informed by GameForge 3D Studio `angelica_model.cc` GPL-2.0-or-later,
/// commit 0a7fae8fcba0df7b0daa988db8696a0feaf49f7b.
pub fn parse_smd(bytes: &[u8]) -> Result<SkinModel, String> {
    use crate::animation::Reader;
    let body = bytes
        .strip_prefix(b"MOXB")
        .ok_or("unsupported SMD AFile header")?;
    let mut r = Reader { b: body, p: 0 };
    if r.take(4)? != b"DMSA" {
        return Err("invalid SMD header".into());
    }
    let version = r.u32()?;
    if !(1..=9).contains(&version) {
        return Err(format!("unsupported SMD version {version}"));
    }
    let count = r.count(4096)?;
    let action_count = r.count(100000)?;
    let hanger_count = r.count(100000)?;
    let prop_count = r.count(100000)?;
    r.take(60)?;
    let skeleton = r.string()?;
    let mut skins = Vec::with_capacity(count);
    for _ in 0..count {
        skins.push(r.string()?);
    }
    let physique = r.string()?;
    let track_directory = if version >= 8 {
        r.string()?
    } else {
        String::new()
    };
    let mut actions = Vec::with_capacity(action_count);
    for _ in 0..action_count {
        let name = r.string()?;
        let mut joints = Vec::new();
        let (start, end, fps) = if version < 6 {
            let _group = r.i32()?;
            let n = r.count(100000)?;
            let _flags = r.i32()?;
            for _ in 0..n {
                joints.push(JointAction {
                    joint: r.i32()?,
                    start: r.f32()?,
                    end: r.f32()?,
                    loops: r.i32()?,
                    speed: r.f32()?,
                });
            }
            joints
                .first()
                .map(|j| (j.start, j.end, None))
                .unwrap_or((0., 0., None))
        } else {
            let start = r.f32()?;
            let end = r.f32()?;
            let fps = if version >= 9 { Some(r.i32()?) } else { None };
            (start, end, fps)
        };
        let track_file = if version >= 7 {
            r.string()?
        } else {
            String::new()
        };
        actions.push(Action {
            name,
            start,
            end,
            fps,
            track_file,
            joints,
        });
    }
    let mut hangers = Vec::with_capacity(hanger_count);
    for _ in 0..hanger_count {
        let kind = r.i32()?;
        if kind != 0 && kind != 1 {
            return Err(format!("unsupported SMD hanger type {kind}"));
        }
        let name = r.string()?;
        let target = r.string()?;
        let _record_kind = r.i32()?;
        let on_bone = r.take(4)?[0] != 0;
        let file = r.string()?;
        let child_hook = if kind == 1 {
            r.string()?
        } else {
            String::new()
        };
        hangers.push(Hanger {
            kind,
            name,
            target,
            on_bone,
            file,
            child_hook,
        });
    }
    let mut properties = Vec::with_capacity(prop_count);
    for _ in 0..prop_count {
        properties.push((r.string()?, r.string()?));
    }
    Ok(SkinModel {
        version,
        skeleton,
        skins,
        physique,
        track_directory,
        actions,
        hangers,
        properties,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_normalization() {
        assert_eq!(normalize("Models\\\\Stuff\\A.SMD"), "models/stuff/a.smd");
    }
    #[test]
    fn smd_rejects_truncation() {
        assert!(parse_smd(b"MOXBDMSA").is_err());
    }
    #[test]
    fn smd_reads_all_referenced_skins_and_checks_lengths() {
        fn put(b: &mut Vec<u8>, v: u32) {
            b.extend_from_slice(&v.to_le_bytes());
        }
        fn text(b: &mut Vec<u8>, s: &str) {
            put(b, s.len() as u32);
            b.extend_from_slice(s.as_bytes());
        }
        let mut b = b"MOXBDMSA".to_vec();
        put(&mut b, 8);
        put(&mut b, 2);
        b.resize(88, 0);
        text(&mut b, "rig.bon");
        text(&mut b, "body.ski");
        text(&mut b, "extra.ski");
        text(&mut b, "");
        text(&mut b, "");
        let smd = parse_smd(&b).unwrap();
        assert_eq!(smd.skeleton, "rig.bon");
        assert_eq!(smd.skins, ["body.ski", "extra.ski"]);
        for n in 0..b.len() {
            assert!(parse_smd(&b[..n]).is_err(), "{n}");
        }
        b[12..16].copy_from_slice(&4097u32.to_le_bytes());
        assert!(parse_smd(&b).is_err());
    }
    #[test]
    fn parses_action_hanger_and_properties() {
        fn put(b: &mut Vec<u8>, v: u32) {
            b.extend_from_slice(&v.to_le_bytes());
        }
        fn text(b: &mut Vec<u8>, s: &str) {
            put(b, s.len() as u32);
            b.extend_from_slice(s.as_bytes());
        }
        let mut b = b"MOXBDMSA".to_vec();
        for x in [8u32, 1, 1, 1, 1] {
            put(&mut b, x);
        }
        b.resize(88, 0);
        for s in ["rig.bon", "body.ski", "body.phy", "tracks", "idle"] {
            text(&mut b, s);
        }
        put(&mut b, 0f32.to_bits());
        put(&mut b, 30f32.to_bits());
        text(&mut b, "idle.stck");
        put(&mut b, 1);
        text(&mut b, "weapon");
        text(&mut b, "hand");
        put(&mut b, 1);
        put(&mut b, 1);
        text(&mut b, "weapon.ecm");
        text(&mut b, "root");
        text(&mut b, "mode");
        text(&mut b, "visible");
        let m = parse_smd(&b).unwrap();
        assert_eq!(m.track_directory, "tracks");
        assert_eq!(m.actions[0].track_file, "idle.stck");
        assert_eq!(m.actions[0].end, 30.);
        assert_eq!(
            m.action_track_path("Models\\Scene\\Rig.SMD", &m.actions[0])
                .as_deref(),
            Some("models/scene/tracks/idle.stck")
        );
        let mut embedded = m.actions[0].clone();
        embedded.track_file.clear();
        assert!(m.action_track_path("models/rig.smd", &embedded).is_none());
        assert!(m.hangers[0].on_bone);
        assert_eq!(m.hangers[0].child_hook, "root");
        assert_eq!(m.properties[0], ("mode".into(), "visible".into()));
        for n in 0..b.len() {
            assert!(parse_smd(&b[..n]).is_err(), "{n}");
        }
    }
    #[test]
    fn parse_ecm_header() {
        let x = parse_ecm(b"MOXTVersion: 19\r\nSkinModelPath: Models\\Foo.SMD\r\n").unwrap();
        assert_eq!(x.version, 19);
        assert_eq!(x.skin_model_path, "models/foo.smd");
    }
}

#[cfg(test)]
mod ecm_schedule_tests {
    use super::*;
    fn parse(body: &str) -> Result<EcModel, String> {
        parse_ecm(format!("MOXTVersion: 47\nSkinModelPath: Models\\test.smd\n{body}").as_bytes())
    }
    #[test]
    fn maps_combined_name_and_keeps_random_loop_schedule() {
        let m=parse("OrgColor: ff804020\nSrcBlend: 5\nDestBlend: 6\nDefSpeed: 1.25\nComActCount: 1\nCombineActName: visible\nLoopCount: -1\nPlaySpeed: 0.5\nBaseActCount: 2\nBaseActName: local-a\nActStartTime: 0\nLoopMinNum: 3\nLoopMaxNum: 10\nBaseActName: local-b\nActStartTime: 3350\nLoopMinNum: 1\nLoopMaxNum: 1\nEventCount: 0\n").unwrap();
        assert_eq!(m.original_color, Some(0xff804020));
        assert_eq!(m.source_blend, Some(5));
        assert_eq!(m.destination_blend, Some(6));
        assert_eq!(m.default_speed, Some(1.25));
        let a = &m.actions[0];
        assert_eq!(a.name, "visible");
        assert_eq!(a.loops, Some(-1));
        assert_eq!(a.speed, Some(0.5));
        assert_eq!(a.bases[0].name, "local-a");
        assert_eq!(a.bases[0].loop_min, Some(3));
        assert_eq!(a.bases[0].loop_max, Some(10));
        assert_eq!(a.bases[1].start_ms, Some(3350));
    }
    #[test]
    fn legacy_loops_and_event_scope_are_distinct() {
        let m=parse("ComActCount: 1\nEventType: 100\nStartTime: 0\nTimeSpan: -1\nFxFilePath: scene\\attached.gfx\nHookName: hook\nCombineActName: visible\nLoopCount: 1\nBaseActCount: 1\nBaseActName: underlying\nActStartTime: 0\nLoopCount: -1\nEventCount: 1\nEventType: 999\nLoopCount: 12\nFxFilePath: other.gfx\n").unwrap();
        assert!(m.original_color.is_none());
        assert!(m.default_speed.is_none());
        assert_eq!(m.actions[0].loops, Some(1));
        assert_eq!(m.actions[0].bases[0].loop_max, Some(-1));
        assert_eq!(m.events.len(), 2);
        assert!(m.events[0].action.is_none());
        assert_eq!(
            m.events[0].gfx_path().as_deref(),
            Some("scene/attached.gfx")
        );
        assert_eq!(m.events[1].action.as_deref(), Some("visible"));
        assert!(m.events[1].gfx_path().is_none());
    }
    #[test]
    fn rejects_bad_counts_timing_and_nonfinite_speed() {
        for body in [
            "ComActCount: 1",
            "DefSpeed: NaN",
            "DefSpeed: 0",
            "CombineActName: a\nBaseActCount: 1",
            "CombineActName: a\nBaseActCount: 1\nBaseActName: b\nLoopMinNum: 9\nLoopMaxNum: 2",
            "CombineActName: a\nBaseActCount: 1\nBaseActName: b\nActStartTime: bad",
        ] {
            assert!(parse(body).is_err(), "accepted {body}");
        }
    }
}
