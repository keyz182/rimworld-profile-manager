# Scenarios, dot-sourced by e2e.ps1.

function Sc-basic {
    Install-Dll
    $p = New-Profile 'basic' @()
    Start-Run 'basic' @{ RWPM_SPIKE_DIR = $p }
    if (-not (Wait-Run 'basic' 300)) { Fail "basic: game did not quit"; return }
    if ((Probe-Lines $p) -ne 1) { Fail "basic: probe lines=$(Probe-Lines $p) rwpm.log=$(Get-Content (Join-Path $p 'rwpm.log') -Raw -ErrorAction SilentlyContinue)"; return }
    Assert-Redirected $p 1
    Check ((Probe-Field $p 'SteamAppId' 1) -eq '294100') "SteamAppId exported" "SteamAppId missing"
    Check (Test-Path (Join-Path $p 'Player.log')) "Player.log in profile" "no profile Player.log"
    Assert-Waited 'basic'
}

# The in-game Restart child is redirected too (payload clears DOORSTOP_DISABLE).
function Sc-restart {
    Install-Dll
    $p = New-Profile 'restart' @()
    New-Item -ItemType File (Join-Path $p 'restart.flag') | Out-Null
    Start-Run 'restart' @{ RWPM_SPIKE_DIR = $p }
    if (-not (Wait-Run 'restart' 420)) { Fail "restart: game did not quit (lines=$(Probe-Lines $p))"; return }
    if ((Probe-Lines $p) -ne 2) { Fail "restart: probe lines=$(Probe-Lines $p)"; return }
    Check ((Probe-Field $p 'pid' 1) -ne (Probe-Field $p 'pid' 2)) "two processes" "same pid twice"
    Assert-Redirected $p 1
    Assert-Redirected $p 2
    Assert-Waited 'restart'
}

# An active workshop mod not frozen in the profile; "Add to profile" (env choice)
# freezes it under its own folder name and the game loads it from the profile.
function Sc-guard {
    if (-not (Test-Path (Join-Path $Workshop $HarmonyWs))) { Inconclusive "Harmony not subscribed on this VM"; return }
    Install-Dll
    $p = New-Profile 'guard' @('brrainz.harmony')
    Start-Run 'guard' @{ RWPM_SPIKE_DIR = $p; RWPM_GUARD_CHOICE = 'add' }
    if (-not (Wait-Run 'guard' 300)) { Fail "guard: game did not quit"; return }
    Check (Log-Has (Join-Path $p 'rwpm.log') 'player chose Add') "guard asked, player added" "guard log missing"
    Check (Test-Path (Join-Path $p "Mods\$HarmonyWs\About\About.xml")) "Harmony frozen as Mods\$HarmonyWs" "Harmony not copied under its folder name"
    if ((Probe-Lines $p) -ne 1) { Fail "guard: probe lines=$(Probe-Lines $p)"; return }
    $from = Installed-Path $p 1 'brrainz.harmony'
    Check ($from -eq (Join-Path $p "Mods\$HarmonyWs")) "Harmony loads from the profile" "Harmony from $from"
    Assert-Waited 'guard'
}

# The real Windows guard dialog (Steam launch, no RWPM_GUARD_CHOICE). clicker.ps1
# presses "Add to profile" in the desktop session; Harmony must be frozen and loaded.
function Sc-guard_dialog_add {
    if (-not (Test-Path (Join-Path $Workshop $HarmonyWs))) { Inconclusive "Harmony not subscribed"; return }
    Install-Dll
    $p = New-Profile 'dialog-add' @('brrainz.harmony')
    Start-Run 'dialog-add' @{ RWPM_SPIKE_DIR = $p } 'Add to profile'
    if (-not (Wait-Run 'dialog-add' 300)) { Fail "add: game did not quit" }
    $click = Get-Content (Join-Path (Run-Dir 'dialog-add') 'click.txt') -Raw -ErrorAction SilentlyContinue
    Check ($click -match '^clicked') "dialog shown and answered" "clicker: $click"
    Check (Log-Has (Join-Path $p 'rwpm.log') 'player chose Add') "choice reached rwpm" "no Add in rwpm.log"
    Check (Test-Path (Join-Path $p "Mods\$HarmonyWs")) "Harmony frozen" "Harmony not copied"
    if ((Probe-Lines $p) -eq 1) {
        $from = Installed-Path $p 1 'brrainz.harmony'
        Check ($from -eq (Join-Path $p "Mods\$HarmonyWs")) "Harmony loads from the profile" "Harmony from $from"
    } else { Fail "add: probe lines=$(Probe-Lines $p)" }
}

# Cancel in the dialog stops the launch; rwpm exits 0 without starting the game.
function Sc-guard_dialog_cancel {
    Install-Dll
    $p = New-Profile 'dialog-cancel' @('brrainz.harmony')
    Start-Run 'dialog-cancel' @{ RWPM_SPIKE_DIR = $p } 'Cancel'
    $null = Wait-Run 'dialog-cancel' 120
    $click = Get-Content (Join-Path (Run-Dir 'dialog-cancel') 'click.txt') -Raw -ErrorAction SilentlyContinue
    Check ($click -match '^clicked') "dialog shown and answered" "clicker: $click"
    Check ((Exit-Code 'dialog-cancel') -eq '0') "cancel: rwpm exit 0" "cancel: exit $(Exit-Code 'dialog-cancel')"
    Check ((Probe-Lines $p) -eq 0 -and -not (Test-Path (Join-Path $p 'Player.log'))) "game not started" "game started after Cancel"
    Check (-not (Test-Path (Join-Path $p "Mods\$HarmonyWs"))) "nothing copied" "copied after Cancel"
}

# A compatible Doorstop we did not record (someone else's) is reused, not replaced.
function Sc-foreign_reuse {
    Install-Dll $false
    $before = Sha $GameDll
    $p = New-Profile 'foreign-reuse' @()
    Start-Run 'foreign-reuse' @{ RWPM_SPIKE_DIR = $p }
    if (-not (Wait-Run 'foreign-reuse' 300)) { Fail "reuse: game did not quit"; return }
    if ((Probe-Lines $p) -ne 1) { Fail "reuse: probe lines=$(Probe-Lines $p)"; return }
    Assert-Redirected $p 1
    Check ((Sha $GameDll) -eq $before) "reused winhttp.dll unchanged" "winhttp.dll changed"
    Install-Dll
}

# An incompatible foreign winhttp.dll blocks OFFLINE launches and is never touched.
function Sc-foreign_block {
    Remove-Dll
    [IO.File]::WriteAllBytes($GameDll, [Text.Encoding]::ASCII.GetBytes('rwpm-e2e dummy foreign winhttp.dll'))
    $before = Sha $GameDll
    try {
        $p = New-Profile 'foreign-block' @()
        Start-Run 'foreign-block' @{ RWPM_SPIKE_DIR = $p }
        $null = Wait-Run 'foreign-block' 90
        $code = Exit-Code 'foreign-block'
        Check ($code -match '^[1-9][0-9]*$') "launch refused (exit $code)" "exit code: $code"
        Check ((Probe-Lines $p) -eq 0) "game never started" "game ran"
        Check (Log-Has (Join-Path $p 'rwpm.log') 'launch refused') "reason logged" "no reason in rwpm.log"
        Check ((Sha $GameDll) -eq $before) "foreign dll left as it was" "foreign dll changed"
    }
    finally {
        if ((Test-Path $GameDll) -and (Sha $GameDll) -eq $before) { Remove-Item $GameDll; Log "removed dummy dll" }
    }
}

# Default launches with neither -savedatafolder nor -logFile, so Unity writes
# its own Player.log in the game's save folder. No probe: the game is closed after a while.
function Sc-default_log {
    $log = Join-Path $DefaultSave 'Player.log'
    $start = Get-Date
    Start-Run 'default' @{}
    $deadline = (Get-Date).AddSeconds(120)
    $cmd = $null
    while (-not $cmd -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        $cmd = (Game-Procs | Select-Object -First 1).CommandLine
    }
    Start-Sleep -Seconds 45
    $null = Wait-Run 'default' 1
    if (-not $cmd) { Fail "Default: game never started"; return }
    Check ($cmd -notmatch 'savedatafolder') "Default has no -savedatafolder" "Default cmd: $cmd"
    Check ($cmd -notmatch 'logFile') "Default has no -logFile" "Default cmd: $cmd"
    Check ((Test-Path $log) -and (Get-Item $log).LastWriteTime -gt $start) "Default log at $log" "no fresh Player.log at $log"
}

# The app itself opens a visible window on Windows, with no console window beside it.
# The check runs in the desktop session (guicheck.ps1): this SSH session cannot see it.
function Sc-gui_smoke {
    $out = Join-Path $Root 'runs\gui.txt'
    if (Test-Path $out) { Remove-Item $out }
    New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$Root\guicheck.ps1`" -Rwpm `"$Rwpm`" -Out `"$out`""
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName
    $deadline = (Get-Date).AddSeconds(90)
    while (-not (Test-Path $out) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 2 }
    if (-not (Test-Path $out)) { Fail "GUI check did not report"; return }
    $seen = Get-Content $out
    Check ($seen -contains 'title:RimWorld Profile Manager') "GUI window open" "GUI window not found: $($seen -join ' ')"
    Check ($seen -contains 'consoles:0') "no console window with the app" "console window: $($seen -join ' ')"
}
