# Spike launch recipe for an OFFLINE profile on Windows. T9 ports this to Rust.
# Run inside the user's interactive session (scheduled task) so the game gets a desktop.
# Reads <ProfileDir>\launch.json: exe, payload, modsDir?, env{}, keepStale, noAppid, noDoorstopArgs.
param([Parameter(Mandatory = $true)][string]$ProfileDir)
$ErrorActionPreference = 'Stop'

$cfg = Get-Content (Join-Path $ProfileDir 'launch.json') -Raw | ConvertFrom-Json

# Scenario-provided env first, so stale-guard scenarios can preset DOORSTOP_* to be cleared below.
foreach ($kv in $cfg.env.PSObject.Properties) { Set-Item "Env:$($kv.Name)" $kv.Value }

# V3: a stale guard from a parent Doorstop process would make ours skip injection.
if (-not $cfg.keepStale) {
    Remove-Item Env:DOORSTOP_INITIALIZED, Env:DOORSTOP_DISABLE -ErrorAction SilentlyContinue
}
# V24: Steam app id via env, independent of steam_appid.txt (R17).
if (-not $cfg.noAppid) { $env:SteamAppId = '294100' }

$env:RWPM_MODS_DIR = if ($cfg.modsDir) { $cfg.modsDir } else { Join-Path $ProfileDir 'Mods' }
$env:RWPM_PAYLOAD_LOG = Join-Path $ProfileDir 'payload.log'
$env:RWPM_SPIKE_DIR = $ProfileDir

$gameArgs = @()
if (-not $cfg.noDoorstopArgs) {
    # C15: winhttp.dll stays inert unless these are passed.
    $gameArgs += '--doorstop-enabled', 'true', '--doorstop-target-assembly', ('"' + $cfg.payload + '"')
}
$gameArgs += ('"-savedatafolder=' + (Join-Path $ProfileDir 'SaveData') + '"')
$gameArgs += '-logFile', ('"' + (Join-Path $ProfileDir 'Player.log') + '"')

$proc = Start-Process -FilePath $cfg.exe -ArgumentList $gameArgs -WorkingDirectory (Split-Path $cfg.exe) -PassThru
$null = $proc.Handle  # keep a handle so ExitCode is available after exit
$proc.WaitForExit()
Set-Content -Path (Join-Path $ProfileDir 'exit.txt') -Value $proc.ExitCode
