# Desktop smoke test: text size survives an app restart.
# Uses UI Automation selection/window patterns only (no keystrokes are sent).
# Usage: powershell -ExecutionPolicy Bypass -File scripts/smoke-prefs.ps1 [-Exe path\to\voice-desktop-agent.exe]
param([string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\voice-desktop-agent.exe")

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$P = [System.Windows.Automation.PropertyCondition]
$CT = [System.Windows.Automation.ControlType]

if (Get-Process voice-desktop-agent -ErrorAction SilentlyContinue) { "FAIL: close the running app first"; exit 1 }
if (-not (Test-Path $Exe)) { "FAIL: $Exe not found (run npm run tauri build)"; exit 1 }

function Find($root, $name, $type) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object $P($A::NameProperty, $name)), (New-Object $P($A::ControlTypeProperty, $type)))
  for ($i = 0; $i -lt 40; $i++) {
    $e = $root.FindFirst($T::Descendants, $c)
    if ($e) { return $e }
    Start-Sleep -Milliseconds 250
  }
  throw "not found: $name"
}

$Sizes = @("Normal", "Large", "Extra large")
$SIP = [System.Windows.Automation.SelectionItemPattern]::Pattern

function Open-Settings {
  $proc = Start-Process $Exe -PassThru
  $win = $null
  for ($i = 0; $i -lt 40 -and -not $win; $i++) {
    $win = $A::RootElement.FindFirst($T::Children, (New-Object $P($A::NameProperty, "Voice Desktop Agent")))
    Start-Sleep -Milliseconds 250
  }
  if (-not $win) { throw "window did not appear" }
  (Find $win "Settings" $CT::Button).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  return @{ proc = $proc; win = $win }
}

function Selected($app) {
  Start-Sleep -Milliseconds 500  # let the saved value load and render
  foreach ($n in $Sizes) {
    if ((Find $app.win $n $CT::RadioButton).GetCurrentPattern($SIP).Current.IsSelected) { return $n }
  }
}

function Choose($app, $name) {
  (Find $app.win $name $CT::RadioButton).GetCurrentPattern($SIP).Select()
}

function Close($app) {
  if (-not $app) { return }
  try { $app.win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch {}
  if (-not $app.proc.WaitForExit(10000)) { Stop-Process -Id $app.proc.Id }
}

$failed = $false
$app = $null
try {
  $app = Open-Settings
  $original = Selected $app
  "OK: started; text size is '$original'"
  $target = if ($original -eq "Large") { "Extra large" } else { "Large" }
  Choose $app $target
  if ((Selected $app) -eq $target) { "OK: chose '$target'" } else { "FAIL: selection did not change"; $failed = $true }
  Close $app

  $app = Open-Settings
  $after = Selected $app
  if ($after -eq $target) { "OK: '$target' survived restart" } else { "FAIL: after restart got '$after'"; $failed = $true }
  Choose $app $original
  if ((Selected $app) -eq $original) { "OK: restored '$original'" } else { "FAIL: could not restore '$original'"; $failed = $true }
} catch {
  "FAIL: $_"; $failed = $true
} finally {
  Close $app
}
if ($failed) { exit 1 }
