//! Text resources in configs.pck. Lines with explicit numeric IDs are parsed
//! independently, so malformed or comment lines never shift another item's ID.
use std::collections::HashMap;

fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        encoding_rs::GBK.decode(bytes).0.into_owned()
    }
}

pub fn numeric_pairs(bytes: &[u8]) -> HashMap<u32, u32> {
    decode(bytes)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
        })
        .collect()
}

pub fn keyed_strings(bytes: &[u8]) -> HashMap<u32, String> {
    let text = decode(bytes);
    let mut result = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('/') {
            continue;
        }
        let Some(start) = line.find('"') else {
            continue;
        };
        let Some(end) = line[start + 1..].rfind('"') else {
            continue;
        };
        let Ok(id) = line[..start].trim().parse::<u32>() else {
            continue;
        };
        result.entry(id).or_insert_with(|| {
            line[start + 1..start + 1 + end]
                .replace("\\r", "\n")
                .replace("\\n", "\n")
        });
    }
    result
}

/// The game's colour escape is a caret followed by exactly six hex digits.
pub fn plain_text(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '^' {
            let next: String = chars.clone().take(6).collect();
            if next.len() == 6 && next.bytes().all(|c| c.is_ascii_hexdigit()) {
                for _ in 0..6 {
                    chars.next();
                }
                continue;
            }
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_id_and_quoted_value_without_shifting_on_comments() {
        let values = keyed_strings(b"# comment\n42 \"^ff0000Sword\\nFine\"\ninvalid\n");
        assert_eq!(plain_text(&values[&42]), "Sword\nFine");
        assert_eq!(plain_text("a^oops"), "a^oops");
    }
    #[test]
    fn addon_blocks_ignore_comments() {
        let types = addon_types(b"/* type 7 {999} */ type: 2 { 41,42 } // 5 { 1 }\ntype: 3 {43}");
        assert_eq!(types.get(&41), Some(&2));
        assert_eq!(types.get(&43), Some(&3));
        assert!(!types.contains_key(&999));
    }
    #[test]
    fn reads_utf16_text() {
        let mut bytes = vec![0xff, 0xfe];
        for unit in "42 \"Name\"".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            keyed_strings(&bytes).get(&42).map(String::as_str),
            Some("Name")
        );
    }
}

/// Parse the client type blocks in item_ext_prop.txt. Comments are removed
/// before tokenizing so example numbers do not become addon IDs.
pub fn addon_types(bytes: &[u8]) -> HashMap<u32, u32> {
    let raw = decode(bytes);
    let mut clean = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            clean.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = '\0';
            for c in chars.by_ref() {
                if previous == '*' && c == '/' {
                    break;
                }
                previous = c;
            }
            clean.push(' ');
        } else {
            clean.push(ch);
        }
    }
    let clean = clean.replace('{', " { ").replace('}', " } ");
    let tokens: Vec<_> = clean
        .split(|c: char| !c.is_ascii_digit() && c != '{' && c != '}')
        .filter(|part| !part.is_empty())
        .collect();
    let mut types = HashMap::new();
    let mut index = 0;
    while index + 1 < tokens.len() {
        let Ok(kind) = tokens[index].parse::<u32>() else {
            index += 1;
            continue;
        };
        if tokens[index + 1] != "{" {
            index += 1;
            continue;
        }
        index += 2;
        while index < tokens.len() && tokens[index] != "}" {
            if let Ok(id) = tokens[index].parse::<u32>() {
                types.entry(id).or_insert(kind);
            }
            index += 1;
        }
        index += 1;
    }
    types
}

/// Interpret a conservative subset of v27 property types. Unknown types retain
/// their raw parameters in the UI rather than being assigned a guessed meaning.
pub fn describe_addon(kind: u32, params: &[i32]) -> Option<String> {
    let (label, percent, floating) = match kind {
        0 => ("Physical attack", false, false),
        1 => ("Maximum physical attack", false, false),
        2 => ("Physical attack", true, true),
        3 => ("Magic attack", false, false),
        4 => ("Maximum magic attack", false, false),
        5 => ("Magic attack", true, true),
        10 => ("Attack range", false, true),
        11 => ("Channeling time", true, true),
        12 => ("Physical defense", false, false),
        13 => ("Physical defense", true, true),
        14 => ("Elemental defense", false, false),
        35 => ("HP", false, false),
        36 => ("MP", false, false),
        37 => ("HP", true, true),
        38 => ("MP", true, true),
        39 => ("HP recovery", false, false),
        40 => ("MP recovery", false, false),
        41 => ("Strength", false, false),
        42 => ("Dexterity", false, false),
        43 => ("Magic", false, false),
        44 => ("Vitality", false, false),
        45 => ("Critical rate", true, true),
        46 => ("Accuracy", false, false),
        47 => ("Accuracy", true, true),
        48 => ("Movement speed", false, true),
        49 => ("Movement speed", true, true),
        50 => ("Evasion", false, false),
        51 => ("Evasion", true, true),
        52 => ("Durability", false, false),
        53 => ("Durability", true, true),
        59 => ("Attack level", false, false),
        60 => ("Defense level", false, false),
        61 => ("Elemental defense", true, true),
        _ => return None,
    };
    let raw = *params.first()?;
    if floating {
        let number = f32::from_bits(raw as u32);
        if !number.is_finite() {
            return None;
        }
        Some(if percent {
            format!(
                "{label}: {:+.2}%",
                number * if kind == 11 { -100. } else { 100. }
            )
        } else {
            format!("{label}: {number:+.2}")
        })
    } else if let Some(&high) = params
        .get(1)
        .filter(|&&high| matches!(kind, 1 | 4 | 41 | 42 | 43 | 44 | 46) && high != raw && high != 0)
    {
        Some(format!("{label}: {raw:+} to {high:+}"))
    } else {
        Some(format!("{label}: {raw:+}"))
    }
}
