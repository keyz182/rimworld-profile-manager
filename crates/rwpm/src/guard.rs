//! `rwpm run`'s OFFLINE guard (V5, I.cmd): asks the player what to do about active mods
//! that are not frozen in the profile, before the game starts.
//!
//! Steam shows no console, so outcomes go to `<profile>/rwpm.log`. If no dialog can be
//! shown, the game launches anyway and the log says why (never blocks play silently).

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use rwpm_core::discovery::{self, GameInstall};
use rwpm_core::launch;
use rwpm_core::profile::guard::{self, Fix, GuardReport};
use rwpm_core::profile::offline::now_datetime;
use rwpm_core::profile::{OsTrash, ProcessProbe, Profile, ProfileStore};

/// Test and headless override: `add`, `off`, `launch`, `cancel` or `unavailable`.
pub const CHOICE_ENV: &str = "RWPM_GUARD_CHOICE";
const LOG: &str = "rwpm.log";
const TITLE: &str = "RimWorld Profile Manager";
const ADD: &str = "Add to profile";
const TURN_OFF: &str = "Turn off";
const LAUNCH: &str = "Launch anyway";
const CANCEL: &str = "Cancel";
/// Carries the dialog text to the Windows dialog (no quoting through a command line).
const TEXT_ENV: &str = "RWPM_GUARD_TEXT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Launch,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Add,
    TurnOff,
    LaunchAnyway,
    Cancel,
}

/// Runs the guard for an OFFLINE profile. Errors never block play: they are logged and
/// the game launches, except a failed fix, which is shown and cancels the launch.
pub fn run(store: &ProfileStore<OsTrash>, profile: &Profile, command: &[OsString]) -> Outcome {
    let log = |msg: &str| append_log(profile, msg);
    let report = match find_game(command)
        .and_then(|game| guard::check_offline(store, &profile.id, &game).map_err(|e| e.to_string()))
    {
        Ok(report) => report,
        Err(e) => {
            log(&format!("guard skipped: {e}"));
            return Outcome::Launch;
        }
    };
    if report.is_clear() {
        return Outcome::Launch;
    }
    let ids: Vec<String> = report
        .unfrozen
        .iter()
        .map(|u| u.active_id.clone())
        .collect();
    let choice = match ask(&message(profile, &report)) {
        Ok(choice) => choice,
        Err(why) => {
            log(&format!(
                "guard: dialog unavailable ({why}); launched with unfrozen mods: {}",
                ids.join(", ")
            ));
            return Outcome::Launch;
        }
    };
    log(&format!(
        "guard: unfrozen mods {}; player chose {choice:?}",
        ids.join(", ")
    ));
    let fixed = match choice {
        Choice::Cancel => return Outcome::Cancel,
        Choice::LaunchAnyway => return Outcome::Launch,
        Choice::Add => guard::add_to_profile(
            store,
            &profile.id,
            &report.unfrozen,
            &ProcessProbe,
            &mut |_| {},
        )
        .map(|_| ()),
        Choice::TurnOff => guard::turn_off(store, &profile.id, &ids),
    };
    match fixed {
        Ok(()) => Outcome::Launch,
        Err(e) => {
            log(&format!("guard: {choice:?} failed: {e}"));
            show_error(&format!(
                "Could not change profile “{}”:\n{e}\n\nThe game was not started.",
                profile.name
            ));
            Outcome::Cancel
        }
    }
}

/// The game folder holding the launched program, else the Steam install.
fn find_game(command: &[OsString]) -> Result<GameInstall, String> {
    let program = launch::game_program(command).map(Path::new);
    if let Some(dir) = program.and_then(Path::parent)
        && let Ok(game) = discovery::from_game_dir(dir)
    {
        return Ok(game);
    }
    discovery::discover(None).map_err(|e| format!("cannot find RimWorld: {e}"))
}

fn message(profile: &Profile, report: &GuardReport) -> String {
    let mut text = format!(
        "Profile “{}” is OFFLINE, but these mods are turned on and not frozen in it. \
         They would load from Steam and can change:\n",
        profile.name
    );
    for u in &report.unfrozen {
        match &u.fix {
            Fix::Copy(c) => text.push_str(&format!("\n  • {} (folder {})", u.active_id, c.folder)),
            Fix::UseFrozen(_) => text.push_str(&format!(
                "\n  • {} (Steam copy of a frozen mod)",
                u.active_id
            )),
        }
    }
    if !report.missing.is_empty() {
        text.push_str(&format!(
            "\n\nNot installed (the game turns these off): {}",
            report.missing.join(", ")
        ));
    }
    text.push_str(&format!(
        "\n\n{ADD}: freeze them in this profile now.\n{TURN_OFF}: remove them from this profile's mod list.\n\
         {LAUNCH}: play with the Steam versions this time."
    ));
    text
}

fn ask(text: &str) -> Result<Choice, String> {
    match std::env::var(CHOICE_ENV).ok().as_deref() {
        Some("add") => return Ok(Choice::Add),
        Some("off") => return Ok(Choice::TurnOff),
        Some("launch") => return Ok(Choice::LaunchAnyway),
        Some("cancel") => return Ok(Choice::Cancel),
        Some("unavailable") => return Err(format!("{CHOICE_ENV}=unavailable")),
        _ => {}
    }
    if cfg!(windows) {
        ask_windows(text)
    } else if cfg!(target_os = "macos") {
        ask_osascript(text)
    } else {
        ask_zenity(text)
    }
}

/// Windows (T21): a small WinForms dialog run by the Windows PowerShell every Windows 10
/// and 11 has. Each button exits with its own code; closing the window is Cancel. Any
/// other outcome (PowerShell missing, blocked by policy) means no dialog was possible.
fn ask_windows(text: &str) -> Result<Choice, String> {
    let status = powershell(&windows_dialog_script())
        .env(TEXT_ENV, text)
        .status()
        .map_err(|e| format!("cannot run PowerShell: {e}"))?;
    choice_from_windows_exit(status.code())
}

fn choice_from_windows_exit(code: Option<i32>) -> Result<Choice, String> {
    match code {
        Some(10) => Ok(Choice::Add),
        Some(11) => Ok(Choice::TurnOff),
        Some(12) => Ok(Choice::LaunchAnyway),
        Some(13) => Ok(Choice::Cancel),
        other => Err(format!("Windows dialog exit {other:?}")),
    }
}

/// The dialog; its text comes from `RWPM_GUARD_TEXT`.
fn windows_dialog_script() -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
try {{
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  [System.Windows.Forms.Application]::EnableVisualStyles()
  $f = New-Object System.Windows.Forms.Form
  $f.Text = '{TITLE}'
  $f.StartPosition = 'CenterScreen'
  $f.FormBorderStyle = 'FixedDialog'
  $f.MaximizeBox = $false
  $f.MinimizeBox = $false
  $f.TopMost = $true
  $f.AutoSize = $true
  $f.AutoSizeMode = 'GrowAndShrink'
  $f.Padding = New-Object System.Windows.Forms.Padding 12
  $col = New-Object System.Windows.Forms.FlowLayoutPanel
  $col.FlowDirection = 'TopDown'
  $col.AutoSize = $true
  $label = New-Object System.Windows.Forms.Label
  $label.Text = $env:{TEXT_ENV}
  $label.AutoSize = $true
  $label.MaximumSize = New-Object System.Drawing.Size 560, 0
  $row = New-Object System.Windows.Forms.FlowLayoutPanel
  $row.AutoSize = $true
  $row.Margin = New-Object System.Windows.Forms.Padding 0, 12, 0, 0
  function B($t, $r) {{ $b = New-Object System.Windows.Forms.Button; $b.Text = $t; $b.AutoSize = $true; $b.DialogResult = $r; $row.Controls.Add($b); $b }}
  $add = B '{ADD}' 'Yes'
  $null = B '{TURN_OFF}' 'No'
  $null = B '{LAUNCH}' 'Retry'
  $cancel = B '{CANCEL}' 'Cancel'
  $col.Controls.Add($label)
  $col.Controls.Add($row)
  $f.Controls.Add($col)
  $f.AcceptButton = $add
  $f.CancelButton = $cancel
  switch ($f.ShowDialog()) {{ 'Yes' {{ exit 10 }} 'No' {{ exit 11 }} 'Retry' {{ exit 12 }} default {{ exit 13 }} }}
}} catch {{ exit 20 }}"#
    )
}

/// `powershell.exe` running `script` with no console window; the script travels
/// base64-encoded (-EncodedCommand), so no quoting can break it.
fn powershell(script: &str) -> Command {
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
    ])
    .arg(encode_utf16_base64(script))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Base64 of UTF-16LE text, as PowerShell's -EncodedCommand expects.
fn encode_utf16_base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// macOS (T41, V45): `choose from list` rather than `display dialog`, which caps at 3
/// buttons — this needs 4 outcomes (Add/Turn off/Launch anyway/Cancel). The script's own
/// built-in Cancel button returns `false`, distinct from the three named items. Text
/// travels via `system attribute`, which reads the process environment — never through
/// `-e` string interpolation, which would make `text` (player-controlled: mod names, R33)
/// an AppleScript injection hazard. The script is piped to `osascript -`'s stdin, so
/// nothing but Rust's own constant labels ever appears on its command line.
fn ask_osascript(text: &str) -> Result<Choice, String> {
    let mut child = Command::new("osascript")
        .arg("-")
        .env(TEXT_ENV, text)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run osascript: {e}"))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(macos_dialog_script().as_bytes())
        .map_err(|e| format!("cannot write osascript stdin: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("osascript failed: {e}"))?;
    choice_from_osascript_output(out.status.success(), &out.stdout, &out.stderr)
}

fn choice_from_osascript_output(ok: bool, stdout: &[u8], stderr: &[u8]) -> Result<Choice, String> {
    if !ok {
        return Err(format!(
            "osascript exit failed: {}",
            String::from_utf8_lossy(stderr).trim()
        ));
    }
    match String::from_utf8_lossy(stdout).trim() {
        s if s == ADD => Ok(Choice::Add),
        s if s == TURN_OFF => Ok(Choice::TurnOff),
        s if s == LAUNCH => Ok(Choice::LaunchAnyway),
        "cancel" => Ok(Choice::Cancel),
        other => Err(format!("osascript output: {other}")),
    }
}

/// `system attribute` reads `RWPM_GUARD_TEXT` from the process env rather than having
/// the text spliced into the script source. A GUI-less session (headless CI, no Finder
/// login) makes `choose from list` raise error -1743 ("not allowed to send keystrokes" /
/// user-interaction-not-allowed); the catch turns that into `"error"`, which
/// `choice_from_osascript_output` treats as a failure — same "dialog unavailable →
/// launch + log" path as Windows/Linux.
fn macos_dialog_script() -> String {
    format!(
        r#"try
  set guardText to system attribute "{TEXT_ENV}"
  set theChoice to choose from list {{"{ADD}", "{TURN_OFF}", "{LAUNCH}"}} with title "{TITLE}" with prompt guardText without multiple selections allowed
  if theChoice is false then
    return "cancel"
  else
    return item 1 of theChoice
  end if
on error
  return "error"
end try"#
    )
}

/// zenity: exit 0 = OK button; exit 1 = an extra button (its label on stdout), Cancel,
/// or the window closed; "Failed to open display" on stderr = no dialog possible.
fn ask_zenity(text: &str) -> Result<Choice, String> {
    let out = Command::new("zenity")
        .args([
            "--question",
            "--no-markup",
            "--width=520",
            "--title",
            TITLE,
            "--text",
            text,
        ])
        .args(["--ok-label", ADD, "--cancel-label", "Cancel"])
        .args(["--extra-button", TURN_OFF, "--extra-button", LAUNCH])
        .output()
        .map_err(|e| format!("cannot run zenity: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    match (out.status.code(), stdout.trim()) {
        (Some(0), _) => Ok(Choice::Add),
        (Some(1), TURN_OFF) => Ok(Choice::TurnOff),
        (Some(1), LAUNCH) => Ok(Choice::LaunchAnyway),
        (Some(1), "") if !stderr.contains("open display") => Ok(Choice::Cancel),
        (code, _) => Err(format!("zenity exit {code:?}: {}", stderr.trim())),
    }
}

/// Best effort: the failure is in the log either way.
fn show_error(text: &str) {
    if std::env::var_os(CHOICE_ENV).is_some() {
        return;
    }
    if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             [void][System.Windows.Forms.MessageBox]::Show($env:{TEXT_ENV}, '{TITLE}', 'OK', 'Error')"
        );
        let _ = powershell(&script).env(TEXT_ENV, text).status();
    } else if cfg!(target_os = "macos") {
        let script = format!(
            r#"display alert "{TITLE}" message (system attribute "{TEXT_ENV}") as critical"#
        );
        if let Ok(mut child) = Command::new("osascript")
            .arg("-")
            .env(TEXT_ENV, text)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(script.as_bytes());
            }
            let _ = child.wait();
        }
    } else {
        let _ = Command::new("zenity")
            .args(["--error", "--no-markup", "--title", TITLE, "--text", text])
            .status();
    }
}

pub(crate) fn append_log(profile: &Profile, msg: &str) {
    let Some(path) = profile.mods_dir().map(|m| m.with_file_name(LOG)) else {
        return;
    };
    let line = format!("{} {msg}\n", now_datetime());
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = written {
        eprintln!("rwpm run: cannot write `{}`: {e}; {msg}", path.display());
    }
    eprintln!("rwpm run: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_dialog_exit_codes_map_to_choices() {
        assert_eq!(choice_from_windows_exit(Some(10)), Ok(Choice::Add));
        assert_eq!(choice_from_windows_exit(Some(11)), Ok(Choice::TurnOff));
        assert_eq!(choice_from_windows_exit(Some(12)), Ok(Choice::LaunchAnyway));
        assert_eq!(choice_from_windows_exit(Some(13)), Ok(Choice::Cancel));
        // PowerShell failed (20), was blocked (1) or killed (None): no dialog was shown.
        for other in [Some(20), Some(1), Some(0), None] {
            assert!(choice_from_windows_exit(other).is_err(), "{other:?}");
        }
    }

    #[test]
    fn windows_dialog_offers_the_four_choices() {
        let script = windows_dialog_script();
        for label in [ADD, TURN_OFF, LAUNCH, CANCEL, TITLE] {
            assert!(script.contains(&format!("'{label}'")), "{label}");
        }
        assert!(!script.contains('"'), "no double quotes to escape");
        assert!(script.contains(&format!("$env:{TEXT_ENV}")));
    }

    #[test]
    fn osascript_output_maps_to_choices() {
        assert_eq!(
            choice_from_osascript_output(true, ADD.as_bytes(), b""),
            Ok(Choice::Add)
        );
        assert_eq!(
            choice_from_osascript_output(true, TURN_OFF.as_bytes(), b""),
            Ok(Choice::TurnOff)
        );
        assert_eq!(
            choice_from_osascript_output(true, LAUNCH.as_bytes(), b""),
            Ok(Choice::LaunchAnyway)
        );
        assert_eq!(
            choice_from_osascript_output(true, b"cancel", b""),
            Ok(Choice::Cancel)
        );
        // The script's own catch block (headless/-1743) or a hard osascript failure:
        // either way, no dialog was shown.
        assert!(choice_from_osascript_output(true, b"error", b"").is_err());
        assert!(choice_from_osascript_output(false, b"", b"not allowed").is_err());
    }

    #[test]
    fn osascript_script_offers_the_three_list_items_and_title() {
        let script = macos_dialog_script();
        for label in [ADD, TURN_OFF, LAUNCH, TITLE] {
            assert!(script.contains(label), "{label}");
        }
        assert!(script.contains(&format!("system attribute \"{TEXT_ENV}\"")));
        assert!(script.contains("choose from list"));
    }

    #[test]
    fn utf16_base64_matches_powershell_encoding() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('exit 13'))
        assert_eq!(encode_utf16_base64("exit 13"), "ZQB4AGkAdAAgADEAMwA=");
        assert_eq!(encode_utf16_base64(""), "");
        assert_eq!(encode_utf16_base64("é"), "6QA=");
    }
}
