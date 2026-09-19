$ErrorActionPreference = "Stop"
Write-Host "MinerDesk - Windows build environment check" -ForegroundColor Cyan

function Require-Command($name, $help) {
  if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
    Write-Host "[MISSING] $name" -ForegroundColor Red
    Write-Host $help -ForegroundColor Yellow
    exit 1
  }
  Write-Host "[OK] $name" -ForegroundColor Green
}

Require-Command "node" "Install Node.js LTS: winget install OpenJS.NodeJS.LTS"
Require-Command "npm"  "npm is normally installed with Node.js."
Require-Command "rustc" "Install Rust: winget install --id Rustlang.Rustup, then restart PowerShell."
Require-Command "cargo" "Cargo is installed by Rustup."

Write-Host "Node : $(node -v)"
Write-Host "npm  : $(npm -v)"
Write-Host "Rust : $(rustc -V)"
Write-Host "Cargo: $(cargo -V)"

Push-Location (Join-Path $PSScriptRoot "..")
try {
  Write-Host "`nInstalling JavaScript development dependencies..." -ForegroundColor Cyan
  npm install --include=dev
} finally {
  Pop-Location
}

Write-Host "`nReady. Run .\scripts\dev-windows.ps1 or npm run tauri:dev" -ForegroundColor Green
Write-Host "If Tauri reports a missing MSVC linker, install Visual Studio Build Tools with the 'Desktop development with C++' workload." -ForegroundColor Yellow
