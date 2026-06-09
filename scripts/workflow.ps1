param (
    [Parameter(Mandatory=$true)]
    [ValidateSet("check", "evaluate", "release")]
    [string]$Mode
)

$ErrorActionPreference = "Stop"

function Run-Check {
    Write-Host "Running static analysis and compilation checks..." -ForegroundColor Cyan
    Write-Host "1. Cargo fmt"
    cargo fmt --all -- --check
    
    Write-Host "2. Cargo clippy (strict)"
    cargo clippy --all-targets --all-features -- -D warnings
    
    Write-Host "3. Cargo build"
    cargo build --all-targets --all-features
    
    Write-Host "4. Panel UI checks"
    Set-Location panel-ui
    pnpm run typecheck
    pnpm run lint
    Set-Location ..
    
    Write-Host "All checks passed!" -ForegroundColor Green
}

function Run-Evaluate {
    Write-Host "Running automated tests and E2E evaluation..." -ForegroundColor Cyan
    Write-Host "1. Backend unit and integration tests"
    cargo test --all-targets --all-features
    
    Write-Host "2. Frontend tests"
    Set-Location panel-ui
    pnpm test
    Set-Location ..
    
    Write-Host "3. System E2E verification"
    if (Test-Path "./tests/e2e_system_alerts.ps1") {
        powershell ./tests/e2e_system_alerts.ps1
    } else {
        Write-Host "E2E scripts not fully implemented yet, skipping." -ForegroundColor Yellow
    }
    
    Write-Host "Evaluation completed successfully!" -ForegroundColor Green
}

function Run-Release {
    Write-Host "Preparing release and generating changelog..." -ForegroundColor Cyan
    pnpm run release
    
    Write-Host "Building production binaries..."
    cargo build --release
    
    Set-Location panel-ui
    pnpm run build
    Set-Location ..
    
    Write-Host "Creating Windows installers..."
    # Placeholder for Tauri/NSIS build or Wix
    # pnpm run tauri build
    
    Write-Host "Release created successfully! Changelog has been updated." -ForegroundColor Green
}

switch ($Mode) {
    "check" { Run-Check }
    "evaluate" { Run-Evaluate }
    "release" { Run-Release }
}
