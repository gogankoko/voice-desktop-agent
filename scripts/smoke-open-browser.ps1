# Desktop smoke test for the first real computer action: "open my browser".
# VISIBLE SIDE EFFECT: opens a new window of your default browser. It does not close any
# browser windows afterwards (that could close your own tabs); close it yourself.
# Usage: launch the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-open-browser.ps1
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

$win = Find-AppWindow
if (-not $win) { "FAIL: window not found"; exit 1 }
"OK: window found"

if (-not (Send-RequestSafely $win "open my browser")) {
  "SKIP: app is not the foreground window; nothing was typed (launch the app just before running)"
  exit 2
}
"OK: typed 'open my browser'"
Check (Wait-ListItemLike $win "Assistant: Opening your web browser.") "app announced the action" "no announcement"

$reply = Wait-ListItemLike $win "Assistant: * is open and in front of you now." 15
Check $reply "app reported verified success: '$reply'" "app did not report verified success"

# Independent check, not trusting the app's own verification.
$fgExe = Get-ForegroundExe
$defaultExe = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice" -ErrorAction SilentlyContinue).ProgId
Check ($fgExe -and $fgExe -notlike "*voice-desktop-agent*") "foreground is now '$fgExe' (default https ProgId: $defaultExe)" "foreground is still the app or unknown: '$fgExe'"
if ($script:failed) { exit 1 }
