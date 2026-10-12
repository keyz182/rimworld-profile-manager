# T21: stands in for the player. Runs in the desktop session (started by launch.ps1),
# waits for the guard dialog (a "RimWorld Profile Manager" window owned by powershell.exe),
# clicks the button labelled -Label, and writes what it did to -Out.
param([Parameter(Mandatory = $true)][string]$Label, [Parameter(Mandatory = $true)][string]$Out)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class Click {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, P p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  static string Text(IntPtr h) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256); return sb.ToString(); }
  public static string Press(string title, string label, uint[] owners) {
    IntPtr dialog = IntPtr.Zero;
    EnumWindows((h, l) => { uint pid; GetWindowThreadProcessId(h, out pid);
      if (IsWindowVisible(h) && Text(h) == title && Array.IndexOf(owners, pid) >= 0) { dialog = h; return false; }
      return true; }, IntPtr.Zero);
    if (dialog == IntPtr.Zero) return null;
    IntPtr button = IntPtr.Zero;
    EnumChildWindows(dialog, (h, l) => { if (Text(h) == label) { button = h; return false; } return true; }, IntPtr.Zero);
    if (button == IntPtr.Zero) return "dialog without button";
    SendMessage(button, 0x00F5, IntPtr.Zero, IntPtr.Zero); // BM_CLICK
    return "clicked";
  }
}
"@
$deadline = (Get-Date).AddSeconds(90)
$result = $null
while (-not $result -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500
    $owners = [uint32[]]@(Get-Process powershell -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
    $result = [Click]::Press('RimWorld Profile Manager', $Label, $owners)
}
if (-not $result) { $result = 'no dialog seen' }
"$result $Label" | Set-Content $Out
