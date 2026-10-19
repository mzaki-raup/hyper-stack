# GUI smoke-test harness (docs/screenshots/README.md): start HyperStack against
# a scratch data folder, send keys, click at window-relative coordinates and
# capture screenshots of the window. Used for the 2026-10-15 screenshot set
# and for checking features in the running app (CLAUDE.md gotchas).
#
# Usage: pwsh -File scripts/gui-smoke.ps1 <command> [args]
#   start [fresh]      launch target/debug/HyperStack.exe --data-dir %TEMP%\hs-gui-data
#                      (fresh deletes that folder first); window moved to 0,0 and 1400x900
#   stop               kill the instance started by this script
#   shot <file.png>    save the window (GetWindowRect, so the 8 px border is included)
#   keys <text>        SendKeys syntax: "test1234{ENTER}", "^%s" is Ctrl+Alt+S
#   click <x> <y>      left click at window-relative coordinates
#   rclick <x> <y>     right click
#   wait <ms>          sleep
#   dark | light       switch the Windows app theme (System mode checks)
# Set HS_EXE to use another executable (a release build, for example).
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rr, B; }
}
"@
[W]::SetProcessDPIAware() | Out-Null
$root = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$exe = if ($env:HS_EXE) { $env:HS_EXE } else { Join-Path $root "target\debug\HyperStack.exe" }
$data = "$env:TEMP\hs-gui-data"
$pidFile = "$env:TEMP\hs-gui.pid"
function Win {
  $p = Get-Process -Id (Get-Content $pidFile) -ErrorAction Stop
  $h = $p.MainWindowHandle
  if ($h -eq 0) { Start-Sleep -Milliseconds 500; $p.Refresh(); $h = $p.MainWindowHandle }
  return $h
}
$cmd = $Args[0]
switch ($cmd) {
  "start" {
    if ($Args.Count -gt 1 -and $Args[1] -eq "fresh") { Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue }
    $p = Start-Process -FilePath $exe -ArgumentList @("--data-dir", $data) -PassThru -WindowStyle Hidden
    $p.Id | Set-Content $pidFile
    Start-Sleep -Seconds 4
    $h = Win
    [W]::ShowWindow($h, 9) | Out-Null
    [W]::MoveWindow($h, 0, 0, 1400, 900, $true) | Out-Null
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 800
    "started pid $($p.Id)"
  }
  "stop" {
    try { Stop-Process -Id (Get-Content $pidFile) -Force -ErrorAction SilentlyContinue } catch {}
    "stopped"
  }
  "shot" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    # A click-only frame can leave egui waiting for the next event, so the
    # consequence of the last click (a modal, a tab switch) is not painted.
    # Nudge the pointer by a pixel to force one more frame before capturing.
    $cur = [System.Windows.Forms.Cursor]::Position
    [W]::SetCursorPos($cur.X + 1, $cur.Y + 1) | Out-Null
    Start-Sleep -Milliseconds 150
    [W]::SetCursorPos($cur.X, $cur.Y) | Out-Null
    Start-Sleep -Milliseconds 400
    $r = New-Object W+R
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.Rr - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    $bmp.Save($Args[1], [System.Drawing.Imaging.ImageFormat]::Png)
    "saved $($Args[1]) ${w}x${hh}"
  }
  "keys" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait($Args[1])
    Start-Sleep -Milliseconds 400
    "sent"
  }
  "click" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    $r = New-Object W+R
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $x = $r.L + [int]$Args[1]; $y = $r.T + [int]$Args[2]
    [W]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 120
    [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    "clicked $x,$y"
  }
  "dclick" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    $r = New-Object W+R
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $x = $r.L + [int]$Args[1]; $y = $r.T + [int]$Args[2]
    [W]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 120
    for ($i = 0; $i -lt 2; $i++) {
      [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 400
    "double-clicked $x,$y"
  }
  "drag" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    $r = New-Object W+R
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $x1 = $r.L + [int]$Args[1]; $y1 = $r.T + [int]$Args[2]
    $x2 = $r.L + [int]$Args[3]; $y2 = $r.T + [int]$Args[4]
    [W]::SetCursorPos($x1, $y1) | Out-Null
    Start-Sleep -Milliseconds 200
    [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    for ($i = 1; $i -le 12; $i++) {
      $mx = [int]($x1 + ($x2 - $x1) * $i / 12); $my = [int]($y1 + ($y2 - $y1) * $i / 12)
      [W]::SetCursorPos($mx, $my) | Out-Null
      Start-Sleep -Milliseconds 25
    }
    [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    "dragged $x1,$y1 -> $x2,$y2"
  }
  "rclick" {
    $h = Win
    [W]::SetForegroundWindow($h) | Out-Null
    $r = New-Object W+R
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $x = $r.L + [int]$Args[1]; $y = $r.T + [int]$Args[2]
    [W]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 120
    [W]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero); [W]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    "right-clicked $x,$y"
  }
  "focus" {
    # Windows rejects SetForegroundWindow from a background process. Going
    # topmost for a moment is allowed, and the window comes back to normal
    # before the next frame, so other windows are not left underneath.
    $h = Win
    [W]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, 0x0013) | Out-Null
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 250
    [W]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, 0x0013) | Out-Null
    "focused"
  }
  "wait" { Start-Sleep -Milliseconds ([int]$Args[1]); "waited" }
  "dark" { Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name AppsUseLightTheme -Value 0; "apps dark" }
  "light" { Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name AppsUseLightTheme -Value 1; "apps light" }
  default { "unknown command $cmd" }
}
