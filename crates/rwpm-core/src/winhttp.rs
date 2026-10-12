//! Windows OFFLINE loader: UnityDoorstop's `winhttp.dll` beside `RimWorldWin64.exe`
//! (V6, V7, V8, V23). It stays inert unless the game is started with Doorstop's
//! arguments, so LIVE launches are unaffected.
//!
//! A `winhttp.dll` is ours only when its SHA-256 matches the one recorded when we
//! installed it; anything else is foreign and never overwritten or removed (V8). A
//! foreign UnityDoorstop 4.x (e.g. pardeike's Rimworld-Doorstop setup) is reused: its
//! `doorstop_config.ini` target is chain-loaded by our payload after the redirect.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::consent::{Consent, Scope};

pub const DLL: &str = "winhttp.dll";
const INI: &str = "doorstop_config.ini";
const INSTALLS: &str = "installs.toml";
/// UnityDoorstop's version resource ProductName (also used by BepInEx's builds).
const DOORSTOP_PRODUCT: &str = "NeighTools";
/// `VS_FIXEDFILEINFO.dwSignature`.
const FIXED_FILE_INFO: u32 = 0xFEEF_04BD;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DllState {
    /// No `winhttp.dll`, and none installed by us.
    Absent,
    /// Ours, unchanged since we put it there.
    Ours,
    /// We installed one but it is gone (e.g. removed by a game update): repairable.
    Missing,
    Foreign(Foreign),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreign {
    pub product: Option<String>,
    pub version: Option<[u16; 4]>,
    /// A UnityDoorstop 4.x: takes our launch arguments, so we can reuse it.
    pub compatible: bool,
    /// The assembly its `doorstop_config.ini` loads; our payload starts it afterwards.
    pub chain_target: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum WinhttpError {
    #[error("cannot {op} `{}`: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read `{}`: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("another tool's winhttp.dll is in the game folder; it is left as it is")]
    Foreign,
    #[error("no recorded permission to change `{}`", .0.display())]
    NoConsent(PathBuf),
    #[error(
        "OFFLINE profiles need the game-folder loader; open RimWorld Profile Manager to set it up"
    )]
    NotInstalled,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Installs {
    #[serde(default, rename = "winhttp")]
    entries: Vec<Install>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    path: PathBuf,
    sha256: String,
}

/// What is beside `RimWorldWin64.exe` in `game_dir`, judged against our install record
/// in `app_dir`.
pub fn inspect(game_dir: &Path, app_dir: &Path) -> Result<DllState, WinhttpError> {
    let dll = game_dir.join(DLL);
    let recorded = recorded(app_dir, &dll)?;
    let bytes = match std::fs::read(&dll) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(if recorded.is_some() {
                DllState::Missing
            } else {
                DllState::Absent
            });
        }
        Err(source) => return Err(io("read", &dll)(source)),
    };
    if recorded.as_deref() == Some(sha256(&bytes).as_str()) {
        return Ok(DllState::Ours);
    }
    let (product, version) = version_info(&bytes);
    let compatible =
        product.as_deref() == Some(DOORSTOP_PRODUCT) && version.is_some_and(|v| v[0] == 4);
    Ok(DllState::Foreign(Foreign {
        chain_target: compatible.then(|| chain_target(game_dir)).flatten(),
        product,
        version,
        compatible,
    }))
}

/// Installs (or repairs, or upgrades our own) `winhttp.dll` from `vendor_dll`. Refuses a
/// foreign one (V8) and needs consent for this exact path (V7).
pub fn install(
    game_dir: &Path,
    app_dir: &Path,
    vendor_dll: &Path,
    consent: &Consent,
) -> Result<(), WinhttpError> {
    let dll = game_dir.join(DLL);
    if !consent.covers(Scope::WinhttpDll, &dll) {
        return Err(WinhttpError::NoConsent(dll));
    }
    if let DllState::Foreign(_) = inspect(game_dir, app_dir)? {
        return Err(WinhttpError::Foreign);
    }
    let bytes = std::fs::read(vendor_dll).map_err(io("read", vendor_dll))?;
    let tmp = game_dir.join(".rwpm-winhttp.tmp");
    std::fs::write(&tmp, &bytes).map_err(io("write", &tmp))?;
    std::fs::rename(&tmp, &dll).map_err(io("replace", &dll))?;
    set_record(app_dir, &dll, Some(sha256(&bytes)))
}

/// Before a Windows OFFLINE launch: `Ok(chain target)` when the game can start with our
/// payload, using ours or a compatible foreign Doorstop.
pub fn launch_check(game_dir: &Path, app_dir: &Path) -> Result<Option<PathBuf>, WinhttpError> {
    match inspect(game_dir, app_dir)? {
        DllState::Ours => Ok(None),
        DllState::Foreign(f) if f.compatible => Ok(f.chain_target),
        DllState::Foreign(_) => Err(WinhttpError::Foreign),
        DllState::Absent | DllState::Missing => Err(WinhttpError::NotInstalled),
    }
}

/// "Remove from game folder": deletes our `winhttp.dll` only if unchanged since install.
pub fn remove(game_dir: &Path, app_dir: &Path) -> Result<(), WinhttpError> {
    let dll = game_dir.join(DLL);
    match inspect(game_dir, app_dir)? {
        DllState::Foreign(_) => Err(WinhttpError::Foreign),
        DllState::Absent => Ok(()),
        DllState::Missing => set_record(app_dir, &dll, None),
        DllState::Ours => {
            std::fs::remove_file(&dll).map_err(io("remove", &dll))?;
            set_record(app_dir, &dll, None)
        }
    }
}

/// `target_assembly` from `doorstop_config.ini` (relative paths: from the game folder).
fn chain_target(game_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(game_dir.join(INI)).ok()?;
    let mut enabled = true;
    let mut target = None;
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match key.as_str() {
            "enabled" => enabled = value.eq_ignore_ascii_case("true"),
            "target_assembly" | "targetassembly" if !value.is_empty() => {
                target = Some(game_dir.join(value))
            }
            _ => {}
        }
    }
    target.filter(|_| enabled)
}

/// ProductName and FileVersion from a PE's version resource, without Windows APIs.
fn version_info(bytes: &[u8]) -> (Option<String>, Option<[u16; 4]>) {
    let version = find(bytes, &FIXED_FILE_INFO.to_le_bytes()).and_then(|i| {
        let ms = u32::from_le_bytes(bytes.get(i + 8..i + 12)?.try_into().ok()?);
        let ls = u32::from_le_bytes(bytes.get(i + 12..i + 16)?.try_into().ok()?);
        Some([(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16])
    });
    let key: Vec<u8> = "ProductName"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let product = find(bytes, &key).map(|i| {
        // Key, NUL, padding to 4 bytes, then the UTF-16 value up to its NUL.
        let mut at = i + key.len() + 2;
        at += (4 - at % 4) % 4;
        let units: Vec<u16> = bytes[at..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .take(64)
            .collect();
        String::from_utf16_lossy(&units)
    });
    (product, version)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn recorded(app_dir: &Path, dll: &Path) -> Result<Option<String>, WinhttpError> {
    Ok(read_installs(app_dir)?
        .entries
        .into_iter()
        .find(|e| e.path == dll)
        .map(|e| e.sha256))
}

fn set_record(app_dir: &Path, dll: &Path, sha: Option<String>) -> Result<(), WinhttpError> {
    let mut installs = read_installs(app_dir)?;
    installs.entries.retain(|e| e.path != dll);
    if let Some(sha256) = sha {
        installs.entries.push(Install {
            path: dll.to_path_buf(),
            sha256,
        });
    }
    let path = app_dir.join(INSTALLS);
    let text = toml::to_string_pretty(&installs).map_err(|e| WinhttpError::Parse {
        path: path.clone(),
        message: e.to_string(),
    })?;
    std::fs::create_dir_all(app_dir).map_err(io("create", app_dir))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(io("write", &tmp))?;
    std::fs::rename(&tmp, &path).map_err(io("replace", &path))
}

fn read_installs(app_dir: &Path) -> Result<Installs, WinhttpError> {
    let path = app_dir.join(INSTALLS);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| WinhttpError::Parse {
            path,
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Installs::default()),
        Err(source) => Err(io("read", &path)(source)),
    }
}

fn io(op: &'static str, path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> WinhttpError {
    let path = path.into();
    move |source| WinhttpError::Io { op, path, source }
}

#[cfg(test)]
mod tests;
