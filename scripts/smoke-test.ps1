# Desktop smoke test: drives the running app through Windows UI Automation.
# No side effects outside the app (the request it types is deliberately unrecognised),
# except that the reply is read aloud if "Read my replies aloud" is on.
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
Check (Find-ByName $win "What this version can do") "sees capability report" "capability report missing"
Check (Find-ByName $win "Your web browser, making the screen bigger, and finding your files (if allowed)") "computer control reported as limited" "computer control status missing"
Check (Find-ByName $win "Available (Windows voices)") "a Windows voice is installed for spoken replies" "no Windows voice found"
Check (Find-ByName $win "Listening (Microsoft Azure Speech)") "listening setup section shown" "listening setup section missing"
Check (Find-ByName $win "Understanding requests (Google Gemini)") "assistant setup section shown" "assistant setup section missing"
Check (Find-ByName $win "0.2.0") "sees version" "version missing"
Click $win "Home"

if (Send-RequestSafely $win "hello there") {
  "OK: typed request"
  Check (Wait-ListItemLike $win "You: hello there") "request shown in transcript" "request not in transcript"
  # Without a Gemini key the fixed-phrase fallback answers; with one, Gemini replies (any text).
  Check (Wait-ListItemLike $win "Assistant: *" 30) "assistant replied" "no reply within 30 s"
} else {
  "SKIP: app is not the foreground window; typing test skipped (launch the app just before running)"
}
if ($script:failed) { exit 1 }
