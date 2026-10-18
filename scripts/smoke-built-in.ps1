# Desktop smoke test for built-in commands (no AI involved): the time, the battery, and
# opening YouTube, each answered in the real app, with how long the answer took.
# VISIBLE SIDE EFFECT: opens YouTube in your browser. Works without a Gemini key.
# Usage: close the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-built-in.ps1
param([string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\voice-desktop-agent.exe")
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

if (Get-Process voice-desktop-agent -ErrorAction SilentlyContinue) { "FAIL: close the running app first"; exit 1 }
$proc = Start-Process $Exe -PassThru
$win = Find-AppWindow
if (-not $win) { "FAIL: window did not appear"; exit 1 }
try {
  Click $win "Settings" | Out-Null; Click $win "Home" | Out-Null  # settles focus after launch
  foreach ($case in @(
      @("what time is it", "Assistant: It's *"),
      @("how much battery do I have", "Assistant: *battery*"),
      @("open YouTube", "Assistant: Opening YouTube.")
    )) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    if (-not (Send-RequestSafely $win $case[0])) { "SKIP: app is not the foreground window; nothing typed"; exit 2 }
    $answer = Wait-ListItemLike $win $case[1] 15
    $ms = $watch.ElapsedMilliseconds
    Check $answer "'$($case[0])' -> '$answer' in $ms ms" "'$($case[0])': no built-in answer"
    [void][Fg]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle)
    Start-Sleep -Seconds 2
  }
  $log = Get-Content "$env:APPDATA\com.voiceagent.desktop\logs\diagnostics.log" -Tail 6
  Check (($log | Select-String "model=built-in").Count -ge 3) "the log shows all three ran without the AI" "the log doesn't show built-in runs"
} finally {
  try { $win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch {}
  if (-not $proc.WaitForExit(10000)) { Stop-Process -Id $proc.Id }
}
if ($script:failed) { exit 1 }
"PASS"
