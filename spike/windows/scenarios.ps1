# T3 Windows spike scenarios, dot-sourced by run.ps1.
# *_control scenarios PASS when the guarded behaviour breaks without the fix.

function Sc-basic {
    $p = New-Profile 'basic' $true @()
    if (Timed-Run $p 'release' 240) { Fail "basic: game did not quit"; return }
    if ((Probe-Lines $p) -ne 1) { Fail "basic: probe lines=$(Probe-Lines $p)"; return }
    Assert-Redirected $p 1
    Check ((Probe-Field $p 'SteamAppId' 1) -eq '294100') "SteamAppId exported" "SteamAppId missing"
    Check (Test-Path (Join-Path $p 'Player.log')) "Player.log in profile" "no profile Player.log"
}

# C15: winhttp.dll present but no Doorstop args → payload never runs.
function Sc-inert {
    $p = New-Profile 'inert' $true @()
    $timedOut = Timed-Run $p 'release' 120 @{ noDoorstopArgs = $true }
    Check ((Payload-Starts $p) -eq 0 -and -not (Test-Path (Join-Path $p 'payload.log'))) "C15 dll inert without args" "C15 payload ran without args"
    Check ($timedOut -and (Probe-Lines $p) -eq 0) "C15 game ran unredirected (probe absent)" "C15 probe ran without injection"
}

function Sc-restart {
    $p = New-Profile 'restart' $true @()
    New-Item -ItemType File (Join-Path $p 'restart.flag') | Out-Null
    if (Timed-Run $p 'release' 360) { Fail "V1 restart: game did not quit (lines=$(Probe-Lines $p))"; return }
    if ((Probe-Lines $p) -ne 2) { Fail "V1 restart: probe lines=$(Probe-Lines $p)"; return }
    Check ((Probe-Field $p 'pid' 1) -ne (Probe-Field $p 'pid' 2)) "V1 two processes" "V1 same pid twice"
    Assert-Redirected $p 1
    Assert-Redirected $p 2
    Check ((Payload-Starts $p) -eq 2) "V1 payload ran in restart child" "V1 payload starts=$(Payload-Starts $p)"
    Log "restart child DOORSTOP_DISABLE=$(Probe-Field $p 'DOORSTOP_DISABLE' 2)"
}

function Sc-restart_control {
    $p = New-Profile 'restart_control' $true @()
    New-Item -ItemType File (Join-Path $p 'restart.flag') | Out-Null
    $null = Timed-Run $p 'spike' 200 @{ env = @{ RWPM_SPIKE_NO_CLEAR = '1' } }
    Check ((Probe-Lines $p) -eq 1 -and (Payload-Starts $p) -eq 1) `
        "V1 control: without clearing, restart child skipped Doorstop" `
        "V1 control: lines=$(Probe-Lines $p) starts=$(Payload-Starts $p) (clearing not shown necessary)"
}

function Sc-stale_env {
    $p = New-Profile 'stale_env' $true @()
    $timedOut = Timed-Run $p 'release' 240 @{ env = @{ DOORSTOP_INITIALIZED = 'TRUE'; DOORSTOP_DISABLE = 'TRUE' } }
    if ((Probe-Lines $p) -ne 1) { Fail "V3 stale env: probe lines=$(Probe-Lines $p) timed_out=$timedOut"; return }
    Assert-Redirected $p 1
}

function Sc-stale_env_control {
    $p = New-Profile 'stale_env_control' $true @()
    $null = Timed-Run $p 'release' 150 @{ keepStale = $true; env = @{ DOORSTOP_INITIALIZED = 'TRUE'; DOORSTOP_DISABLE = 'TRUE' } }
    Check ((Payload-Starts $p) -eq 0 -and (Probe-Lines $p) -eq 0) `
        "V3 control: stale DOORSTOP_DISABLE blocks Doorstop when not unset" `
        "V3 control: starts=$(Payload-Starts $p) lines=$(Probe-Lines $p)"
}

function Sc-no_mods_dir {
    $p = New-Profile 'no_mods_dir' $false @()
    $timedOut = Timed-Run $p 'release' 90
    Check $timedOut "V16 game alive at 90 s" "V16 game exited early"
    Check (Test-Path (Join-Path $p 'Mods')) "V16 Mods dir created" "V16 Mods dir missing"
    Check (Log-Has (Join-Path $p 'payload.log') ' created ') "V16 payload logged create" "V16 no create log"
    Check (-not (Log-Has (Join-Path $p 'Player.log') 'DirectoryNotFoundException')) "V16 no DirectoryNotFoundException" "V16 DirectoryNotFoundException"
}

function Sc-no_mods_dir_control {
    $p = New-Profile 'no_mods_dir_control' $false @()
    $null = Timed-Run $p 'spike' 90 @{ env = @{ RWPM_SPIKE_NO_CREATE = '1' } }
    Check (Log-Has (Join-Path $p 'Player.log') 'DirectoryNotFoundException') `
        "V16 control: missing dir throws without create" "V16 control: no exception (create not shown necessary)"
}

# V26: fail closed. FailFast may raise a WER dialog, so a kill at timeout also counts if FATAL was logged.
function Sc-payload_fail {
    $p = New-Profile 'payload_fail' $true @()
    New-Item -ItemType File (Join-Path $p 'blocker') | Out-Null
    $timedOut = Timed-Run $p 'release' 90 @{ modsDir = (Join-Path $p 'blocker\Mods') }
    $exit = if (Test-Path (Join-Path $p 'exit.txt')) { (Get-Content (Join-Path $p 'exit.txt')).Trim() } else { '?' }
    Check ($timedOut -or $exit -ne '0') "V26 game did not exit cleanly (exit=$exit timed_out=$timedOut)" "V26 exit 0"
    Check (Log-Has (Join-Path $p 'payload.log') 'FATAL') "V26 reason in payload log" "V26 no FATAL in payload log"
    Check ((Probe-Lines $p) -eq 0) "V26 game never loaded mods" "V26 probe ran"
}

# V24 (opt-in, consented): hide steam_appid.txt, rely on SteamAppId env.
function Hide-Appid {
    if (-not (Test-Path $AppidFile)) { return $false }
    Set-Content $AppidMarker $AppidFile
    Log "steam_appid.txt hidden; if interrupted: Move-Item '$AppidFile.rwpm-spike' '$AppidFile'"
    Move-Item $AppidFile "$AppidFile.rwpm-spike"
    return $true
}

function Sc-appid {
    if (-not (Hide-Appid)) { Inconclusive "V24: no steam_appid.txt to hide"; return }
    try {
        $p = New-Profile 'appid' $true @()
        $timedOut = Timed-Run $p 'release' 240
        if ((Probe-Lines $p) -ne 1) { Fail "V24: probe lines=$(Probe-Lines $p) timed_out=$timedOut"; return }
        Check ((Probe-Field $p 'steam' 1) -eq 'True') "V24 Steam initialised without steam_appid.txt" "V24 steam=$(Probe-Field $p 'steam' 1)"
        Check (-not (Log-Has (Join-Path $p 'Player.log') 'SteamAPI.Init() failed')) "V24 no Init failure" "V24 SteamAPI.Init failed"
        Assert-Redirected $p 1

        $c = New-Profile 'appid_control' $true @()
        $null = Timed-Run $c 'release' 200 @{ noAppid = $true }
        $foreign = @(Game-Procs '')
        if ($foreign.Count -gt 0) {
            Pass "V24 control: Steam relaunched the game without our args"
            $foreign | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        }
        else {
            Check ((Probe-Field $c 'steam' 1) -eq 'False' -or (Log-Has (Join-Path $c 'Player.log') 'SteamAPI.Init() failed')) `
                "V24 control: Steam init fails without SteamAppId" "V24 control: steam=$(Probe-Field $c 'steam' 1) (env not shown necessary)"
        }
    }
    finally { Restore-Appid }
}

# ?U1: does Steam "verify integrity" remove or replace our winhttp.dll?
function Sc-verify {
    $before = (Get-FileHash $GameDll).Hash
    $log = 'C:\Program Files (x86)\Steam\logs\content_log.txt'
    $start = (Get-Item $log).Length
    $action = New-ScheduledTaskAction -Execute 'explorer.exe' -Argument 'steam://validate/294100'
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName
    $deadline = (Get-Date).AddSeconds(900)
    $done = $false
    while (-not $done -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 5
        $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
        try { $null = $fs.Seek($start, 'Begin'); $new = (New-Object IO.StreamReader($fs)).ReadToEnd() } finally { $fs.Close() }
        # Done only after the app's own verify pass ends: "Verifying Installed" then "App update changed : None".
        # ("Fully Installed" alone also appears before verifying starts.)
        $done = $new -match '(?s)294100 App update changed : [^\r\n]*Verifying Installed.*294100 App update changed : None'
    }
    if (-not $done) { Inconclusive "?U1: no validation-finished line in content_log within 900 s"; return }
    Check (Test-Path $GameDll) "?U1 winhttp.dll survives Steam verify" "?U1 Steam verify removed winhttp.dll"
    if (Test-Path $GameDll) {
        Check ((Get-FileHash $GameDll).Hash -eq $before) "?U1 winhttp.dll unchanged" "?U1 winhttp.dll replaced"
    }
}
