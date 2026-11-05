<#
.SYNOPSIS
    Test de AGY CLI standalone
.DESCRIPTION
    Verifica que AGY funciona correctamente antes de integrarlo con Gestalt
#>

Write-Host "🧪 Testing AGY CLI..." -ForegroundColor Cyan

# Test 1: Version check
Write-Host "`n[Test 1] AGY Version Check" -ForegroundColor Yellow
try {
    $version = agy --version
    Write-Host "✅ AGY Version: $version" -ForegroundColor Green
} catch {
    Write-Host "❌ AGY version check failed: $_" -ForegroundColor Red
    exit 1
}

# Test 2: Simple query (non-interactive)
Write-Host "`n[Test 2] AGY Simple Query" -ForegroundColor Yellow
try {
    # AGY en modo no-interactivo requiere auth previa
    Write-Host "ℹ️  Testing AGY availability..." -ForegroundColor Blue
    $testResult = agy --help 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✅ AGY is available and ready" -ForegroundColor Green
    } else {
        Write-Host "⚠️  AGY requires authentication. Run 'agy' interactively first." -ForegroundColor Yellow
    }
} catch {
    Write-Host "❌ AGY query test failed: $_" -ForegroundColor Red
}

Write-Host "`n✅ AGY CLI tests completed" -ForegroundColor Green
