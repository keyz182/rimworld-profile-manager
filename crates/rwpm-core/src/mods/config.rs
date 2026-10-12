//! `<SaveData>/Config/ModsConfig.xml`: the active mods in load order.

use std::path::Path;

use super::{ModsError, read_xml_text};

/// Active packageIds in load order, lowercased as the game stores them.
pub fn read_active_mods(path: &Path) -> Result<Vec<String>, ModsError> {
    let text = read_xml_text(path)?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| ModsError::Xml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let Some(list) = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("activeMods"))
    else {
        return Ok(Vec::new());
    };
    Ok(list
        .children()
        .filter(|n| n.has_tag_name("li"))
        .filter_map(|n| n.text())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect())
}

/// Edits `activeMods` in place: each `(id, None)` removes that entry, `(id, Some(new))`
/// renames it. Ids match case-insensitively. Every other byte (BOM, comments, layout)
/// stays as it was; the file is replaced atomically.
pub fn rewrite_active_mods(
    path: &Path,
    changes: &[(String, Option<String>)],
) -> Result<(), ModsError> {
    let io = |source| ModsError::Io {
        path: path.to_path_buf(),
        source,
    };
    let raw = std::fs::read(path).map_err(io)?;
    let bom: &[u8] = if raw.starts_with(BOM) { BOM } else { &[] };
    let text = read_xml_text(path)?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| ModsError::Xml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let lists = doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("activeMods"));
    for li in lists
        .flat_map(|l| l.children())
        .filter(|n| n.has_tag_name("li"))
    {
        let id = li.text().unwrap_or_default().trim().to_lowercase();
        match changes.iter().find(|(c, _)| c.to_lowercase() == id) {
            Some((_, None)) => edits.push((line_range(&text, li.range()), String::new())),
            Some((_, Some(new))) => {
                if let Some(t) = li.first_child() {
                    edits.push((t.range(), new.clone()));
                }
            }
            None => {}
        }
    }
    let mut out = text.clone();
    for (range, with) in edits.into_iter().rev() {
        out.replace_range(range, &with);
    }
    let tmp = path.with_extension("xml.rwpm-tmp");
    let mut bytes = bom.to_vec();
    bytes.extend_from_slice(out.as_bytes());
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Widens an element's range to its whole line when only whitespace surrounds it there.
fn line_range(text: &str, r: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let before = &text[..r.start];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let after = &text[r.end..];
    let line_end = after.find('\n').map_or(text.len(), |i| r.end + i + 1);
    let blank = |s: &str| s.chars().all(char::is_whitespace);
    if blank(&before[line_start..]) && blank(&text[r.end..line_end]) {
        line_start..line_end
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ModsConfigData>\n  \
        <version>1.6</version>\n  <activeMods>\n    <li>brrainz.harmony</li>\n    \
        <li>ludeon.rimworld</li>\n    <li>me.gone</li>\n    <li>a.b_steam</li>\n  \
        </activeMods>\n  <!-- keep me -->\n</ModsConfigData>\n";

    #[test]
    fn rewrite_removes_and_renames_only_those_entries() {
        let (_d, p) = write(LAYOUT.as_bytes());
        let changes = [
            ("ME.GONE".to_owned(), None),
            ("a.b_steam".to_owned(), Some("a.b".to_owned())),
        ];
        rewrite_active_mods(&p, &changes).expect("rewrite");
        let expected = LAYOUT
            .replace("    <li>me.gone</li>\n", "")
            .replace("<li>a.b_steam</li>", "<li>a.b</li>");
        assert_eq!(std::fs::read_to_string(&p).expect("read"), expected);
        assert_eq!(
            read_active_mods(&p).expect("read"),
            ["brrainz.harmony", "ludeon.rimworld", "a.b"]
        );
    }

    #[test]
    fn rewrite_keeps_bom_and_single_line_layout() {
        let mut body = BOM.to_vec();
        body.extend_from_slice(
            b"<ModsConfigData><activeMods><li>x.a</li><li>x.b</li></activeMods></ModsConfigData>",
        );
        let (_d, p) = write(&body);
        rewrite_active_mods(&p, &[("x.a".to_owned(), None)]).expect("rewrite");
        let mut expected = BOM.to_vec();
        expected.extend_from_slice(
            b"<ModsConfigData><activeMods><li>x.b</li></activeMods></ModsConfigData>",
        );
        assert_eq!(std::fs::read(&p).expect("read"), expected);
    }

    fn write(text: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ModsConfig.xml");
        std::fs::write(&path, text).expect("write");
        (dir, path)
    }

    #[test]
    fn reads_active_mods_in_order_lowercased() {
        let (_d, p) = write(
            b"<ModsConfigData><version>1.6</version><activeMods>\
              <li>brrainz.harmony</li><li> Ludeon.RimWorld </li><li>a.b_steam</li>\
              </activeMods><knownExpansions><li>ludeon.rimworld.royalty</li></knownExpansions>\
              </ModsConfigData>",
        );
        assert_eq!(
            read_active_mods(&p).expect("read"),
            ["brrainz.harmony", "ludeon.rimworld", "a.b_steam"]
        );
    }

    #[test]
    fn config_with_bom_and_empty_list() {
        let (_d, p) = write(
            b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\
              <ModsConfigData><activeMods /></ModsConfigData>",
        );
        assert!(read_active_mods(&p).expect("read").is_empty());
    }

    #[test]
    fn malformed_config_is_error() {
        let (_d, p) = write(b"<ModsConfigData><activeMods>");
        assert!(matches!(read_active_mods(&p), Err(ModsError::Xml { .. })));
        let missing = p.with_file_name("nope.xml");
        assert!(matches!(
            read_active_mods(&missing),
            Err(ModsError::Io { .. })
        ));
    }
}
