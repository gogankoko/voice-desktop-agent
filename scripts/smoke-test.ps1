# Desktop smoke test: drives the running app through Windows UI Automation.
# Usage: start `npm run tauri dev`, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$P = [System.Windows.Automation.PropertyCondition]

$win = $null
for ($i = 0; $i -lt 30 -and -not $win; $i++) {
  $win = $A::RootElement.FindFirst($T::Children, (New-Object $P($A::NameProperty, "Voice Desktop Agent")))
  if (-not $win) { Start-Sleep -Milliseconds 500 }
}
if (-not $win) { "FAIL: window not found"; exit 1 }
"OK: window found"

$ErrorActionPreference = "Stop"
$CT = [System.Windows.Automation.ControlType]
function Find-ByName($name, $type = $null) {
  $cond = New-Object $P($A::NameProperty, $name)
  if ($type) { $cond = New-Object System.Windows.Automation.AndCondition($cond, (New-Object $P($A::ControlTypeProperty, $type))) }
  for ($i = 0; $i -lt 20; $i++) {
    $e = $win.FindFirst($T::Descendants, $cond)
    if ($e) { return $e }
    Start-Sleep -Milliseconds 300
  }
}
function Click($name) {
  $e = Find-ByName $name $CT::Button
  if (-not $e) { "FAIL: '$name' not found"; exit 1 }
  $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  "OK: clicked '$name'"
}
function Expect($name) {
  if (Find-ByName $name) { "OK: sees '$name'" } else { "FAIL: missing '$name'"; $script:failed = $true }
}

Expect "Talk to me"
if (Find-ByName "Stop" $CT::Button) { "OK: sees Stop button" } else { "FAIL: no Stop button"; $script:failed = $true }
Click "Settings"
Expect "Not available yet"   # proves get_app_status IPC round-trip + schema validation succeeded
Expect "0.1.0"
Click "Home"

$box = Find-ByName "Type a request" $CT::Edit
# Real keystrokes: UIA ValuePattern.SetValue does not fire React onChange in WebView2.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Fg {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
"@
# May be refused by Windows' foreground lock; the check below then skips typing.
[void][Fg]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle)
Start-Sleep -Milliseconds 300
$box.SetFocus()
Start-Sleep -Milliseconds 200
# Never type unless BOTH the OS foreground window is ours AND the element has focus.
# HasKeyboardFocus alone reflects DOM focus inside WebView2, not where keystrokes go.
$hwnd = [IntPtr]$win.Current.NativeWindowHandle
if ([Fg]::GetForegroundWindow() -ne $hwnd -or -not $box.Current.HasKeyboardFocus) {
  "SKIP: app is not the foreground window; typing test skipped (run with the app focused)"
} else {
  [System.Windows.Forms.SendKeys]::SendWait("open my browser")
  Start-Sleep -Milliseconds 300
  "OK: typed request"
  Click "Send"
  Expect "open my browser"
  $reply = $win.FindFirst($T::Descendants, (New-Object $P($A::NameProperty, "I heard you, but I can't act on requests yet. This early version isn't connected to an assistant or to your computer.")))
  if ($reply) { "OK: honest not-connected reply shown" } else { "FAIL: reply not shown"; $failed = $true }
}
if ($failed) { exit 1 }
