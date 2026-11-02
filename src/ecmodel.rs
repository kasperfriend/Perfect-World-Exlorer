//! Client EC model references and the string table in A3D skin model headers.
//! These are dependency parsers, not a skinning or animation renderer.
#[derive(Debug)]
pub struct EcModel {
    pub version: u32,
    pub skin_model_path: String,
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
    for line in text.lines() {
        if let Some(x) = line.strip_prefix("MOXTVersion:") {
            version = x.trim().parse().ok()
        };
        if let Some(x) = line.strip_prefix("SkinModelPath:") {
            path = Some(normalize(x));
        }
    }
    Ok(EcModel {
        version: version.ok_or("missing ECM version")?,
        skin_model_path: path
            .filter(|x| !x.is_empty())
            .ok_or("missing ECM skin model")?,
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
