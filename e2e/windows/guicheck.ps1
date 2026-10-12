# T18 gui_smoke helper. Runs inside the user's desktop session (scheduled task): only
# there are the app's windows visible. Starts rwpm.exe as a double-click would, then
# writes what it saw to <Out>: one "title:<text>" line per visible window, and
# "consoles:<n>" for console hosts attached to rwpm.
param([Parameter(Mandatory = $true)][string]$Rwpm, [Parameter(Mandatory = $true)][string]$Out)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class Win {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  public static List<string> VisibleTitles(uint target) {
    var r = new List<string>();
    EnumWindows((h, l) => { uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == target && IsWindowVisible(h)) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256); r.Add(sb.ToString()); }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
$proc = Start-Process -FilePath $Rwpm -PassThru
$deadline = (Get-Date).AddSeconds(60)
$titles = @()
while ($titles -notcontains 'RimWorld Profile Manager' -and (Get-Date) -lt $deadline -and -not $proc.HasExited) {
    Start-Sleep -Seconds 2
    $titles = @([Win]::VisibleTitles([uint32]$proc.Id))
}
$consoles = @(Get-CimInstance Win32_Process -Filter "Name='conhost.exe'" | Where-Object { $_.ParentProcessId -eq $proc.Id }).Count
$lines = @($titles | ForEach-Object { "title:$_" }) + @("consoles:$consoles", "exited:$($proc.HasExited)")
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
$lines | Set-Content $Out
