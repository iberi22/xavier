# Gestalt Event Bus Test Script
param(
    [string]$BusUrl = "http://127.0.0.1:8081"
)

Write-Host "Testing Gestalt Event Bus..." -ForegroundColor Cyan

# Test 1: Check if bus is running
Write-Host "`n[Test 1] Event Bus Connectivity" -ForegroundColor Yellow
try {
    $healthUrl = "$BusUrl/healthz"
    $response = Invoke-WebRequest -Uri $healthUrl -Method GET -TimeoutSec 2 -UseBasicParsing -ErrorAction SilentlyContinue
    if (-not $response -or $response.StatusCode -ne 200) {
        $healthUrl = "$BusUrl/health"
        $response = Invoke-WebRequest -Uri $healthUrl -Method GET -TimeoutSec 2 -UseBasicParsing -ErrorAction Stop
    }
    if ($response.StatusCode -eq 200) {
        Write-Host "Event Bus is running at $BusUrl" -ForegroundColor Green
    }
} catch {
    Write-Host "Event Bus is not running at $BusUrl" -ForegroundColor Red
    Write-Host "Start it with: cd e:\proyectosSWAL\gestalt; cargo run --bin gestalt_cli -- bus serve --port 8081" -ForegroundColor Blue
    exit 1
}

# Test 2: Send test event
Write-Host "`n[Test 2] Send Test Event" -ForegroundColor Yellow
try {
    $testEvent = @{
        agent = "test-script"
        event_type = "test_event"
        state = "Testing"
        summary = "Test event from PowerShell"
        message = "Test event from PowerShell"
        ts = (Get-Date).ToUniversalTime().ToString("o")
        timestamp = (Get-Date).ToUniversalTime().ToString("o")
    } | ConvertTo-Json

    $response = Invoke-RestMethod -Method POST -Uri "$BusUrl/api/event" -Body $testEvent -ContentType "application/json" -TimeoutSec 5
    Write-Host "Test event sent successfully" -ForegroundColor Green
} catch {
    Write-Host "Failed to send test event: $_" -ForegroundColor Red
    exit 1
}

Write-Host "`nGestalt Event Bus tests completed successfully" -ForegroundColor Green
