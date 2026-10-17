# Desktop smoke test: drives the running app through Windows UI Automation.
# Has no side effects outside the app (the request it types is deliberately unrecognised).
# Usage: start the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

$win = Find-AppWindow
if (-not $win) { "FAIL: window not found"; exit 1 }
"OK: window found"

Check (Find-ByName $win "Talk to me" $CT::Button) "sees mic button" "no mic button"
Check (Find-ByName $win "Stop" $CT::Button) "sees Stop button" "no Stop button"
Click $win "Settings"
# These come from the get_app_status IPC round-trip, validated by zod.
Check (Find-ByName $win "Not available yet") "sees capability report" "capability report missing"
Check (Find-ByName $win "Only opening your web browser, so far") "computer control reported as limited" "computer control status missing"
Check (Find-ByName $win "0.1.0") "sees version" "version missing"
Click $win "Home"

if (Send-RequestSafely $win "hello there") {
  "OK: typed request"
  Check (Wait-ListItemLike $win "You: hello there") "request shown in transcript" "request not in transcript"
  Check (Wait-ListItemLike $win "Assistant: Sorry, I can only do a few things so far*") "honest can't-do-that reply shown" "reply not shown"
} else {
  "SKIP: app is not the foreground window; typing test skipped (launch the app just before running)"
}
if ($script:failed) { exit 1 }
