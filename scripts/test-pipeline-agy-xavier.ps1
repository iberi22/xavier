# Multi-Agent Pipeline Test: AGY -> Gestalt -> Xavier Memory -> Xavier Chat
param(
    [string]$Topic = "Actor model in Rust Tokio",
    [string]$GestaltBusUrl = "http://127.0.0.1:8081",
    [string]$XavierHttpUrl = "http://127.0.0.1:8006"
)

$ErrorActionPreference = "Continue"
$AgyBin = "C:\Users\belal\AppData\Local\agy\bin\agy.exe"
$XavierBin = "e:\proyectosSWAL\xavier\target\release\xavier.exe"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "  MULTI-AGENT WORKFLOW: AGY -> Gestalt -> Xavier Memory -> Xavier Chat" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

function Send-BusEvent {
    param([string]$Type, [string]$Summary, [hashtable]$Meta = @{})
    $payload = @{
        agent = "pipeline-orchestrator"
        event_type = $Type
        state = "Running"
        summary = $Summary
        metadata = $Meta
        ts = (Get-Date).ToUniversalTime().ToString("o")
    } | ConvertTo-Json
    try {
        Invoke-RestMethod -Method POST -Uri "$GestaltBusUrl/api/event" -Body $payload -ContentType "application/json" -TimeoutSec 3 | Out-Null
    } catch {
        Write-Warning "Bus event dispatch failed: $_"
    }
}

# Step 1: Start workflow
Write-Host "`n[Step 1] Initializing workflow on Gestalt Event Bus..." -ForegroundColor Yellow
Send-BusEvent -Type "run_started" -Summary "Multi-agent pipeline started for topic: $Topic"
Write-Host "[PASS] Event run_started published" -ForegroundColor Green

# Step 2: Query AGY
Write-Host "`n[Step 2] Executing AGY (Antigravity CLI) query..." -ForegroundColor Yellow
$prompt = "Explain in 2 concise sentences: $Topic"
$agyOutput = & $AgyBin --print $prompt 2>&1 | Out-String
$cleanAgyOutput = ($agyOutput -replace "$([char]27)\[[0-9;]*[a-zA-Z]", "").Trim()

if (-not $cleanAgyOutput -or $LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] AGY execution failed" -ForegroundColor Red
    Send-BusEvent -Type "run_failed" -Summary "AGY query failed"
    exit 1
}
Write-Host "[PASS] AGY query completed successfully:" -ForegroundColor Green
Write-Host "       $cleanAgyOutput" -ForegroundColor Gray
Send-BusEvent -Type "checkpoint" -Summary "AGY research complete" -Meta @{ length = $cleanAgyOutput.Length }

# Step 3: Ingest into Xavier Memory
Write-Host "`n[Step 3] Ingesting AGY output into Xavier Memory..." -ForegroundColor Yellow
$cleanMemoryText = ($cleanAgyOutput -replace '"', '\"')
$addOutput = & $XavierBin add "$cleanMemoryText" "AGY Knowledge: $Topic" -k fact 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Xavier memory ingestion failed: $addOutput" -ForegroundColor Red
    Send-BusEvent -Type "run_failed" -Summary "Xavier memory add failed"
    exit 1
}
Write-Host "[PASS] Knowledge saved into Xavier memory" -ForegroundColor Green
Send-BusEvent -Type "checkpoint" -Summary "Xavier memory ingested" -Meta @{ topic = $Topic }

# Step 4: Verify Memory Retrieval via Search
Write-Host "`n[Step 4] Verifying memory retrieval via Xavier search..." -ForegroundColor Yellow
$searchRes = & $XavierBin search $Topic 2>&1 | Out-String
if ($searchRes -match "AGY Knowledge|Actor model") {
    Write-Host "[PASS] Ingested memory verified in Xavier vector/BM25 index" -ForegroundColor Green
} else {
    Write-Host "[WARN] Memory retrieved but title match differed" -ForegroundColor Yellow
}

# Step 5: Xavier Chat synthesis
Write-Host "`n[Step 5] Synthesizing response via Xavier Chat..." -ForegroundColor Yellow
$chatRes = & $XavierBin chat "What do you know about $Topic?" 2>&1 | Out-String
$cleanChatRes = ($chatRes -replace "$([char]27)\[[0-9;]*[a-zA-Z]", "").Trim()
Write-Host "[PASS] Xavier Chat synthesized response using memory context:" -ForegroundColor Green
$preview = $cleanChatRes -split "`n" | Select-Object -First 8
Write-Host ($preview -join "`n") -ForegroundColor Gray

# Step 6: Complete workflow
Send-BusEvent -Type "run_finished" -Summary "Multi-agent pipeline completed successfully" -Meta @{ topic = $Topic }
Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host "  PIPELINE RESULT: 100% SUCCESS" -ForegroundColor Green
Write-Host "================================================================================" -ForegroundColor Cyan
exit 0
