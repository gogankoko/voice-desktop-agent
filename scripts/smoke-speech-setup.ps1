# Desktop smoke test for Azure Speech setup, using a deliberately FAKE key.
# Proves the Settings form -> Rust -> Microsoft token endpoint path works, and that a bad
# key is rejected with a plain-language message and is NOT stored.
# Sends one request to Microsoft (it is refused). Never touches a real stored key: it
# refuses to run if listening is already set up.
# Usage: launch the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-speech-setup.ps1
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

$win = Find-AppWindow
if (-not $win) { "FAIL: window not found"; exit 1 }
Click $win "Settings" | Out-Null
if (Find-ByName $win "Remove key from this computer" $CT::Button) {
  "SKIP: a real key is already set up; not testing over it"; exit 2
}

function Type-Into($name, $text) {
  $box = Find-ByName $win $name $CT::Edit
  if (-not $box) { throw "field '$name' not found" }
  $hwnd = [IntPtr]$win.Current.NativeWindowHandle
  [void][Fg]::SetForegroundWindow($hwnd)
  Start-Sleep -Milliseconds 300
  $box.SetFocus()
  Start-Sleep -Milliseconds 200
  if ([Fg]::GetForegroundWindow() -ne $hwnd -or -not $box.Current.HasKeyboardFocus) { return $false }
  [System.Windows.Forms.SendKeys]::SendWait($text)
  Start-Sleep -Milliseconds 200
  return $true
}

if (-not (Type-Into "Azure region" "eastus")) { "SKIP: app not in foreground; nothing typed"; exit 2 }
if (-not (Type-Into "Speech key" ("0" * 32))) { "SKIP: app not in foreground"; exit 2 }
"OK: typed region and fake key"
Click $win "Save and test"

$deadline = (Get-Date).AddSeconds(20); $msg = $null
while (-not $msg -and (Get-Date) -lt $deadline) {
  $msg = Find-ByName $win "Microsoft didn't accept that key for that region. Please check both."
  if (-not $msg) { Start-Sleep -Milliseconds 500 }
}
Check $msg "fake key rejected by Microsoft with a plain-language message" "no rejection message within 20 s"
Check (-not (Find-ByName $win "Remove key from this computer" $CT::Button)) "fake key was not stored" "fake key appears to have been stored"
if ($script:failed) { exit 1 }
