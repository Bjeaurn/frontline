//! Edits `data/maps.ron` as text, so adding or changing one Map's entry keeps the file's comments
//! and the other entries as they are. The file is a RON map `{ "id": ( name: ..., file: ...,
//! look: ..., modes: [...], factions: (a, b) ), ... }`; only the entry being written is
//! re-rendered, in the layout the file already uses.

use frontline::maps::{MapId, MapInfo};
use std::collections::BTreeMap;

/// One Map's entry, as `maps.ron` stores it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// Asset path of the `.map` (`maps/arena.map`; relative to `content/private/` in a private
    /// `maps.ron`).
    pub file: String,
    pub look: Option<String>,
    pub modes: Vec<String>,
    pub factions: [String; 2],
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Entry {
    fn render(&self, id: &str) -> String {
        let look = match &self.look {
            Some(look) => format!("Some({})", quote(look)),
            None => "None".to_owned(),
        };
        let modes: Vec<String> = self.modes.iter().map(|m| quote(m)).collect();
        format!(
            "    {}: (\n        name: {},\n        file: {},\n        look: {look},\n        modes: [{}],\n        factions: ({}, {}),\n    ),\n",
            quote(id),
            quote(&self.name),
            quote(&self.file),
            modes.join(", "),
            quote(&self.factions[0]),
            quote(&self.factions[1]),
        )
    }
}

/// A located top-level entry.
struct Found {
    key: String,
    /// Start of the line the key is on.
    start: usize,
    /// Just after the `)`.
    close: usize,
    /// Just after the entry (past its comma and the end of the line, when they follow).
    end: usize,
    comma: bool,
}

/// Where the entries of a maps.ron text are, and where its closing brace is.
struct Layout {
    entries: Vec<Found>,
    /// Offset of the closing `}`.
    brace: usize,
}

fn scan(text: &str) -> Result<Layout, String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut entries = Vec::new();
    let mut brace = None;
    // The key string of the entry being read, and where its line starts.
    let mut pending: Option<(String, usize)> = None;
    let mut open: Option<(String, usize)> = None;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..].find("*/").ok_or("a /* comment never ends")?;
                i += end + 4;
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut value = String::new();
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    let ch = text[i..].chars().next().ok_or("a string never ends")?;
                    value.push(ch);
                    i += ch.len_utf8();
                }
                if i >= bytes.len() {
                    return Err("a string never ends".into());
                }
                i += 1;
                if depth == 1 && open.is_none() {
                    let line_start = text[..start].rfind('\n').map_or(0, |n| n + 1);
                    pending = Some((value, line_start));
                }
            }
            b'{' | b'(' | b'[' => {
                depth += 1;
                if bytes[i] == b'(' && depth == 2 && open.is_none() {
                    open = pending.take();
                }
                i += 1;
            }
            b'}' | b')' | b']' => {
                if depth == 0 {
                    return Err("an unmatched closing bracket".into());
                }
                depth -= 1;
                if bytes[i] == b')'
                    && depth == 1
                    && let Some((key, start)) = open.take()
                {
                    let close = i + 1;
                    let mut end = close;
                    let mut comma = false;
                    let mut j = close;
                    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b',') {
                        comma = true;
                        end = j + 1;
                    }
                    let mut k = end;
                    while k < bytes.len() && matches!(bytes[k], b' ' | b'\t' | b'\r') {
                        k += 1;
                    }
                    if bytes.get(k) == Some(&b'\n') {
                        end = k + 1;
                    }
                    entries.push(Found {
                        key,
                        start,
                        close,
                        end,
                        comma,
                    });
                }
                if bytes[i] == b'}' && depth == 0 {
                    brace = Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    if depth != 0 {
        return Err("an unclosed bracket".into());
    }
    Ok(Layout {
        entries,
        brace: brace.ok_or("no closing `}`")?,
    })
}

/// `text` with `id`'s entry replaced by `entry` (or added last). The result is parsed again
/// before it is returned, so a file this cannot edit safely is an error, not a broken file.
pub fn upsert(text: &str, id: &str, entry: &Entry) -> Result<String, String> {
    let layout = scan(text)?;
    let rendered = entry.render(id);
    let mut out = text.to_owned();
    match layout.entries.iter().find(|found| found.key == id) {
        Some(found) => out.replace_range(found.start..found.end, &rendered),
        None => {
            let line_start = text[..layout.brace].rfind('\n').map_or(0, |n| n + 1);
            let at = if text[line_start..layout.brace].trim().is_empty() {
                line_start
            } else {
                // `}` shares its line with something: break the line first.
                out.insert(layout.brace, '\n');
                layout.brace + 1
            };
            out.insert_str(at, &rendered);
            if let Some(last) = layout.entries.last()
                && !last.comma
            {
                out.insert(last.close, ',');
            }
        }
    }
    let parsed: BTreeMap<MapId, MapInfo> =
        ron::from_str(&out).map_err(|err| format!("the edited maps.ron would not parse: {err}"))?;
    match parsed.get(&MapId::new(id)) {
        Some(info) if info.name == entry.name && info.file == entry.file => Ok(out),
        _ => Err("the edited maps.ron lost the entry".into()),
    }
}

/// An empty maps.ron with the explanatory header of `data/maps.ron`'s private sibling.
pub fn empty_private() -> String {
    "// Maps of the private content folder, in the format of data/maps.ron; files are relative to\n// content/private/.\n{\n}\n"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, file: &str) -> Entry {
        Entry {
            name: name.into(),
            file: file.into(),
            look: Some("desert_dust".into()),
            modes: vec!["tdm".into(), "dom".into()],
            factions: ["ranger".into(), "harmattan".into()],
        }
    }

    const SAMPLE: &str = "// header comment\n// second line\n{\n    // about a\n    \"a\": (\n        name: \"A\", // trailing\n        file: \"maps/a.map\",\n        look: None,\n        modes: [],\n        factions: (\"ranger\", \"harmattan\"),\n    ),\n    \"b\": (\n        name: \"B\",\n        file: \"maps/b.map\",\n        factions: (\"ranger\", \"harmattan\"),\n    )\n}\n";

    fn parse(text: &str) -> BTreeMap<MapId, MapInfo> {
        ron::from_str(text).unwrap()
    }

    #[test]
    fn a_new_entry_goes_last_and_everything_else_stays() {
        let out = upsert(SAMPLE, "c", &entry("C \"quoted\"", "maps/c.map")).unwrap();
        let maps = parse(&out);
        assert_eq!(maps.len(), 3);
        assert_eq!(maps[&MapId::new("c")].name, "C \"quoted\"");
        assert_eq!(maps[&MapId::new("c")].modes, ["tdm", "dom"]);
        assert!(out.starts_with("// header comment\n// second line\n{\n    // about a\n"));
        assert!(
            out.contains("name: \"A\", // trailing"),
            "other entries are untouched"
        );
        // `b` had no trailing comma: it got one.
        assert!(out.contains("    ),\n    \"c\": ("), "{out}");
        assert!(out.ends_with("    ),\n}\n"));
    }

    #[test]
    fn an_existing_entry_is_replaced_in_place() {
        let out = upsert(SAMPLE, "a", &entry("Renamed", "maps/a.map")).unwrap();
        let maps = parse(&out);
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[&MapId::new("a")].name, "Renamed");
        assert_eq!(maps[&MapId::new("a")].look.as_deref(), Some("desert_dust"));
        assert!(out.contains("// about a"), "comments between entries stay");
        assert!(out.contains("// header comment"));
        assert!(
            !out.contains("trailing"),
            "the replaced entry's own comment goes"
        );
        // Replacing again with the same values changes nothing.
        assert_eq!(
            upsert(&out, "a", &entry("Renamed", "maps/a.map")).unwrap(),
            out
        );
    }

    #[test]
    fn an_empty_file_takes_an_entry() {
        let out = upsert("{\n}\n", "x", &entry("X", "maps/x.map")).unwrap();
        assert_eq!(parse(&out).len(), 1);
        let out = upsert(&empty_private(), "x", &entry("X", "maps/x.map")).unwrap();
        assert_eq!(parse(&out).len(), 1);
        assert!(out.starts_with("// Maps of the private"));
        let out = upsert("{}", "x", &entry("X", "maps/x.map")).unwrap();
        assert_eq!(parse(&out).len(), 1);
    }

    #[test]
    fn a_broken_file_is_refused_not_written() {
        assert!(upsert("{ \"a\": (", "x", &entry("X", "maps/x.map")).is_err());
        assert!(upsert("not ron at all", "x", &entry("X", "maps/x.map")).is_err());
    }

    #[test]
    fn the_real_registry_takes_entries_and_keeps_its_comments() {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps.ron"),
        )
        .unwrap();
        let before = parse(&text);
        let out = upsert(
            &text,
            "throwaway_test_map",
            &entry("Throwaway", "maps/throwaway_test_map.map"),
        )
        .unwrap();
        let after = parse(&out);
        assert_eq!(after.len(), before.len() + 1);
        for (id, info) in &before {
            assert_eq!(&after[id], info, "{id}");
        }
        assert!(out.starts_with(text.lines().next().unwrap()));
        let mut arena = before[&MapId::new("arena")].clone();
        arena.name = "Arena 2".into();
        let changed = upsert(
            &text,
            "arena",
            &Entry {
                name: arena.name.clone(),
                file: arena.file.clone(),
                look: arena.look.clone(),
                modes: arena.modes.clone(),
                factions: arena.factions.clone(),
            },
        )
        .unwrap();
        let after = parse(&changed);
        assert_eq!(after[&MapId::new("arena")], arena);
        assert_eq!(
            after[&MapId::new("crossroads")],
            before[&MapId::new("crossroads")]
        );
        assert_eq!(changed.lines().count(), text.lines().count(), "same layout");
    }
}
