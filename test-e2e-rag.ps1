# test-e2e-rag.ps1
# End-to-End Test for Xavier RAG Flow

$PORT = 8990
$TOKEN = "e2e-test-token"
$XAVIER_BIN = "xavier"

if (-not (Get-Command $XAVIER_BIN -ErrorAction SilentlyContinue)) {
    if (Test-Path "./xavier.exe") { $XAVIER_BIN = "./xavier.exe" }
    elseif (Test-Path "./target/debug/xavier.exe") { $XAVIER_BIN = "./target/debug/xavier.exe" }
    elseif (Test-Path "./target/release/xavier.exe") { $XAVIER_BIN = "./target/release/xavier.exe" }
}

Write-Host "🚀 Starting E2E RAG Test..." -ForegroundColor Cyan

$env:XAVIER_TOKEN = $TOKEN
$env:XAVIER_CONFIG_DIR = Join-Path $env:TEMP ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $env:XAVIER_CONFIG_DIR | Out-Null

$Process = Start-Process $XAVIER_BIN -ArgumentList "http", $PORT -NoNewWindow -PassThru

try {
    # 1. Wait for readiness
    Write-Host "⏳ Waiting for readiness..." -ForegroundColor Gray
    $ready = $false
    for ($i=0; $i -lt 30; $i++) {
        try {
            $resp = Invoke-RestMethod -Uri "http://localhost:$PORT/v1/health/ready" -ErrorAction SilentlyContinue
            if ($resp.status -eq "ok") {
                $ready = $true
                break
            }
        } catch {}
        Start-Sleep -Seconds 1
    }

    if (-not $ready) {
        Write-Host "❌ Server failed to become ready." -ForegroundColor Red
        exit 1
    }
    Write-Host "✅ Server ready!" -ForegroundColor Green

    # 2. Add Memory
    Write-Host "📝 Adding memory..." -ForegroundColor Gray
    $headers = @{ "X-Xavier-Token" = $TOKEN }
    $body = @{
        text = "The secret code for today is XAVIER-2026"
        content = "The secret code for today is XAVIER-2026"
        user_id = "tester"
    } | ConvertTo-Json

    $resp = Invoke-RestMethod -Method Post -Uri "http://localhost:$PORT/v1/memories" -Headers $headers -Body $body -ContentType "application/json"

    if ($resp.status -ne "ok") {
        Write-Host "❌ Failed to add memory." -ForegroundColor Red
        exit 1
    }
    Write-Host "✅ Memory added!" -ForegroundColor Green

    # 3. Search Memory
    Write-Host "🔍 Searching memory..." -ForegroundColor Gray
    $searchBody = @{
        query = "secret code"
        limit = 1
    } | ConvertTo-Json

    $resp = Invoke-RestMethod -Method Post -Uri "http://localhost:$PORT/v1/memories/search" -Headers $headers -Body $searchBody -ContentType "application/json"

    if ($resp.results[0].memory -like "*XAVIER-2026*") {
        Write-Host "✅ Search successful! Found the secret code." -ForegroundColor Green
    } else {
        Write-Host "❌ Search failed or result missing." -ForegroundColor Red
        exit 1
    }

    Write-Host "🎉 E2E RAG Test PASSED!" -ForegroundColor Green
}
finally {
    Write-Host "🧹 Cleaning up..." -ForegroundColor Gray
    Stop-Process -Id $Process.Id -Force
    Remove-Item -Path $env:XAVIER_CONFIG_DIR -Recurse -ErrorAction SilentlyContinue
}
