# T3 Windows spike harness. Runs on the VM; launches RimWorld per scenario and checks §V behaviour.
# usage: run.ps1 -Scenario <name|all|cleanup> [-AllowSteamFolderWrite]
# exit: 0 pass, 1 fail, 2 inconclusive. *_control scenarios are fail-first controls.
param(
    [Parameter(Mandatory = $true)][string]$Scenario,
    [switch]$AllowSteamFolderWrite
)
$ErrorActionPreference = 'Stop'

$Root = 'C:\rwpm-spike'
$Game = 'C:\Program Files (x86)\Steam\steamapps\common\RimWorld'
$Exe = Join-Path $Game 'RimWorldWin64.exe'
$GameDll = Join-Path $Game 'winhttp.dll'
$OurDll = Join-Path $Root 'vendor\winhttp.dll'
$AppidFile = Join-Path $Game 'steam_appid.txt'
$AppidMarker = Join-Path $Root '.appid-renamed'
$TaskName = 'rwpm-spike'
$Dlcs = @{ Royalty = 'ludeon.rimworld.royalty'; Ideology = 'ludeon.rimworld.ideology'; Biotech = 'ludeon.rimworld.biotech'; Anomaly = 'ludeon.rimworld.anomaly'; Odyssey = 'ludeon.rimworld.odyssey' }
$script:Result = 0

function Log($m) { Write-Host "[spike] $m" }
function Pass($m) { Log "PASS $m" }
function Fail($m) { Log "FAIL $m"; $script:Result = 1 }
function Inconclusive($m) { Log "INCONCLUSIVE $m"; if ($script:Result -ne 1) { $script:Result = 2 } }
function Check($ok, $passMsg, $failMsg) { if ($ok) { Pass $passMsg } else { Fail $failMsg } }

# C2: always undo an interrupted V24 rename first.
function Restore-Appid {
    if (Test-Path $AppidMarker) {
        if ((Test-Path "$AppidFile.rwpm-spike") -and -not (Test-Path $AppidFile)) {
            Move-Item "$AppidFile.rwpm-spike" $AppidFile
            Log "restored $AppidFile"
        }
        Remove-Item $AppidMarker
    }
}

# V8: never overwrite a winhttp.dll we did not put there.
function Install-Dll {
    $ours = (Get-FileHash $OurDll).Hash
    if (Test-Path $GameDll) {
        if ((Get-FileHash $GameDll).Hash -ne $ours) { throw "foreign winhttp.dll present; refusing to overwrite (V8)" }
        return
    }
    Copy-Item $OurDll $GameDll
    Log "installed winhttp.dll"
}

function Remove-Dll {
    if ((Test-Path $GameDll) -and ((Get-FileHash $GameDll).Hash -eq (Get-FileHash $OurDll).Hash)) {
        Remove-Item $GameDll
        Log "removed winhttp.dll"
    }
}

function Game-Procs([string]$filter) {
    @(Get-CimInstance Win32_Process -Filter "Name='RimWorldWin64.exe'" |
        Where-Object { -not $filter -or $_.CommandLine -like "*$filter*" })
}

function New-Profile([string]$name, [bool]$probe, [string[]]$extra) {
    $p = Join-Path $Root "profiles\$name"
    if (Test-Path $p) { Remove-Item $p -Recurse -Force }
    New-Item -ItemType Directory -Force (Join-Path $p 'SaveData\Config') | Out-Null
    if ($probe) {
        Copy-Item (Join-Path $Root 'probe') (Join-Path $p 'Mods\rwpm.spike.probe') -Recurse
    }
    $owned = @(Get-ChildItem (Join-Path $Game 'Data') -Directory | ForEach-Object { $Dlcs[$_.Name] } | Where-Object { $_ })
    $active = @('ludeon.rimworld') + $owned + $extra
    if ($probe) { $active += 'rwpm.spike.probe' }
    $version = (Get-Content (Join-Path $Game 'Version.txt') -Raw).Trim()
    $xml = "<?xml version=`"1.0`" encoding=`"utf-8`"?>`n<ModsConfigData>`n  <version>$version</version>`n  <activeMods>`n"
    $xml += ($active | ForEach-Object { "    <li>$_</li>`n" }) -join ''
    $xml += "  </activeMods>`n  <knownExpansions>`n"
    $xml += ($owned | ForEach-Object { "    <li>$_</li>`n" }) -join ''
    $xml += "  </knownExpansions>`n</ModsConfigData>`n"
    Set-Content -Path (Join-Path $p 'SaveData\Config\ModsConfig.xml') -Value $xml -Encoding UTF8
    return $p
}

# Launch via an interactive scheduled task so the game runs on the user's desktop.
function Start-Game([string]$p, [string]$flavour, [hashtable]$opts) {
    $cfg = @{ exe = $Exe; payload = (Join-Path $Root "payload-$flavour\Rwpm.Payload.dll"); env = @{} }
    foreach ($k in $opts.Keys) { $cfg[$k] = $opts[$k] }
    $cfg | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $p 'launch.json') -Encoding UTF8
    $launch = Join-Path $Root 'launch.ps1'
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$launch`" -ProfileDir `"$p`""
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName
}

# Wait-Gone → $true when launch.ps1 recorded the first exit and no game process remains.
# Matches every RimWorldWin64.exe: the harness refuses to start while one runs, and the
# in-game restart child's command line did not match a -savedatafolder filter in practice.
function Wait-Gone([string]$p, [int]$timeout) {
    $deadline = (Get-Date).AddSeconds($timeout)
    while (-not (Test-Path (Join-Path $p 'exit.txt')) -or @(Game-Procs '').Count -gt 0) {
        if ((Get-Date) -gt $deadline) { return $false }
        Start-Sleep -Seconds 2
    }
    return $true
}

function Timed-Run([string]$p, [string]$flavour, [int]$timeout, [hashtable]$opts = @{}) {
    Start-Game $p $flavour $opts
    if (Wait-Gone $p $timeout) { return $false }
    foreach ($proc in Game-Procs '') {
        try { Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop }
        catch { Log "WARN could not kill pid $($proc.ProcessId): $($_.Exception.Message)" }
    }
    $null = Wait-Gone $p 30
    return $true  # timed out
}

function Probe-Lines([string]$p) {
    $f = Join-Path $p 'probe.log'
    if (Test-Path $f) { @(Get-Content $f | Where-Object { $_ }).Count } else { 0 }
}

function Probe-Field([string]$p, [string]$key, [int]$n) {
    $line = @(Get-Content (Join-Path $p 'probe.log') | Where-Object { $_ })[$n - 1]
    foreach ($f in $line -split "`t") { if ($f.StartsWith("$key=")) { return $f.Substring($key.Length + 1) } }
    return $null
}

function Payload-Starts([string]$p) {
    $f = Join-Path $p 'payload.log'
    if (Test-Path $f) { @(Select-String -Path $f -Pattern ' start pid=').Count } else { 0 }
}

function Log-Has([string]$file, [string]$pattern) {
    (Test-Path $file) -and [bool](Select-String -Path $file -Pattern $pattern -SimpleMatch -Quiet)
}

function Assert-Redirected([string]$p, [int]$n) {
    $mods = Probe-Field $p 'mods' $n
    Check ($mods -eq (Join-Path $p 'Mods')) "run $n mods=$mods" "run $n mods=$mods"
    $save = Probe-Field $p 'save' $n
    Check ($save -eq (Join-Path $p 'SaveData')) "run $n save dir" "run $n save=$save"
    $installed = (Probe-Field $p 'installed' $n) -split ','
    Check (-not ($installed | Where-Object { $_ -like "*@$Game\Mods\*" })) "run $n no install-Mods mods" "run $n loaded install-Mods mods"
}

. (Join-Path $Root 'scenarios.ps1')

function Invoke-Cleanup {
    Restore-Appid
    Remove-Dll
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Log "cleanup done"
}

Restore-Appid
if ($Scenario -eq 'cleanup') { Invoke-Cleanup; exit 0 }
if (@(Game-Procs '').Count -gt 0) { Log "RimWorld already running; close it first"; exit 1 }
Install-Dll

$all = 'basic', 'inert', 'restart', 'restart_control', 'stale_env', 'stale_env_control', 'no_mods_dir', 'no_mods_dir_control', 'payload_fail'
$optIn = 'appid', 'verify'
$list = if ($Scenario -eq 'all') { $all } else { @($Scenario) }
if (($list | Where-Object { $optIn -contains $_ }) -and -not $AllowSteamFolderWrite) {
    Log "$Scenario needs -AllowSteamFolderWrite"; exit 1
}
try {
    foreach ($s in $list) { Log "=== $s"; & "Sc-$s" }
}
finally {
    Restore-Appid
}
Log "result=$script:Result"
exit $script:Result
