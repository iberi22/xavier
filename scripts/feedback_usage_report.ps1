# Generates usage feedback report from Xavier indexed memory
# Analyzes what's been indexed, what tags exist, query patterns

$ErrorActionPreference = "Continue"
$XAVIER_URL = "http://localhost:8006"
$TOKEN = "dev-token"
$HEADERS = @{ "Content-Type" = "application/json"; "X-Xavier-Token" = $TOKEN }

Write-Host "=== Usage Feedback Report ==="
Write-Host "Target: $XAVIER_URL"
Write-Host ""

try {
    $health = Invoke-RestMethod -Uri "$XAVIER_URL/health" -Method Get -ErrorAction Stop
    Write-Host "✅ Xavier OK" -ForegroundColor Green
} catch {
    Write-Host "❌ Xavier NOT RESPONDING" -ForegroundColor Red
    exit 1
}

# --- 1. Query patterns: search for different agent sessions ---
Write-Host "`n--- 1. Agent Session Coverage ---" -ForegroundColor Cyan

$agentQueries = @(
    @{ query = "lasantacruz content tiktok video"; label = "LaSantacruz" }
    @{ query = "xavier data commons governance wallet"; label = "Xavier Core" }
    @{ query = "openclaw session conversation"; label = "Main Sessions" }
    @{ query = "worldexams exam practice questions"; label = "WorldExams" }
    @{ query = "ventas inventory sales products"; label = "Ventas" }
    @{ query = "pgheart postgres database query"; label = "PGHeart" }
)

foreach ($aq in $agentQueries) {
    $payload = @{ query = $aq.query; limit = 3 } | ConvertTo-Json -Compress
    try {
        $resp = Invoke-RestMethod -Uri "$XAVIER_URL/memory/search" -Method Post -Body $payload -ContentType "application/json" -Headers $HEADERS -ErrorAction Stop
        $count = if ($resp.results) { $resp.results.Count } else { 0 }
        Write-Host "  $($aq.label): $count results found" -ForegroundColor $(if($count -gt 0){"Green"}else{"Yellow"})
    } catch {
        Write-Host "  $($aq.label): ERROR - $_" -ForegroundColor Red
    }
    Start-Sleep -Milliseconds 50
}

# --- 2. Topic density analysis ---
Write-Host "`n--- 2. Topic Density (content analysis queries) ---" -ForegroundColor Cyan

$topicQueries = @(
    @{ query = "data commons architecture design"; label = "Data Commons" }
    @{ query = "wallet ML-KEM post-quantum cryptography"; label = "Post-Quantum Wallet" }
    @{ query = "governance bicameral vote council proposal"; label = "Governance" }
    @{ query = "reputation proof contribution staking"; label = "Reputation" }
    @{ query = "memory RAG search indexing embedding"; label = "Memory/RAG" }
    @{ query = "Rust compilacion error fix borrow checker"; label = "Rust Code" }
    @{ query = "API endpoint HTTP REST server"; label = "API" }
    @{ query = "configuracion bridge sync session hook"; label = "Bridge Config" }
    @{ query = "benchmark performance latency throughput"; label = "Benchmarking" }
    @{ query = "token supply economic distribution incentive"; label = "Tokenomics" }
)

$topicResults = @()
foreach ($tq in $topicQueries) {
    $payload = @{ query = $tq.query; limit = 5 } | ConvertTo-Json -Compress
    try {
        $resp = Invoke-RestMethod -Uri "$XAVIER_URL/memory/search" -Method Post -Body $payload -ContentType "application/json" -Headers $HEADERS -ErrorAction Stop
        $count = if ($resp.results) { $resp.results.Count } else { 0 }
        $topicResults += @{ label = $tq.label; count = $count }
        Write-Host "  $($tq.label): $count entries"
    } catch {
        $topicResults += @{ label = $tq.label; count = 0 }
        Write-Host "  $($tq.label): ERROR - $_" -ForegroundColor Red
    }
    Start-Sleep -Milliseconds 50
}

# --- 3. Memory stats from Xavier ---
Write-Host "`n--- 3. Xavier Memory Stats ---" -ForegroundColor Cyan

# Try to get memory count
try {
    $payload = @{ query = "xavier memory indexed session"; limit = 100 } | ConvertTo-Json -Compress
    $resp = Invoke-RestMethod -Uri "$XAVIER_URL/memory/search" -Method Post -Body $payload -ContentType "application/json" -Headers $HEADERS -ErrorAction Stop
    $totalMemories = if ($resp.results) { $resp.results.Count } else { 0 }
    Write-Host "  Total memories accessible: ~$totalMemories"
} catch {
    Write-Host "  Could not get memory stats: $_" -ForegroundColor Yellow
}

# --- 4. Cross-agent memory overlap ---
Write-Host "`n--- 4. Cross-Data Commons Knowledge (memory search performance) ---" -ForegroundColor Cyan

$crossQueries = @(
    "gobernanza DAO wallet post-cuantica xavier"
    "data commons funnel reputation staking tokenomics"
    "rules bicameral council vote weight consensus"
    "compilacion Rust gobernanza governance memory"
    "openclaw session index bridge sync config"
)

foreach ($cq in $crossQueries) {
    $payload = @{ query = $cq; limit = 3 } | ConvertTo-Json -Compress
    try {
        $start = Get-Date
        $resp = Invoke-RestMethod -Uri "$XAVIER_URL/memory/search" -Method Post -Body $payload -ContentType "application/json" -Headers $HEADERS -ErrorAction Stop
        $elapsed = ((Get-Date) - $start).TotalMilliseconds
        $count = if ($resp.results) { $resp.results.Count } else { 0 }
        Write-Host "  [$($elapsed.ToString('N1'))ms] '$($cq.Substring(0,40))...' → $count results"
    } catch {
        Write-Host "  ERROR: $cq" -ForegroundColor Red
    }
    Start-Sleep -Milliseconds 30
}

# --- 5. Summary ---
Write-Host "`n==============================" -ForegroundColor Cyan
Write-Host "USAGE FEEDBACK SUMMARY" -ForegroundColor Yellow
Write-Host "==============================" -ForegroundColor Cyan

# Agent coverage
$agentCovered = 0
foreach ($ar in $topicResults) {
    if ($ar.count -gt 0) { $agentCovered++ }
}
$totalTopics = $topicResults.Count

Write-Host "Topics with indexed data: $agentCovered / $totalTopics"
Write-Host "Coverage rate: $([math]::Round($agentCovered/$totalTopics*100, 1))%"

# Most dense topics
Write-Host "`nMost dense topics (by search results):"
$topicResults | Sort-Object -Property count -Descending | Select-Object -First 5 | ForEach-Object {
    Write-Host "  📊 $($_.label): $($_.count) results"
}

# Save report
$report = @{
    timestamp = (Get-Date -Format "o")
    topics = $topicResults
    crossQueries = $crossQueries
    summary = @{
        topicsCovered = $agentCovered
        totalTopics = $totalTopics
        coverageRate = [math]::Round($agentCovered/$totalTopics*100, 1)
    }
} | ConvertTo-Json -Depth 5

$reportPath = "E:\scripts-python\xavier\feedback_usage_report.json"
$report | Out-File -FilePath $reportPath -Encoding utf8
Write-Host "`n📊 Report saved to: $reportPath"
