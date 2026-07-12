# start-xavier.ps1 - Xavier Windows Launcher (PowerShell)
# Panel UI: http://localhost:8006/panel

$ErrorActionPreference = "Stop"
$XavierDir = "E:\scripts-python\xavier"
$DataDir   = "$env:USERPROFILE\.xavier\data"
$Binary    = "$XavierDir\target\release\xavier.exe"
$Port      = 8006

Write-Host "`n=== Xavier Cognitive Memory Runtime v0.12.0 ===" -ForegroundColor Cyan
Write-Host "Panel UI: http://localhost:$Port/panel" -ForegroundColor Green
Write-Host "Health:   http://localhost:$Port/health" -ForegroundColor Green
Write-Host ""

# Ensure data dir
if (-not (Test-Path $DataDir)) { New-Item -ItemType Directory -Path $DataDir -Force | Out-Null }

# Set env vars
$env:XAVIER_DATA_DIR = $DataDir
$env:XAVIER_TOKEN = "dev-token"
$env:XAVIER_LOG_LEVEL = "info"
$env:XAVIER_EMBEDDING_PROVIDER_MODE = "cloud"

# Kill any existing instance
Get-Process -Name "xavier" -ErrorAction SilentlyContinue | Stop-Process -Force

# Start Xavier
Write-Host "Starting Xavier..." -ForegroundColor Yellow
$process = Start-Process -FilePath $Binary -ArgumentList "http $Port" -NoNewWindow -PassThru -WorkingDirectory $DataDir

Write-Host "Xavier running (PID: $($process.Id))" -ForegroundColor Green
Start-Sleep -Seconds 5

# Open panel in browser
Start-Process "http://localhost:$Port/panel"

Write-Host "`nPress any key to stop Xavier..." -ForegroundColor Yellow
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")

# Cleanup
Write-Host "Shutting down..." -ForegroundColor Yellow
$process | Stop-Process -Force
Write-Host "Done." -ForegroundColor Green
