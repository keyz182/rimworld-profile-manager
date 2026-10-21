//! Installed-mod index built the way the game builds its mod list (ModLister.cs:90-146,
//! 290-330), and resolution of active packageIds to folders (ModsConfig.cs:123-139).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{STEAM_POSTFIX, read_xml_text};
use crate::profile::ModSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `<install>/Data`: Core and DLCs. Never copied.
    Official,
    /// `<install>/Mods`.
    Local,
    Workshop,
}

impl Origin {
    /// Source recorded for a frozen copy; `None` for official content.
    pub fn mod_source(self) -> Option<ModSource> {
        match self {
            Self::Official => None,
            Self::Local => Some(ModSource::Local),
            Self::Workshop => Some(ModSource::Workshop),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledMod {
    /// The game's PackageId: lowercased `<packageId>`, plus `_steam` on a workshop duplicate.
    pub package_id: String,
    /// Folder name (the workshop id for workshop mods).
    pub folder: String,
    /// The `<name>` players see in the game; the package id when About.xml has none.
    pub name: String,
    pub dir: PathBuf,
    pub origin: Origin,
}

/// A folder the game would not list (or we cannot read), with the reason in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub dir: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// Resolved active mods, in load order. Includes official content.
    pub found: Vec<InstalledMod>,
    /// Active ids with no installed mod.
    pub missing: Vec<String>,
    /// Active ids with more than one folder in the same source: the game's choice
    /// depends on filesystem order, so the player must pick (DA1).
    pub ambiguous: Vec<(String, Vec<PathBuf>)>,
}

#[derive(Debug, Default)]
pub struct ModIndex {
    mods: Vec<InstalledMod>,
    by_id: HashMap<String, usize>,
    /// Same-source duplicates of `mods[i]`.
    alternates: HashMap<usize, Vec<PathBuf>>,
    skipped: Vec<Skipped>,
}

impl ModIndex {
    /// Scans official → local → workshop. A missing folder counts as empty.
    pub fn scan(official: &Path, local: &Path, workshop: Option<&Path>) -> Self {
        Self::scan_with_extra(official, local, workshop, None)
    }

    /// Like `scan`, with one more folder of Workshop-style mods (`<dir>/<id>/About/…`), such
    /// as the ones the steamcmd route downloads into this app's own folder.
    pub fn scan_with_extra(
        official: &Path,
        local: &Path,
        workshop: Option<&Path>,
        extra: Option<&Path>,
    ) -> Self {
        let mut index = Self::default();
        let sources = [
            (Origin::Official, Some(official)),
            (Origin::Local, Some(local)),
            (Origin::Workshop, workshop),
            (Origin::Workshop, extra),
        ];
        for (origin, dir) in sources {
            for mod_dir in dir.map(sorted_subdirs).unwrap_or_default() {
                match read_about(&mod_dir) {
                    Ok((id, name)) => index.add(InstalledMod {
                        name: name.unwrap_or_else(|| id.clone()),
                        package_id: id,
                        folder: folder_name(&mod_dir),
                        dir: mod_dir,
                        origin,
                    }),
                    Err(reason) => index.skipped.push(Skipped {
                        dir: mod_dir,
                        reason,
                    }),
                }
            }
        }
        index
    }

    pub fn mods(&self) -> &[InstalledMod] {
        &self.mods
    }

    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// The mod the game would use for an active id (ModsConfig.cs:123-139).
    pub fn find(&self, id: &str) -> Option<&InstalledMod> {
        self.find_index(id).map(|i| &self.mods[i])
    }

    pub fn resolve(&self, active: &[String]) -> Resolution {
        let mut out = Resolution::default();
        for id in active {
            match self.find_index(id) {
                None => out.missing.push(id.clone()),
                Some(i) => match self.alternates.get(&i) {
                    Some(alts) => {
                        let mut dirs = vec![self.mods[i].dir.clone()];
                        dirs.extend(alts.iter().cloned());
                        out.ambiguous.push((id.clone(), dirs));
                    }
                    None => out.found.push(self.mods[i].clone()),
                },
            }
        }
        out
    }

    fn find_index(&self, id: &str) -> Option<usize> {
        let id = id.trim().to_lowercase();
        if let Some(&i) = self.by_id.get(&id) {
            return Some(i);
        }
        // `_steam` left over after the local copy went away.
        if let Some(&i) = id
            .strip_suffix(STEAM_POSTFIX)
            .and_then(|p| self.by_id.get(p))
        {
            return Some(i);
        }
        // Very old configs list folder names.
        self.mods.iter().position(|m| m.folder.to_lowercase() == id)
    }

    /// ModLister.TryAddMod: a workshop/non-workshop clash postfixes the workshop copy;
    /// any other clash keeps the first and ignores the rest.
    fn add(&mut self, mut new: InstalledMod) {
        let Some(&i) = self.by_id.get(&new.package_id) else {
            self.by_id.insert(new.package_id.clone(), self.mods.len());
            self.mods.push(new);
            return;
        };
        let is_ws = |m: &InstalledMod| m.origin == Origin::Workshop;
        let old = &self.mods[i];
        if is_ws(old) != is_ws(&new) {
            if is_ws(&new) && !new.package_id.ends_with(STEAM_POSTFIX) {
                new.package_id.push_str(STEAM_POSTFIX);
                return self.add(new);
            }
            if is_ws(old) && !old.package_id.ends_with(STEAM_POSTFIX) {
                let mut moved = self.mods[i].package_id.clone();
                self.by_id.remove(&moved);
                moved.push_str(STEAM_POSTFIX);
                self.mods[i].package_id = moved.clone();
                self.by_id.insert(moved, i);
                return self.add(new);
            }
        }
        if old.origin == new.origin {
            self.alternates.entry(i).or_default().push(new.dir.clone());
        }
        self.skipped.push(Skipped {
            reason: format!(
                "same packageId `{}` as `{}`",
                new.package_id,
                old.dir.display()
            ),
            dir: new.dir,
        });
    }
}

/// Subfolders sorted by name. The game uses filesystem order; sorting keeps "first wins"
/// stable, and same-source clashes surface as `ambiguous` anyway.
fn sorted_subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Case-insensitive child lookup, as `GenFile.ResolveCaseInsensitiveFilePath`.
fn child_ignore_case(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// Lowercased root-level `<packageId>` (DA2: never a dependency's) and `<name>` from
/// `About/About.xml`.
fn read_about(mod_dir: &Path) -> Result<(String, Option<String>), String> {
    let about = child_ignore_case(mod_dir, "About")
        .and_then(|d| child_ignore_case(&d, "About.xml"))
        .ok_or_else(|| "no About/About.xml".to_owned())?;
    let text = read_xml_text(&about).map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("About.xml: {e}"))?;
    let field = |tag: &str| {
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let id = field("packageId")
        .map(|t| t.to_lowercase())
        .ok_or_else(|| "About.xml has no packageId".to_owned())?;
    Ok((id, field("name")))
}

/// A root-level text field of `About/About.xml` (for example `url`), trimmed; `None` when the
/// file or the field is missing.
pub(crate) fn about_field(mod_dir: &Path, tag: &str) -> Option<String> {
    let about =
        child_ignore_case(mod_dir, "About").and_then(|d| child_ignore_case(&d, "About.xml"))?;
    root_field(&about, tag)
}

/// `<version>` of `About/Manifest.xml`, for the mods that ship one.
pub(crate) fn manifest_version(mod_dir: &Path) -> Option<String> {
    let manifest =
        child_ignore_case(mod_dir, "About").and_then(|d| child_ignore_case(&d, "Manifest.xml"))?;
    root_field(&manifest, "version")
}

fn root_field(xml: &Path, tag: &str) -> Option<String> {
    let text = read_xml_text(xml).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tmp: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                tmp: tempfile::tempdir().expect("tempdir"),
            }
        }

        fn dir(&self, source: &str) -> PathBuf {
            self.tmp.path().join(source)
        }

        /// Writes `<source>/<folder>/About/About.xml` with the given body.
        fn raw(&self, source: &str, folder: &str, about: &str, body: &str) -> PathBuf {
            let dir = self.dir(source).join(folder);
            std::fs::create_dir_all(dir.join(about)).expect("about dir");
            let file = if about == "About" {
                "About.xml"
            } else {
                "about.xml"
            };
            std::fs::write(dir.join(about).join(file), body).expect("about");
            dir
        }

        fn add(&self, source: &str, folder: &str, id: &str) -> PathBuf {
            let body = format!(
                "<ModMetaData><name>{folder}</name><packageId>{id}</packageId></ModMetaData>"
            );
            self.raw(source, folder, "About", &body)
        }

        fn scan(&self) -> ModIndex {
            let ws = self.dir("ws");
            ModIndex::scan(&self.dir("Data"), &self.dir("Mods"), Some(&ws))
        }
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn resolves_local_and_workshop_and_official() {
        let f = Fixture::new();
        f.add("Data", "Core", "Ludeon.RimWorld");
        let local = f.add("Mods", "MyMod", "me.mymod");
        let ws = f.add("ws", "2009463077", "brrainz.harmony");
        let r = f
            .scan()
            .resolve(&ids(&["brrainz.harmony", "ludeon.rimworld", "me.mymod"]));
        let got: Vec<_> = r
            .found
            .iter()
            .map(|m| (m.package_id.as_str(), m.origin))
            .collect();
        assert_eq!(
            got,
            [
                ("brrainz.harmony", Origin::Workshop),
                ("ludeon.rimworld", Origin::Official),
                ("me.mymod", Origin::Local)
            ]
        );
        assert_eq!(r.found[0].dir, ws);
        assert_eq!(r.found[0].folder, "2009463077");
        assert_eq!(r.found[2].dir, local);
        assert!(r.missing.is_empty() && r.ambiguous.is_empty());
    }

    #[test]
    fn duplicate_local_and_workshop_gets_steam_postfix() {
        let f = Fixture::new();
        let local = f.add("Mods", "Harmony", "brrainz.harmony");
        let ws = f.add("ws", "2009463077", "brrainz.harmony");
        let index = f.scan();
        assert_eq!(index.find("brrainz.harmony").map(|m| &m.dir), Some(&local));
        assert_eq!(
            index.find("brrainz.harmony_steam").map(|m| &m.dir),
            Some(&ws)
        );
        assert!(index.skipped().is_empty());
    }

    #[test]
    fn steam_suffixed_id_resolves_workshop_copy() {
        let f = Fixture::new();
        f.add("Mods", "Harmony", "brrainz.harmony");
        f.add("ws", "2009463077", "brrainz.harmony");
        let r = f.scan().resolve(&ids(&["brrainz.harmony_steam"]));
        assert_eq!(r.found.len(), 1);
        assert_eq!(r.found[0].origin, Origin::Workshop);
        assert_eq!(r.found[0].package_id, "brrainz.harmony_steam");
    }

    #[test]
    fn duplicate_within_one_source_is_ambiguous() {
        let f = Fixture::new();
        let a = f.add("Mods", "Mod", "me.mod");
        let b = f.add("Mods", "Mod - old", "me.mod");
        let index = f.scan();
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(index.skipped()[0].dir, b);
        let r = index.resolve(&ids(&["me.mod"]));
        assert!(r.found.is_empty());
        assert_eq!(r.ambiguous, [("me.mod".to_owned(), vec![a, b])]);
    }

    #[test]
    fn official_and_local_clash_keeps_official() {
        let f = Fixture::new();
        let core = f.add("Data", "Core", "ludeon.rimworld");
        f.add("Mods", "FakeCore", "ludeon.rimworld");
        let r = f.scan().resolve(&ids(&["ludeon.rimworld"]));
        assert_eq!(r.found[0].dir, core);
        assert!(r.ambiguous.is_empty());
    }

    #[test]
    fn names_come_from_about_xml_else_the_package_id() {
        let f = Fixture::new();
        f.add("Mods", "Harmony", "brrainz.harmony");
        f.raw(
            "Mods",
            "Nameless",
            "About",
            "<ModMetaData><modDependencies><li><name>Dep</name></li></modDependencies>\
             <packageId>me.nameless</packageId></ModMetaData>",
        );
        let index = f.scan();
        let name = |id| index.find(id).map(|m| m.name.as_str());
        assert_eq!(name("brrainz.harmony"), Some("Harmony"));
        assert_eq!(
            name("me.nameless"),
            Some("me.nameless"),
            "a dependency's name is not it"
        );
    }

    #[test]
    fn package_id_ignores_dependency_entries() {
        let f = Fixture::new();
        f.raw(
            "Mods",
            "Dependent",
            "About",
            "<ModMetaData><modDependencies><li><packageId>brrainz.harmony</packageId></li>\
             </modDependencies><packageId>me.dependent</packageId></ModMetaData>",
        );
        let index = f.scan();
        assert!(index.find("brrainz.harmony").is_none());
        assert_eq!(
            index.find("me.dependent").map(|m| m.folder.as_str()),
            Some("Dependent")
        );
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let f = Fixture::new();
        f.add("Mods", "M", "Author.MixedCase");
        let index = f.scan();
        assert_eq!(index.mods()[0].package_id, "author.mixedcase");
        assert!(index.find("AUTHOR.MixedCase").is_some());
    }

    #[test]
    fn about_xml_path_case_insensitive() {
        let f = Fixture::new();
        f.raw(
            "Mods",
            "Lower",
            "about",
            "<ModMetaData><packageId>me.lower</packageId></ModMetaData>",
        );
        assert!(f.scan().find("me.lower").is_some());
    }

    #[test]
    fn folder_name_fallback() {
        let f = Fixture::new();
        f.add("Mods", "OldStyleMod", "me.oldstyle");
        let r = f.scan().resolve(&ids(&["oldstylemod"]));
        assert_eq!(r.found[0].package_id, "me.oldstyle");
    }

    #[test]
    fn steam_postfix_strip_fallback() {
        let f = Fixture::new();
        let ws = f.add("ws", "2009463077", "brrainz.harmony");
        let r = f.scan().resolve(&ids(&["brrainz.harmony_steam"]));
        assert_eq!(r.found[0].dir, ws);
    }

    #[test]
    fn missing_package_id_is_skipped() {
        let f = Fixture::new();
        f.raw(
            "Mods",
            "Ancient",
            "About",
            "<ModMetaData><name>Ancient</name></ModMetaData>",
        );
        f.raw(
            "Mods",
            "Blank",
            "About",
            "<ModMetaData><packageId>  </packageId></ModMetaData>",
        );
        let index = f.scan();
        assert!(index.mods().is_empty());
        assert_eq!(index.skipped().len(), 2);
        assert!(index.skipped()[0].reason.contains("no packageId"));
        let r = index.resolve(&ids(&["me.gone"]));
        assert_eq!(r.missing, ["me.gone"]);
    }

    #[test]
    fn malformed_about_is_skipped_not_fatal() {
        let f = Fixture::new();
        f.raw("Mods", "Broken", "About", "<ModMetaData><packageId>x");
        std::fs::create_dir_all(f.dir("Mods").join("NoAbout")).expect("dir");
        let latin1 = f.dir("Mods").join("Latin1");
        std::fs::create_dir_all(latin1.join("About")).expect("dir");
        std::fs::write(
            latin1.join("About/About.xml"),
            b"<ModMetaData>\xE9</ModMetaData>",
        )
        .expect("w");
        f.add("Mods", "Good", "me.good");
        let index = f.scan();
        assert_eq!(index.skipped().len(), 3);
        assert!(index.find("me.good").is_some());
    }

    #[test]
    fn missing_dirs_are_empty() {
        let f = Fixture::new();
        let index = ModIndex::scan(&f.dir("none"), &f.dir("none2"), None);
        assert!(index.mods().is_empty() && index.skipped().is_empty());
    }

    /// Manual smoke against this machine's real install: `cargo test -p rwpm-core
    /// real_install -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the real RimWorld install and workshop"]
    fn real_install_resolves_live_modsconfig() {
        let game = crate::discovery::discover(None).expect("discover");
        let ws = game.steam.as_ref().map(|s| s.workshop_dir.clone());
        let index = ModIndex::scan(&game.data_dir(), &game.mods_dir(), ws.as_deref());
        let config = crate::paths::default_game_save_dir()
            .expect("save dir")
            .join("Config/ModsConfig.xml");
        let active = super::super::read_active_mods(&config).expect("ModsConfig");
        let r = index.resolve(&active);
        println!(
            "installed={} skipped={} active={} found={} missing={:?} ambiguous={:?}",
            index.mods().len(),
            index.skipped().len(),
            active.len(),
            r.found.len(),
            r.missing,
            r.ambiguous
        );
        for s in index.skipped() {
            println!("skipped {}: {}", s.dir.display(), s.reason);
        }
    }
}
