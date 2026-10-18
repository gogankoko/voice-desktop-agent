# Desktop smoke test for the confirmation gate, using Practice mode (nothing is really sent).
# Needs a Gemini key. Launches the app itself, once per round (a fresh window is reliably the
# foreground window for typing). Practice mode is switched on for the test and set back to
# -RestorePractice afterwards (default: off). Uses roughly 4-6 Gemini requests.
# Usage: close the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-confirm.ps1
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\voice-desktop-agent.exe",
  [bool]$RestorePractice = $false
)
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

if (Get-Process voice-desktop-agent -ErrorAction SilentlyContinue) { "FAIL: close the running app first"; exit 1 }
$TP = [System.Windows.Automation.TogglePattern]::Pattern
$PracticeLabel = "Practice mode (try sending a pretend message; nothing is really sent)"
$Request = "Please send a message to Anna saying I will be there at five"

function Start-App {
  $script:proc = Start-Process $Exe -PassThru
  $script:win = Find-AppWindow
  if (-not $script:win) { throw "window did not appear" }
}
function Stop-App {
  if ($script:proc) {
    try { $script:win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch {}
    if (-not $script:proc.WaitForExit(10000)) { Stop-Process -Id $script:proc.Id }
    $script:proc = $null
  }
}
function Set-Practice($on) {
  Click $win "Settings" | Out-Null
  $cb = Find-ByName $win $PracticeLabel $CT::CheckBox
  if (-not $cb) { throw "practice checkbox not found" }
  if (($cb.GetCurrentPattern($TP).Current.ToggleState -eq "On") -ne $on) {
    $cb.GetCurrentPattern($TP).Toggle(); Start-Sleep -Milliseconds 500
  }
  Click $win "Home" | Out-Null
}
function Texts-Like($pattern) {
  foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::Text)))) {
    if ($e.Current.Name -like $pattern) { return $e.Current.Name }
  }
}
function Dump-Transcript {
  "--- conversation:"
  foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::ListItem)))) { "    " + $e.Current.Name }
}
function Ask-Practice {
  if (-not (Send-RequestSafely $win $Request)) { "SKIP: app is not the foreground window; nothing typed"; exit 2 }
  $deadline = (Get-Date).AddSeconds(40)
  while ((Get-Date) -lt $deadline) {
    $yes = Find-ByName $win "Yes, do it" $CT::Button
    if ($yes) { return $yes }
  }
}

try {
  # Round 1: decline
  Start-App
  Set-Practice $true
  "OK: practice mode on"
  $yes = Ask-Practice
  Check $yes "confirmation screen appeared" "no confirmation screen within 40 s"
  if ($yes) {
    Check (Texts-Like "*Anna*") "screen shows the recipient" "recipient not shown"
    Check (Find-ByName $win "Practice: nothing will really happen") "screen is labelled as practice" "practice label missing"
    Click $win "No" | Out-Null
    Check (Wait-ListItemLike $win "You: No (pressed No)" 10) "No was recorded" "No not recorded"
    Check (Wait-ListItemLike $win "Assistant: *" 1) "assistant answered" "no answer"
    Start-Sleep -Seconds 6
    Check (-not (Wait-ListItemLike $win "Action: *Practice only*" 1)) "declining ran nothing" "an action ran after No!"
  }
  if ($script:failed) { Dump-Transcript }
  Stop-App

  Start-Sleep -Seconds 30  # free tier: 5 requests per minute

  # Round 2: approve
  Start-App
  Set-Practice $true  # also visits Settings and Home, which settles keyboard focus after launch
  $yes = Ask-Practice
  Check $yes "confirmation screen appeared again" "no confirmation screen the second time"
  if ($yes) {
    $yes.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Check (Wait-ListItemLike $win "Action: *Practice only: pretended to send a message to Anna*" 30) "approving ran exactly the practice action" "no practice action after Yes"
    # The reply after Yes: an assistant message that isn't the confirmation question.
    $reply = $null
    $deadline = (Get-Date).AddSeconds(30)
    while (-not $reply -and (Get-Date) -lt $deadline) {
      foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::ListItem)))) {
        $n = $e.Current.Name
        if ($n -like "Assistant: *" -and $n -notlike "*Before I go ahead*") { $reply = $n }
      }
      if (-not $reply) { Start-Sleep -Milliseconds 500 }
    }
    Check ($reply -like "*practice*" -or $reply -like "*nothing*sent*" -or $reply -like "*not*really*") "assistant said it was only practice: '$reply'" "reply after Yes didn't say it was practice: '$reply'"
  }
  if ($script:failed) { Dump-Transcript }
} catch {
  "FAIL: $_"; $script:failed = $true
  if ($script:win) { Dump-Transcript }
} finally {
  try {
    if (-not $script:proc) { Start-App }
    Set-Practice $RestorePractice
    "OK: practice mode set back to $RestorePractice"
  } catch { "WARN: could not restore practice mode: $_" }
  Stop-App
}
if ($script:failed) { exit 1 }
