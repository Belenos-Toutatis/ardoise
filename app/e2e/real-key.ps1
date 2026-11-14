# Sends a key to the window of process `-ProcessId` with the real keyboard path (SendKeys: WM_KEYDOWN through the message
# queue), which the web view's browser accelerators see, unlike the DevTools protocol's keys. It refuses (exit 2) unless
# that window is the foreground one when the key is sent, so that no key ever goes to another program.
param([Parameter(Mandatory = $true)][int]$ProcessId, [Parameter(Mandatory = $true)][string]$Keys)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern void SwitchToThisWindow(IntPtr h, bool alt);
}
"@
$handle = (Get-Process -Id $ProcessId).MainWindowHandle
if ($handle -eq [IntPtr]::Zero) { exit 3 }
$owner = 0
[Win]::GetWindowThreadProcessId([Win]::GetForegroundWindow(), [ref]$owner) | Out-Null
if ($owner -ne $ProcessId) {
  [Win]::ShowWindow($handle, 9) | Out-Null
  # Windows only lets the foreground process hand the focus over: SwitchToThisWindow is not held to that (an Alt tap would
  # do it too, but the window then enters its menu mode and eats the next keys). A window that is already in front is left
  # alone: activating it again makes the web view lose its keyboard focus for a moment.
  [Win]::SwitchToThisWindow($handle, $false)
  [Win]::SetForegroundWindow($handle) | Out-Null
  Start-Sleep -Milliseconds 800
  [Win]::GetWindowThreadProcessId([Win]::GetForegroundWindow(), [ref]$owner) | Out-Null
  if ($owner -ne $ProcessId) { exit 2 }
}
[System.Windows.Forms.SendKeys]::SendWait($Keys)
