$ErrorActionPreference = "SilentlyContinue"
Get-Process MinerDesk -ErrorAction SilentlyContinue | Stop-Process -Force
$path = Join-Path $env:LOCALAPPDATA "com.minerdesk.app\EBWebView"
Remove-Item -Recurse -Force $path -ErrorAction SilentlyContinue
Write-Host "Removed MinerDesk WebView2 cache: $path" -ForegroundColor Green
Write-Host "Start MinerDesk normally (not as Administrator)."
