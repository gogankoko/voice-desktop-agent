# Desktop smoke test for Phase 11: a 1-minute timer goes off in the real app (chime, spoken,
# shown, window restored from minimized), and the talk shortcut brings the app forward.
# Needs a Gemini key (1-2 requests). Launches the app itself. The shortcut also starts the
# microphone briefly; the test presses it again to stop.
# Usage: close the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-helpers.ps1
param([string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\voice-desktop-agent.exe")
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Keys {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  public static void Press(byte mod1, byte mod2, byte key) {
    keybd_event(mod1, 0, 0, UIntPtr.Zero); keybd_event(mod2, 0, 0, UIntPtr.Zero);
    keybd_event(key, 0, 0, UIntPtr.Zero); keybd_event(key, 0, 2, UIntPtr.Zero);
    keybd_event(mod2, 0, 2, UIntPtr.Zero); keybd_event(mod1, 0, 2, UIntPtr.Zero);
  }
}
"@

if (Get-Process voice-desktop-agent -ErrorAction SilentlyContinue) { "FAIL: close the running app first"; exit 1 }
$WP = [System.Windows.Automation.WindowPattern]::Pattern
$proc = Start-Process $Exe -PassThru
$win = Find-AppWindow
if (-not $win) { "FAIL: window did not appear"; exit 1 }
try {
  Click $win "Settings" | Out-Null; Click $win "Home" | Out-Null  # settles focus after launch
  # The free Gemini tier is often busy: ask up to 3 times, a minute apart.
  $set = $false
  for ($try = 1; $try -le 3 -and -not $set; $try++) {
    if ($try -gt 1) { "    (Gemini busy; asking again in a minute)"; Start-Sleep -Seconds 60 }
    if (-not (Send-RequestSafely $win "Set a timer for 1 minute")) { "SKIP: app is not the foreground window; nothing typed"; exit 2 }
    $set = [bool](Wait-ListItemLike $win "Action: Set reminder*" 40)
  }
  if (-not $set) { "FAIL: no timer was set"; exit 1 }
  "OK: timer set"

  $win.GetCurrentPattern($WP).SetWindowVisualState("Minimized")
  Start-Sleep -Seconds 1
  "OK: app minimized; waiting for the timer (about a minute)"
  Check (Wait-ListItemLike $win "Assistant: *Reminder:*" 90) "reminder went off and was shown" "reminder did not go off"
  Start-Sleep -Seconds 1
  Check ($win.GetCurrentPattern($WP).Current.WindowVisualState -ne "Minimized") "window came back from minimized" "window stayed minimized"

  # Talk shortcut: whichever one the app could register (it logs which).
  $log = "$env:APPDATA\com.voiceagent.desktop\logs\diagnostics.log"
  $line = (Select-String -Path $log -Pattern "talk shortcut is (.+)$" | Select-Object -Last 1)
  if (-not $line) { "FAIL: the app registered no talk shortcut"; exit 1 }
  $shortcut = $line.Matches[0].Groups[1].Value.Trim()
  "OK: the app's talk shortcut is $shortcut"
  $keys = @{ "Ctrl+Alt+Space" = @(0x11, 0x12, 0x20); "Ctrl+Alt+V" = @(0x11, 0x12, 0x56); "Ctrl+Shift+Space" = @(0x11, 0x10, 0x20); "Ctrl+Alt+A" = @(0x11, 0x12, 0x41) }[$shortcut]
  $win.GetCurrentPattern($WP).SetWindowVisualState("Minimized")
  Start-Sleep -Seconds 1
  [Keys]::Press($keys[0], $keys[1], $keys[2])
  Start-Sleep -Seconds 2
  Check ($win.GetCurrentPattern($WP).Current.WindowVisualState -ne "Minimized") "the shortcut restored the window" "window stayed minimized after the shortcut"
  Check ((Get-ForegroundExe) -like "*voice-desktop-agent.exe") "the shortcut brought the app to the front" "app not in front after the shortcut"
  # Either still listening, or it already heard something (room noise counts) and moved on.
  $stop = Find-ByName $win "Listening… press to stop" $CT::Button
  $heard = Wait-ListItemLike $win "You: *" 1 | Where-Object { $_ -notlike "You: Set a timer*" }
  Check ($stop -or $heard) "the shortcut started listening" "not listening after the shortcut (is Listening set up?)"
  if (-not $stop) {
    "--- conversation:"
    foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::ListItem)))) { "    " + $e.Current.Name }
    "--- buttons:"
    foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::Button)))) { "    " + $e.Current.Name + " enabled=" + $e.Current.IsEnabled }
  }
  if ($stop) { $stop.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle() }  # mic off
} finally {
  try { $win.GetCurrentPattern($WP).Close() } catch {}
  if (-not $proc.WaitForExit(10000)) { Stop-Process -Id $proc.Id }
}
if ($script:failed) { exit 1 }
"PASS"
