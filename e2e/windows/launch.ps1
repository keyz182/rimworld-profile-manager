# T18: runs inside the user's interactive session (scheduled task) so the game gets a desktop.
# Reads <Config>.json {rwpm, id, exe, env{}, out, click?} and runs `rwpm run <id> -- <exe>`.
# Writes <out>\exit.txt (rwpm's exit code) and <out>\procs-at-exit.txt (game processes
# still alive when rwpm returned: must be 0, rwpm waits through the in-game Restart).
#
# rwpm is started with Start-Process and file redirects, not `& rwpm *> file`: in Windows
# PowerShell a native program's stderr line becomes an error record, which would stop
# this script and, through the task's job object, kill rwpm mid-run.
param([Parameter(Mandatory = $true)][string]$Config)

$cfg = Get-Content $Config -Raw | ConvertFrom-Json
foreach ($p in $cfg.env.PSObject.Properties) {
    [Environment]::SetEnvironmentVariable($p.Name, [string]$p.Value, 'Process')
}
New-Item -ItemType Directory -Force $cfg.out | Out-Null
$empty = Join-Path $cfg.out 'stdin.txt'
New-Item -ItemType File -Force $empty | Out-Null
# T21: optionally play the player's part in the guard dialog.
if ($cfg.click) {
    $clicker = Join-Path (Split-Path $PSCommandPath) 'clicker.ps1'
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass',
        '-File', "`"$clicker`"", '-Label', "`"$($cfg.click)`"", '-Out', "`"$(Join-Path $cfg.out 'click.txt')`"")
}
$rwpmArgs = @('run', $cfg.id, '--', "`"$($cfg.exe)`"")
$proc = Start-Process -FilePath $cfg.rwpm -ArgumentList $rwpmArgs -PassThru -WindowStyle Hidden `
    -RedirectStandardInput $empty `
    -RedirectStandardOutput (Join-Path $cfg.out 'rwpm.out') `
    -RedirectStandardError (Join-Path $cfg.out 'rwpm.err')
$null = $proc.Handle  # keep the handle so ExitCode is readable after exit
$proc.WaitForExit()
@(Get-Process RimWorldWin64 -ErrorAction SilentlyContinue).Count | Set-Content (Join-Path $cfg.out 'procs-at-exit.txt')
$proc.ExitCode | Set-Content (Join-Path $cfg.out 'exit.txt')
