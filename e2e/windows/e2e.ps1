# T18 Windows e2e: drives the packaged rwpm.exe against the real game on the VM.
# usage: e2e.ps1 -Scenario <name|all|cleanup>
# exit: 0 pass, 1 fail, 2 inconclusive.
# Changes outside C:\rwpm-e2e (consented 2026-10-12): our winhttp.dll beside the game
# (removed at cleanup), test profiles in %LOCALAPPDATA%\RimWorld Profile Manager (to the
# recycle bin at cleanup), one scheduled task, and a dummy foreign winhttp.dll (V8 only).
param([Parameter(Mandatory = $true)][string]$Scenario)
$ErrorActionPreference = 'Stop'

$Root = 'C:\rwpm-e2e'
$App = Join-Path $Root 'app'
$Rwpm = Join-Path $App 'rwpm.exe'
$OurDll = Join-Path $App 'doorstop\winhttp.dll'
$Game = 'C:\Program Files (x86)\Steam\steamapps\common\RimWorld'
$Exe = Join-Path $Game 'RimWorldWin64.exe'
$GameDll = Join-Path $Game 'winhttp.dll'
$Tool = Join-Path $env:LOCALAPPDATA 'RimWorld Profile Manager'
$Profiles = Join-Path $Tool 'profiles'
$Marker = Join-Path $Tool '.rwpm-e2e'
$Installs = Join-Path $Tool 'installs.toml'
$DefaultSave = Join-Path $env:USERPROFILE 'AppData\LocalLow\Ludeon Studios\RimWorld by Ludeon Studios'
$TaskName = 'rwpm-e2e'
$HarmonyWs = '2009463077'
$Workshop = 'C:\Program Files (x86)\Steam\steamapps\workshop\content\294100'
$Dlcs = @{ Royalty = 'ludeon.rimworld.royalty'; Ideology = 'ludeon.rimworld.ideology'; Biotech = 'ludeon.rimworld.biotech'; Anomaly = 'ludeon.rimworld.anomaly'; Odyssey = 'ludeon.rimworld.odyssey' }
$script:Result = 0

function Log($m) { Write-Host "[e2e] $m" }
function Pass($m) { Log "PASS $m" }
function Fail($m) { Log "FAIL $m"; $script:Result = 1 }
function Inconclusive($m) { Log "INCONCLUSIVE $m"; if ($script:Result -ne 1) { $script:Result = 2 } }
function Check($ok, $passMsg, $failMsg) { if ($ok) { Pass $passMsg } else { Fail $failMsg } }

function Sha([string]$f) { (Get-FileHash $f -Algorithm SHA256).Hash.ToLower() }

# Never touch a real install of the tool: only a folder this harness made.
function Open-Tool {
    if ((Test-Path $Tool) -and -not (Test-Path $Marker)) { throw "$Tool exists and is not ours; refusing" }
    New-Item -ItemType Directory -Force $Profiles | Out-Null
    New-Item -ItemType File -Force $Marker | Out-Null
}

# Our dll, recorded as ours the way rwpm records an install (winhttp.rs: installs.toml).
function Install-Dll([bool]$record = $true) {
    if ((Test-Path $GameDll) -and (Sha $GameDll) -ne (Sha $OurDll)) { throw "foreign winhttp.dll present; refusing (V8)" }
    Copy-Item $OurDll $GameDll -Force
    if ($record) {
        "[[winhttp]]`npath = '$GameDll'`nsha256 = `"$(Sha $OurDll)`"`n" | Set-Content $Installs -Encoding ASCII
    } elseif (Test-Path $Installs) { Remove-Item $Installs }
}

function Remove-Dll {
    if ((Test-Path $GameDll) -and (Sha $GameDll) -eq (Sha $OurDll)) { Remove-Item $GameDll; Log "removed winhttp.dll" }
    if (Test-Path $Installs) { Remove-Item $Installs }
}

function Game-Procs { @(Get-CimInstance Win32_Process -Filter "Name='RimWorldWin64.exe'") }

function New-Profile([string]$id, [string[]]$extra, [bool]$probe = $true) {
    $p = Join-Path $Profiles $id
    if (Test-Path $p) { Remove-Item $p -Recurse -Force }
    New-Item -ItemType Directory -Force (Join-Path $p 'SaveData\Config') | Out-Null
    "schema = 1`nname = `"$id`"`nmode = `"offline`"`n" | Set-Content (Join-Path $p 'profile.toml') -Encoding ASCII
    if ($probe) { Copy-Item (Join-Path $Root 'probe') (Join-Path $p 'Mods\rwpm.spike.probe') -Recurse }
    $owned = @(Get-ChildItem (Join-Path $Game 'Data') -Directory | ForEach-Object { $Dlcs[$_.Name] } | Where-Object { $_ })
    $active = @($extra) + @('ludeon.rimworld') + $owned
    if ($probe) { $active += 'rwpm.spike.probe' }
    $version = (Get-Content (Join-Path $Game 'Version.txt') -Raw).Trim()
    $xml = "<?xml version=`"1.0`" encoding=`"utf-8`"?>`n<ModsConfigData>`n  <version>$version</version>`n  <activeMods>`n"
    $xml += ($active | ForEach-Object { "    <li>$_</li>`n" }) -join ''
    $xml += "  </activeMods>`n  <knownExpansions>`n" + (($owned | ForEach-Object { "    <li>$_</li>`n" }) -join '')
    $xml += "  </knownExpansions>`n</ModsConfigData>`n"
    [IO.File]::WriteAllText((Join-Path $p 'SaveData\Config\ModsConfig.xml'), $xml)
    return $p
}

function Run-Dir([string]$id) { Join-Path $Root "runs\$id" }

# Starts `rwpm run <id> -- RimWorldWin64.exe` on the user's desktop.
function Start-Run([string]$id, [hashtable]$envs, [string]$click = '') {
    $out = Run-Dir $id
    if (Test-Path $out) { Remove-Item $out -Recurse -Force }
    New-Item -ItemType Directory -Force $out | Out-Null
    $cfg = Join-Path $out 'launch.json'
    @{ rwpm = $Rwpm; id = $id; exe = $Exe; env = $envs; out = $out; click = $click } | ConvertTo-Json -Depth 4 | Set-Content $cfg -Encoding UTF8
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$Root\launch.ps1`" -Config `"$cfg`""
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName
}

# → $true when rwpm returned and no game process remains; kills everything at timeout.
function Wait-Run([string]$id, [int]$timeout) {
    $exitFile = Join-Path (Run-Dir $id) 'exit.txt'
    $deadline = (Get-Date).AddSeconds($timeout)
    while (-not (Test-Path $exitFile) -or (Game-Procs).Count -gt 0) {
        if ((Get-Date) -gt $deadline) {
            foreach ($proc in Game-Procs) { Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue }
            $end = (Get-Date).AddSeconds(30)
            while (-not (Test-Path $exitFile) -and (Get-Date) -lt $end) { Start-Sleep -Seconds 1 }
            return $false
        }
        Start-Sleep -Seconds 2
    }
    return $true
}

function Exit-Code([string]$id) {
    $f = Join-Path (Run-Dir $id) 'exit.txt'
    if (Test-Path $f) { (Get-Content $f -Raw).Trim() } else { '?' }
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

function Installed-Path([string]$p, [int]$n, [string]$id) {
    foreach ($entry in (Probe-Field $p 'installed' $n) -split ',') {
        $parts = $entry -split '@', 2
        if ($parts[0] -eq $id) { return $parts[1] }
    }
    return $null
}

function Log-Has([string]$file, [string]$pattern) {
    (Test-Path $file) -and [bool](Select-String -Path $file -Pattern $pattern -SimpleMatch -Quiet)
}

function Assert-Redirected([string]$p, [int]$n) {
    Check ((Probe-Field $p 'mods' $n) -eq (Join-Path $p 'Mods')) "run $n mods in profile" "run $n mods=$(Probe-Field $p 'mods' $n)"
    Check ((Probe-Field $p 'save' $n) -eq (Join-Path $p 'SaveData')) "run $n save dir in profile" "run $n save=$(Probe-Field $p 'save' $n)"
    $installed = (Probe-Field $p 'installed' $n) -split ','
    Check (-not ($installed | Where-Object { $_ -like "*@$Game\Mods\*" })) "run $n no install-Mods mods" "run $n loaded install-Mods mods"
}

function Assert-Waited([string]$id) {
    Check ((Exit-Code $id) -eq '0') "${id}: rwpm exit 0" "${id}: rwpm exit $(Exit-Code $id)"
    $procs = (Get-Content (Join-Path (Run-Dir $id) 'procs-at-exit.txt') -Raw).Trim()
    Check ($procs -eq '0') "${id}: rwpm outlived every game process" "${id}: $procs game processes alive when rwpm returned"
}

. (Join-Path $Root 'scenarios.ps1')

function Invoke-Cleanup {
    foreach ($proc in Game-Procs) { Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue }
    Get-Process rwpm -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Dll
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    if (Test-Path $Marker) {
        Add-Type -AssemblyName Microsoft.VisualBasic
        [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($Tool, 'OnlyErrorDialogs', 'SendToRecycleBin')
        Log "tool folder sent to the recycle bin"
    }
    Log "cleanup done"
}

if ($Scenario -eq 'cleanup') { Invoke-Cleanup; exit 0 }
if ((Game-Procs).Count -gt 0) { Log "RimWorld already running; close it first"; exit 1 }
Open-Tool

$all = 'basic', 'restart', 'guard', 'guard_dialog_add', 'guard_dialog_cancel', 'foreign_reuse', 'foreign_block', 'default_log', 'gui_smoke'
$list = if ($Scenario -eq 'all') { $all } else { @($Scenario) }
foreach ($s in $list) { Log "=== $s"; & "Sc-$s" }
Log "result=$script:Result"
exit $script:Result
