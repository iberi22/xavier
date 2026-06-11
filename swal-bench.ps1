# SWAL Dogfooding Benchmark Suite for Xavier
# Run with: pwsh -File swal-bench.ps1

$env:XAVIER_TOKEN = "test-bench-token"

$XAVIER_BIN = ".\target\release\xavier.exe"
if (-not (Test-Path $XAVIER_BIN)) {
    $XAVIER_BIN = ".\target\debug\xavier.exe"
}

Write-Host "=== SWAL Benchmarks ==="
Write-Host "Binary: $XAVIER_BIN"
Write-Host ""

# TEST 1: CLI Startup
Write-Host "--- TEST 1: CLI Startup (5 cold runs) ---"
[double[]]$startup_times = @()
1..5 | ForEach-Object {
    $t = Measure-Command { & $XAVIER_BIN --help *>$null }
    $ms = $t.TotalMilliseconds
    $startup_times += $ms
    Write-Host "  Run $_ : $([math]::Round($ms,1))ms"
}
$startup_avg = ($startup_times | Measure-Object -Average).Average
Write-Host "  --> Avg startup: $([math]::Round($startup_avg,1))ms"

Write-Host ""

# TEST 2: Add memory (5 entries)
Write-Host "--- TEST 2: Memory Add (5 entries) ---"
[double[]]$add_times = @()
1..5 | ForEach-Object {
    $content = "SWAL benchmark test entry number $_ - Xavier memory system dogfooding"
    $label = "swal-bench/entry-$_"
    $t = Measure-Command { & $XAVIER_BIN add "$content" "$label" *>$null }
    $ms = $t.TotalMilliseconds
    $add_times += $ms
    Write-Host "  Add $_ : $([math]::Round($ms,1))ms"
}
$add_avg = ($add_times | Measure-Object -Average).Average
Write-Host "  --> Avg add: $([math]::Round($add_avg,1))ms"

Write-Host ""

# TEST 3: Search
Write-Host "--- TEST 3: Search queries ---"
[double[]]$search_times = @()
@("benchmark", "memoria", "vectorial", "xavier", "agent").ForEach({
    $t = Measure-Command { & $XAVIER_BIN search "$_" *>$null }
    $ms = $t.TotalMilliseconds
    $search_times += $ms
    Write-Host "  Search '$_' : $([math]::Round($ms,1))ms"
})
$search_avg = ($search_times | Measure-Object -Average).Average
Write-Host "  --> Avg search: $([math]::Round($search_avg,1))ms"

Write-Host ""

# TEST 4: Stats
Write-Host "--- TEST 4: Stats ---"
$t = Measure-Command { & $XAVIER_BIN stats *>$null }
Write-Host "  Stats: $([math]::Round($t.TotalMilliseconds,1))ms"

Write-Host ""

# TEST 5: Recall
Write-Host "--- TEST 5: Recall ---"
$t = Measure-Command { & $XAVIER_BIN recall "swal" --limit 3 *>$null }
Write-Host "  Recall: $([math]::Round($t.TotalMilliseconds,1))ms"

Write-Host ""

# TEST 6: Version
Write-Host "--- TEST 6: Version ---"
$t = Measure-Command { & $XAVIER_BIN --version *>$null }
Write-Host "  Version: $([math]::Round($t.TotalMilliseconds,1))ms"

# Summary
Write-Host ""
Write-Host "=== SWAL BENCHMARK SUMMARY ==="
Write-Host "Startup avg:   $([math]::Round($startup_avg,1)) ms"
Write-Host "Add avg:       $([math]::Round($add_avg,1)) ms"
Write-Host "Search avg:    $([math]::Round($search_avg,1)) ms"
Write-Host "Stats:         $([math]::Round(($t.TotalMilliseconds),1)) ms"
Write-Host "================================"
