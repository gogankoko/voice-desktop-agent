# End-to-end test: real app + real Gemini + real browser, on the local PRETEND shop page.
# Asks the assistant to search for milk and add it to the basket, then checks the page;
# then asks it to check out and verifies the confirmation screen appears (answers No).
# Nothing is real: the page is a local file with no shop behind it.
# VISIBLE: opens the pretend shop in your browser; the browser comes to the front while the
# assistant types. Needs a Gemini key. Uses roughly 6-12 Gemini requests.
# Usage: close the app, then: powershell -ExecutionPolicy Bypass -File scripts/smoke-shop.ps1
param([string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\voice-desktop-agent.exe")
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\uia-helpers.ps1"

if (Get-Process voice-desktop-agent -ErrorAction SilentlyContinue) { "FAIL: close the running app first"; exit 1 }
$shop = (Resolve-Path "$PSScriptRoot\..\src-tauri\tests\fixtures\shop-test.html").Path
$shopUrl = "file:///" + ($shop -replace '\\', '/')

function Shop-Text {
  $w = $null
  foreach ($c in $A::RootElement.FindAll($T::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
    if ($c.Current.Name -like "*Pretend Grocery Shop*") { $w = $c; break }
  }
  if (-not $w) { return "" }
  $doc = $w.FindFirst($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::Document)))
  if (-not $doc) { return "" }
  $tp = $doc.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
  return $tp.DocumentRange.GetText(20000)
}
function Dump {
  "--- conversation:"
  foreach ($e in $win.FindAll($T::Descendants, (New-Object $PropCond($A::ControlTypeProperty, $CT::ListItem)))) { "    " + $e.Current.Name }
}

# 1. Open the pretend shop in the browser (msedge is the default here; any Chromium browser works).
Start-Process "msedge.exe" $shopUrl
$deadline = (Get-Date).AddSeconds(15)
while ((Get-Date) -lt $deadline -and -not ((Shop-Text) -like "*Pretend Grocery Shop*")) { Start-Sleep -Milliseconds 500 }
Check ((Shop-Text) -like "*Basket: empty*") "pretend shop open, basket empty" "pretend shop didn't open"

# 2. Start the app (it comes to the front, which lets it hand the front to the browser to type).
$proc = Start-Process $Exe -PassThru
$win = Find-AppWindow
try {
  Click $win "Settings" | Out-Null; Click $win "Home" | Out-Null  # settle focus after launch
  if (-not (Send-RequestSafely $win "On the shop page in my browser, search for milk and then add milk to the basket.")) {
    "SKIP: app not in front; nothing typed"; exit 2
  }
  Check (Wait-ListItemLike $win "Action: *Typed *milk* into *Search products* (checked)*" 90) "assistant typed milk into the search box (verified)" "no verified typing"
  Check (Wait-ListItemLike $win "Action: *Pressed *Add milk to basket*" 60) "assistant pressed Add milk to basket" "Add milk not pressed"
  Start-Sleep -Seconds 2
  $page = Shop-Text
  Check ($page -like "*Basket: milk*") "page shows milk in the basket" "basket doesn't show milk"
  Check ($page -like "*Results for*milk*") "page shows the milk search results" "search results not shown"
  if ($script:failed) { Dump }

  Start-Sleep -Seconds 20  # free tier: 5 requests per minute
  Click $win "Home" | Out-Null
  if (-not (Send-RequestSafely $win "Now please check out.")) { "SKIP: app not in front"; exit 2 }
  $deadline = (Get-Date).AddSeconds(60); $yes = $null
  while (-not $yes -and (Get-Date) -lt $deadline) { $yes = Find-ByName $win "Yes, do it" $CT::Button }
  Check $yes "checkout asked for confirmation" "no confirmation screen for checkout"
  if ($yes) {
    Check (Find-ByName $win "Checkout") "confirmation shows the Checkout button" "button name not shown"
    Click $win "No" | Out-Null
    Start-Sleep -Seconds 5
    Check (-not ((Shop-Text) -like "*Pretend payment*Card number*")) "declining did not press Checkout" "checkout happened after No!"
  }
  if ($script:failed) { Dump }
} catch {
  "FAIL: $_"; $script:failed = $true; Dump
} finally {
  try { $win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch {}
  if (-not $proc.WaitForExit(10000)) { Stop-Process -Id $proc.Id }
}
if ($script:failed) { exit 1 }
