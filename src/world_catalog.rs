//! Discoverable client worlds/maps and the names the client gives them.
//!
//! The viewer used to load `maps/world/world.*` unconditionally. The retail
//! client also ships stand-alone instance maps (`maps/a01/a01.*`, ...) and a
//! UTF-16 `configs/instance.txt` inside `configs.pck`, which pairs every map
//! folder with its client-authored name and instance ID. The Teleport List uses
//! this catalog to offer only maps whose terrain, scene and EC-BSD files can
//! actually be read, instead of offering a list of folder names that cannot be
//! loaded.
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};

/// One readable world/map directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldMap {
    /// Folder name and the path written in `instance.txt`, e.g. `world`, `a01`.
    pub id: String,
    /// Client-authored name. This is the literal name from `instance.txt`, which
    /// may use the client's localized script. Never invented from a web table.
    pub name: String,
    /// A readable UI name: an ASCII alias for common map IDs where the client's
    /// own name may need a font the viewer does not embed, otherwise the client
    /// name (or a zone name recovered from `precinct.clt`).
    pub display_name: String,
    /// `element/maps/<id>` (or a stand-alone map directory).
    pub dir: PathBuf,
    /// The `element` folder that owns this map's shared PCK archives.
    pub element: PathBuf,
    /// Native instance IDs from `instance.txt`. A folder can serve more than one
    /// ID (for example `b30` serves six territory-war instances).
    pub instance_ids: Vec<u32>,
    /// Authored world-region grid from `instance.txt`; used as a hint for the
    /// client's own minimap artwork.
    pub rows: u32,
    pub cols: u32,
}
impl WorldMap {
    pub fn is_main_world(&self) -> bool {
        self.id.eq_ignore_ascii_case("world")
    }
    /// Stable, readable label for the selected text of a combo box.
    pub fn label(&self) -> String {
        format!("{} ({})", self.display_name, self.id)
    }
    /// Full details for a tooltip, including the client's literal name.
    pub fn tooltip(&self) -> String {
        let ids = if self.instance_ids.is_empty() {
            "not listed in instance.txt".to_string()
        } else {
            self.instance_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "Client name: {}\nMap: {}\nInstance IDs: {}\nFolder: {}",
            self.name,
            self.id,
            ids,
            self.dir.display()
        )
    }
    /// Resolve `<map stem>.<ext>` case-insensitively, with the main-world file
    /// names as a fallback for stand-alone/legacy directories.
    pub fn file(&self, ext: &str) -> PathBuf {
        data_file(&self.dir, ext)
    }
    fn from_dir(dir: &Path, records: &HashMap<String, Vec<InstanceRecord>>) -> Option<Self> {
        let id = dir.file_name().and_then(OsStr::to_str)?.to_string();
        let required = ["trn2", "ecwld", "ecbsd"]
            .iter()
            .all(|ext| data_file(dir, ext).is_file());
        if !required {
            return None;
        }
        let element = dir
            .parent()
            .map(|maps| {
                if maps.file_name() == Some(OsStr::new("maps")) {
                    maps.parent().unwrap_or(maps)
                } else {
                    maps
                }
            })
            .unwrap_or(dir)
            .to_path_buf();
        let key = id.to_ascii_lowercase();
        let records = records.get(&key).cloned().unwrap_or_default();
        let client_name = records
            .first()
            .map(|r| r.name.clone())
            .filter(|name| !name.trim().is_empty());
        let fallback_instance_ids: Vec<_> = implied_instance_id(&id).into_iter().collect();
        let fallback_name = first_zone_name(dir).unwrap_or_else(|| pretty_id(&id));
        let name = client_name.unwrap_or_else(|| fallback_name.clone());
        let display_name = english_name(&id).map(str::to_string).unwrap_or_else(|| {
            if name.chars().all(|c| c.is_ascii() || c.is_whitespace()) {
                name.clone()
            } else {
                fallback_name
            }
        });
        let is_main_world = id.eq_ignore_ascii_case("world");
        Some(Self {
            id,
            name,
            display_name,
            dir: dir.to_path_buf(),
            element,
            instance_ids: if records.is_empty() {
                fallback_instance_ids
            } else {
                records.iter().map(|r| r.id).collect()
            },
            rows: records
                .first()
                .map_or(if is_main_world { 11 } else { 1 }, |r| r.rows),
            cols: records
                .first()
                .map_or(if is_main_world { 8 } else { 1 }, |r| r.cols),
        })
    }
}

/// The readable maps in one `element` folder. A missing `configs.pck` is a
/// warning, not a failure: the folder names and `precinct.clt` still provide a
/// usable list.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub maps: Vec<WorldMap>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug)]
struct InstanceRecord {
    id: u32,
    name: String,
    path: String,
    rows: u32,
    cols: u32,
}

/// Decode the client's UTF-16LE text files; accept UTF-8 and GBK for unusual
/// client variants and for text fixtures used by tests.
pub fn decode_client_text(bytes: &[u8]) -> Result<String, String> {
    fn trim_bom(mut s: String) -> String {
        if s.starts_with('\u{feff}') {
            s.drain(..'\u{feff}'.len_utf8());
        }
        s
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16(&words)
            .map(trim_bom)
            .map_err(|e| format!("invalid UTF-16LE client text: {e}"));
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16(&words)
            .map(trim_bom)
            .map_err(|e| format!("invalid UTF-16BE client text: {e}"));
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(trim_bom(s.to_string()));
    }
    if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let zero_high_bytes = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        if zero_high_bytes * 4 > bytes.len() {
            let words = bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>();
            if let Ok(s) = String::from_utf16(&words) {
                return Ok(trim_bom(s));
            }
        }
    }
    let (decoded, _, bad) = encoding_rs::GBK.decode(bytes);
    if bad {
        return Err("client text is neither UTF-16, UTF-8 nor GBK".into());
    }
    Ok(trim_bom(decoded.into_owned()))
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'"' => quoted = !quoted,
            b'/' if !quoted && bytes[i + 1] == b'/' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}
fn quoted(s: &str) -> Option<String> {
    let s = s.trim();
    let inner = s.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(inner[..end].to_string())
}
fn map_id_from_path(raw: &str) -> Option<String> {
    let normalized = raw.trim().replace('\\', "/");
    let id = normalized.rsplit('/').find(|part| !part.is_empty())?;
    if id.is_empty() || id == "." || id == ".." || id.contains(':') {
        return None;
    }
    Some(id.to_string())
}
fn parse_pair(s: &str) -> Option<(u32, u32)> {
    let values = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    match values.as_slice() {
        [rows, cols, ..] => Some((*rows, *cols)),
        _ => None,
    }
}

/// Parse the retail `instance.txt` block grammar. A record is a quoted name,
/// followed by a brace containing `ID`, `"path"` and `row, column` lines.
fn parse_instance_text(text: &str) -> Vec<InstanceRecord> {
    let lines = text
        .lines()
        .map(strip_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = quoted(line) else { continue };
        let Some(open) = (i + 1..lines.len().min(i + 4)).find(|&j| lines[j] == "{") else {
            continue;
        };
        let mut fields = Vec::new();
        for candidate in lines.iter().skip(open + 1).take(8) {
            if *candidate == "}" {
                break;
            }
            fields.push(*candidate);
            if fields.len() == 3 {
                break;
            }
        }
        if fields.len() < 3 {
            continue;
        }
        let Some(id) = fields[0]
            .split_whitespace()
            .next()
            .and_then(|v| v.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(path) = quoted(fields[1]).and_then(|p| map_id_from_path(&p)) else {
            continue;
        };
        let (rows, cols) = parse_pair(fields[2]).unwrap_or((1, 1));
        records.push(InstanceRecord {
            id,
            name,
            path,
            rows,
            cols,
        });
    }
    records
}

fn instance_records(element: &Path) -> Result<HashMap<String, Vec<InstanceRecord>>, String> {
    let path = element.join("configs.pck");
    let mut file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let entries =
        crate::pck::directory(&mut file).map_err(|e| format!("{}: {e}", path.display()))?;
    let entry = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("configs/instance.txt"))
        .ok_or_else(|| format!("{} has no configs/instance.txt", path.display()))?;
    let bytes = crate::pck::read_entry(&mut file, entry)
        .map_err(|e| format!("configs/instance.txt: {e}"))?;
    let text = decode_client_text(&bytes).map_err(|e| format!("configs/instance.txt: {e}"))?;
    let mut by_folder = HashMap::<String, Vec<InstanceRecord>>::new();
    for record in parse_instance_text(&text) {
        by_folder
            .entry(record.path.to_ascii_lowercase())
            .or_default()
            .push(record);
    }
    Ok(by_folder)
}

/// Find a file by a case-insensitive name. Client archives and extracted client
/// folders disagree on casing from time to time.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}
fn map_stem(dir: &Path) -> String {
    if let Some(id) = dir.file_name().and_then(OsStr::to_str) {
        if find_file(dir, &format!("{id}.ecbsd")).is_some() {
            return id.to_string();
        }
    }
    "world".to_string()
}
/// Path of `<map stem>.<ext>`; falls back to `world.<ext>` for stand-alone map
/// directories or clients that keep main-world file names in another folder.
pub fn data_file(dir: &Path, ext: &str) -> PathBuf {
    let stem = map_stem(dir);
    find_file(dir, &format!("{stem}.{ext}"))
        .or_else(|| find_file(dir, &format!("world.{ext}")))
        .unwrap_or_else(|| dir.join(format!("{stem}.{ext}")))
}
/// EC-BSD path for loaders that receive only a map directory (`environment`,
/// `water`, `grass`). For `maps/a01` this resolves `a01.ecbsd`, while legacy
/// `maps/world` and test fixtures keep resolving `world.ecbsd`.
pub fn ecbsd_path(dir: &Path) -> PathBuf {
    data_file(dir, "ecbsd")
}

fn first_zone_name(dir: &Path) -> Option<String> {
    let bytes = fs::read(dir.join("precinct.clt")).ok()?;
    let text = decode_client_text(&bytes).ok()?;
    for line in text.lines().map(strip_comment).map(str::trim) {
        if let Some(name) = quoted(line) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                // Many instance precincts name sub-zones as "Map - Quarter".
                // The common prefix is a better map label than one quarter.
                let short = name.split(" - ").next().unwrap_or(&name).trim();
                return Some(short.to_string());
            }
        }
    }
    None
}
/// Native instance ID implied by the client's map-folder naming when the
/// client's `instance.txt` is unavailable. `a01` is 101, `b30` is 230; the
/// main world is 1. One folder may also be shared by several authored IDs
/// (`b30` serves six territory-war instances), so this is only a fallback.
pub fn implied_instance_id(id: &str) -> Option<u32> {
    let lower = id.to_ascii_lowercase();
    if lower == "world" {
        return Some(1);
    }
    let mut chars = lower.chars();
    let prefix = chars.next()?;
    let digits = chars.take_while(|c| c.is_ascii_digit()).collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let number = digits.parse::<u32>().ok()?;
    match prefix {
        'a' => 100u32.checked_add(number),
        'b' => 200u32.checked_add(number),
        _ => None,
    }
}

fn pretty_id(id: &str) -> String {
    let mut result = String::new();
    for word in id.split(|c: char| c == '_' || c == '-' || c == ' ') {
        if word.is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.push_str(chars.as_str());
        }
    }
    if result.is_empty() {
        id.to_string()
    } else {
        result
    }
}

/// Well-known English map labels. The client name is still retained in `name`
/// and shown in the tooltip; this table only keeps the combo box legible when a
/// client's instance names use glyphs the default egui font does not embed.
fn english_name(id: &str) -> Option<&'static str> {
    Some(match id.to_ascii_lowercase().as_str() {
        "world" => "Perfect World — Mainland",
        "login" => "Character Selection",
        "a01" => "City of Abominations / Ghost City",
        "a02" => "Hero's Tomb / Secret Passage",
        "a03" => "Silk Cave",
        "a04" => "Earth Cave",
        "a05" => "Firecrag Grotto",
        "a06" => "Den of Rabid Wolves",
        "a07" => "Cave of the Vicious",
        "a08" => "Hall of Deception",
        "a09" => "Gate of Delirium",
        "a10" => "Secret Frostcover Grounds",
        "a11" => "Valley of Disaster",
        "a12" => "Forest Ruins",
        "a13" => "Cave of Sadistic Glee",
        "a14" => "Wraithgate",
        "a15" => "Hallucinatory Trench",
        "a16" => "Eden",
        "a17" => "Brimstone Pit",
        "a18" => "Temple of the Dragon",
        "a19" => "Nightscream Island",
        "a20" => "Snake Isle",
        "a21" => "Lothranis (Heaven)",
        "a22" => "Momaganon (Hell)",
        "a23" => "Seat of Torment",
        "a24" => "Abaddon",
        "a25" => "Warsong City",
        "a26" => "Palace of Nirvana",
        "a27" => "Lunar Glade",
        "a28" => "Valley of Reciprocity",
        "a29" => "Frostcover City",
        "a31" => "Twilight Temple",
        "a32" => "Cube of Fate",
        "a33" => "Chrono City",
        "a34" => "Perfect Chapel",
        "a35" => "Faction Base",
        "a37" => "Morai",
        "a38" => "Phoenix Valley",
        "a39" => "Endless Universe",
        "a40" => "Blighted Chamber",
        "a41" => "Advanced Endless Universe",
        "a42" => "Wargod Gulch",
        "a43" => "Nation Wars Base Camp",
        "a44" => "Nation Wars Capture the Flag",
        "a45" => "Nation Wars Bridge Battle",
        "a46" => "Nation Wars Crystal Contest",
        "a47" => "Sunset Valley",
        "a48" => "Unyielding Hall",
        "a49" => "Hidden Dragon Den",
        "a50" => "Realm of Reflection",
        "a61" => "Celestial Vale",
        "a62" => "Place of Origination",
        "a63" => "Primal World",
        "a64" => "Lothranis",
        "a66" => "Flowsilver Palace",
        "a67" => "Snowfall Valley",
        "a68" => "Home of Stealth",
        "a69" => "Nightfall Hall",
        "b01" => "Etherblade Arena",
        "b02" => "Lost Arena",
        "b03" => "Plume Arena",
        "b04" => "Archosaur Arena",
        "b05" => "Arena of Souls",
        "b06" => "Arena of Swords",
        "b30" => "Territory War",
        "mtv" => "MTV Island",
        "random03" => "Random Map 03",
        _ => return None,
    })
}

/// Only safe, single-folder IDs may be passed to `--world` or persisted in the
/// settings file. The catalog reads real directory names, which need not be
/// valid command-line settings tokens.
pub fn valid_world_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn sort_key(map: &WorldMap) -> (u8, u8, u32, String) {
    let id = map.id.to_ascii_lowercase();
    if id == "world" {
        return (0, 0, 0, id);
    }
    let group = id.chars().next().unwrap_or('z') as u8;
    let number = id
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<u32>()
        .unwrap_or(u32::MAX);
    (1, group, number, id)
}

/// Enumerate readable map folders. `element` is the folder containing `maps/`
/// and the shared PCK archives.
pub fn discover(element: &Path) -> Catalog {
    let (records, warning) = match instance_records(element) {
        Ok(records) => (records, None),
        Err(e) => (
            HashMap::new(),
            Some(format!(
                "Client map names unavailable ({e}); showing folder names."
            )),
        ),
    };
    let maps_dir = element.join("maps");
    let mut maps = Vec::new();
    match fs::read_dir(&maps_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    if let Some(map) = WorldMap::from_dir(&entry.path(), &records) {
                        maps.push(map);
                    }
                }
            }
        }
        Err(e) => {
            return Catalog {
                maps: Vec::new(),
                warning: Some(format!("Cannot read {}: {e}", maps_dir.display())),
            }
        }
    }
    maps.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    Catalog { maps, warning }
}

fn element_and_maps(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let candidates = [
        (root.join("element"), root.join("element/maps")),
        (root.to_path_buf(), root.join("maps")),
    ];
    for (element, maps) in candidates {
        if maps.is_dir() {
            return Some((element, maps));
        }
    }
    if root.file_name().and_then(OsStr::to_str) == Some("maps") {
        let element = root.parent().unwrap_or(root).to_path_buf();
        return Some((element, root.to_path_buf()));
    }
    None
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Resolve a launcher client root, an `element` folder, a `maps` folder, or a
/// direct map directory into the world to load. `preferred` is the persisted map
/// ID when one was chosen in the Teleport List.
pub fn resolve(root: &Path, preferred: Option<&str>) -> Result<WorldMap, String> {
    let direct = WorldMap::from_dir(root, &HashMap::new());
    let (element, _) = if let Some(direct) = direct.as_ref() {
        (direct.element.clone(), root.to_path_buf())
    } else {
        element_and_maps(root).ok_or_else(|| {
            format!(
                "expected a client root, element folder, maps folder, or readable map directory: {}",
                root.display()
            )
        })?
    };
    let catalog = discover(&element);
    if let Some(direct) = direct {
        if let Some(enriched) = catalog.maps.iter().find(|map| same_dir(&map.dir, root)) {
            return Ok(enriched.clone());
        }
        let mut direct = direct;
        direct.element = element.clone();
        if let Some(record) = catalog
            .maps
            .iter()
            .find(|map| map.id.eq_ignore_ascii_case(&direct.id))
        {
            direct.name = record.name.clone();
            direct.display_name = record.display_name.clone();
            direct.instance_ids = record.instance_ids.clone();
            direct.rows = record.rows;
            direct.cols = record.cols;
        }
        return Ok(direct);
    }
    if catalog.maps.is_empty() {
        return Err(format!(
            "no readable world maps found under {} (need <map>.trn2, .ecwld and .ecbsd)",
            element.join("maps").display()
        ));
    }
    if let Some(id) = preferred.map(str::trim).filter(|id| !id.is_empty()) {
        if let Some(map) = catalog
            .maps
            .iter()
            .find(|map| map.id.eq_ignore_ascii_case(id))
        {
            return Ok(map.clone());
        }
    }
    catalog
        .maps
        .iter()
        .find(|map| map.is_main_world())
        .or_else(|| catalog.maps.first())
        .cloned()
        .ok_or_else(|| "no readable world map".into())
}

/// Client root for diagnostics. A direct `element/maps/<id>` argument should
/// still be recognized as belonging to the containing client, so logs do not
/// silently default inside the client installation.
pub fn client_root(path: &Path) -> PathBuf {
    if let Some((element, _)) = element_and_maps(path) {
        if element.file_name() == Some(OsStr::new("element")) {
            return element.parent().unwrap_or(path).to_path_buf();
        }
        return path.to_path_buf();
    }
    if let Some(element) = path.parent().and_then(Path::parent) {
        if element.join("maps").is_dir() {
            if element.file_name() == Some(OsStr::new("element")) {
                return element.parent().unwrap_or(element).to_path_buf();
            }
            return element.to_path_buf();
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const INSTANCE: &str = r#"
// Client instance description file
"完美大陆"
{
1    // ID
"world" // path
11, 8
}
"鬼城"
{
101
"a01"
1, 1
}
"城战1"
{
230
"b30"
1, 1
}
"城战2"
{
231
"b30"
1, 1
}
"#;

    #[test]
    fn instance_records_keep_names_ids_and_duplicate_paths() {
        let records = parse_instance_text(INSTANCE);
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].name, "完美大陆");
        assert_eq!(records[0].id, 1);
        assert_eq!(records[0].path, "world");
        assert_eq!((records[0].rows, records[0].cols), (11, 8));
        assert_eq!(records[1].path, "a01");
        assert_eq!(records[2].path, "b30");
        assert_eq!(records[3].id, 231);
    }

    #[test]
    fn utf16_bom_and_comment_stripping_are_supported() {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(INSTANCE.encode_utf16().flat_map(u16::to_le_bytes));
        let text = decode_client_text(&bytes).unwrap();
        assert!(text.contains("\"a01\""));
        assert_eq!(parse_instance_text(&text).len(), 4);
    }

    #[test]
    fn discover_lists_only_readable_maps_and_uses_folder_fallback_names() {
        static UNIQUE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "pwe-world-catalog-{}-{}",
            std::process::id(),
            UNIQUE.fetch_add(1, Ordering::Relaxed)
        ));
        let maps = root.join("maps");
        for id in ["world", "a01", "broken"] {
            let dir = maps.join(id);
            fs::create_dir_all(&dir).unwrap();
            if id != "broken" {
                for ext in ["trn2", "ecwld", "ecbsd"] {
                    fs::write(dir.join(format!("{id}.{ext}")), []).unwrap();
                }
            }
        }
        let catalog = discover(&root);
        assert_eq!(catalog.maps.len(), 2);
        assert_eq!(catalog.maps[0].id, "world");
        assert_eq!(catalog.maps[0].display_name, "Perfect World — Mainland");
        assert_eq!(catalog.maps[0].instance_ids, vec![1]);
        assert_eq!(catalog.maps[1].id, "a01");
        // Without configs.pck, the client's aNN folder naming still gives the
        // instance's native ID rather than leaving destination filtering blind.
        assert_eq!(catalog.maps[1].instance_ids, vec![101]);
        assert!(catalog.warning.is_some());
        let resolved = resolve(&root, Some("a01")).unwrap();
        assert_eq!(resolved.id, "a01");
        assert_eq!(resolved.file("trn2"), maps.join("a01/a01.trn2"));
        let direct = resolve(&maps.join("a01"), Some("world")).unwrap();
        assert_eq!(direct.id, "a01");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ecbsd_path_follows_the_selected_map_stem() {
        static UNIQUE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "pwe-ecbsd-{}-{}",
            std::process::id(),
            UNIQUE.fetch_add(1, Ordering::Relaxed)
        ));
        let a01 = root.join("maps/a01");
        fs::create_dir_all(&a01).unwrap();
        fs::write(a01.join("a01.ecbsd"), []).unwrap();
        assert_eq!(ecbsd_path(&a01), a01.join("a01.ecbsd"));
        fs::remove_dir_all(root).unwrap();
    }
}
