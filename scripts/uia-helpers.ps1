# Shared UI Automation helpers for the desktop smoke tests. Dot-source this file.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Fg {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$PropCond = [System.Windows.Automation.PropertyCondition]  # not $P: PowerShell names are case-insensitive, $p is common in callers
$CT = [System.Windows.Automation.ControlType]
$script:failed = $false

function Find-AppWindow {
  for ($i = 0; $i -lt 30; $i++) {
    $w = $A::RootElement.FindFirst($T::Children, (New-Object $PropCond($A::NameProperty, "Voice Desktop Agent")))
    if ($w) { return $w }
    Start-Sleep -Milliseconds 500
  }
}

function Find-ByName($win, $name, $type = $null) {
  $cond = New-Object $PropCond($A::NameProperty, $name)
  if ($type) { $cond = New-Object System.Windows.Automation.AndCondition($cond, (New-Object $PropCond($A::ControlTypeProperty, $type))) }
  for ($i = 0; $i -lt 20; $i++) {
    $e = $win.FindFirst($T::Descendants, $cond)
    if ($e) { return $e }
    Start-Sleep -Milliseconds 300
  }
}

# Waits for a transcript list item whose accessible name matches a -like pattern.
function Wait-ListItemLike($win, $pattern, $seconds = 15) {
  $cond = New-Object $PropCond($A::ControlTypeProperty, $CT::ListItem)
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) {
    foreach ($e in $win.FindAll($T::Descendants, $cond)) {
      if ($e.Current.Name -like $pattern) { return $e.Current.Name }
    }
    Start-Sleep -Milliseconds 300
  }
}

function Click($win, $name) {
  $e = Find-ByName $win $name $CT::Button
  if (-not $e) { throw "button '$name' not found" }
  $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  "OK: clicked '$name'"
}

function Check($ok, $okText, $failText) {
  if ($ok) { "OK: $okText" } else { "FAIL: $failText"; $script:failed = $true }
}

function Get-ForegroundExe {
  $pid_ = 0
  [void][Fg]::GetWindowThreadProcessId([Fg]::GetForegroundWindow(), [ref]$pid_)
  try { (Get-Process -Id $pid_).Path } catch { $null }
}

# Types into the request box ONLY if the app is the OS foreground window and the box has focus.
# Element focus alone is not enough: in WebView2 it reflects DOM focus, not where keystrokes go.
# Keystrokes can still be swallowed right after a dialog closes or the window opens, so it
# verifies the text arrived and retries (max 3, only into an empty box). Returns $false if it
# never got a safe moment to type.
function Send-RequestSafely($win, $text) {
  $box = Find-ByName $win "Type a request" $CT::Edit
  if (-not $box) { throw "request box not found" }
  $send = Find-ByName $win "Send" $CT::Button
  $hwnd = [IntPtr]$win.Current.NativeWindowHandle
  $VP = [System.Windows.Automation.ValuePattern]::ValueProperty
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    [void][Fg]::SetForegroundWindow($hwnd)  # may be refused by Windows' foreground lock
    Start-Sleep -Milliseconds 300
    $box.SetFocus()
    Start-Sleep -Milliseconds 300
    if ([Fg]::GetForegroundWindow() -ne $hwnd -or -not $box.Current.HasKeyboardFocus) { return $false }
    if ($box.GetCurrentPropertyValue($VP) -ne "") { throw "request box not empty; refusing to type" }
    # UIA ValuePattern.SetValue would not fire React onChange, so real keystrokes are used.
    [System.Windows.Forms.SendKeys]::SendWait($text)
    for ($i = 0; $i -lt 20; $i++) {
      $value = $box.GetCurrentPropertyValue($VP)
      if ($value -eq $text -and $send.Current.IsEnabled) {
        [void](Click $win "Send")
        return $true
      }
      Start-Sleep -Milliseconds 150
    }
    if ($value -ne "") { throw "request box has unexpected text after typing: '$value'" }
  }
  throw "typed text did not arrive in the request box after 3 attempts"
}
