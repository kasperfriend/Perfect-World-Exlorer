//! Text resources in configs.pck. Lines with explicit numeric IDs are parsed
//! independently, so malformed or comment lines never shift another item's ID.
use std::collections::HashMap;

pub fn keyed_strings(bytes: &[u8]) -> HashMap<u32, String> {
    let text = encoding_rs::GBK.decode(bytes).0;
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
}

/// Parse the client type blocks in item_ext_prop.txt. Comments are removed
/// before tokenizing so example numbers do not become addon IDs.
pub fn addon_types(bytes: &[u8]) -> HashMap<u32, u32> {
    let raw = encoding_rs::GBK.decode(bytes).0;
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
