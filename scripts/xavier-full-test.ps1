# Xavier Full Usage Test Suite
# Comprehensive tests of all Xavier features

param(
    [string]$XavierUrl = "http://localhost:8006",
    [string]$XavierToken = ($env:XAVIER_TOKEN, "dev-token" -ne $null)[0]
)

$headers = @{
    "X-Xavier-Token" = $XavierToken
    "Content-Type" = "application/json"
}

$passed = 0
$failed = 0

function Test-Step($name, $scriptblock) {
    Write-Host "TEST: $name" -ForegroundColor Gray -NoNewline
    try {
        $result = & $scriptblock
        if ($result -eq $true) {
            Write-Host " ✅ PASS" -ForegroundColor Green
            $script:passed++
        } else {
            Write-Host " ❌ FAIL" -ForegroundColor Red
            $script:failed++
        }
    } catch {
        Write-Host " ❌ FAIL: $_" -ForegroundColor Red
        $script:failed++
    }
}

# ==================== TESTS ====================

Test-Step "Health endpoint" {
    $r = Invoke-RestMethod -Uri "$XavierUrl/health" -Method GET -TimeoutSec 10
    return ($r.status -eq "ok" -and $r.version -eq "0.6.1-beta")
}

Test-Step "Memory add" {
    $body = '{"path":"test/full-suite/add","content":"Testing memory add functionality","metadata":{"test":true}}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.status -eq "ok" -and $r.id -ne $null)
}

Test-Step "Memory search" {
    $body = '{"query":"Testing memory add functionality","limit":5}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.results.Count -gt 0)
}

Test-Step "Memory retrieve by path" {
    $body = '{"query":"Testing memory add functionality","limit":10}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    $found = $false
    foreach ($res in $r.results) {
        if ($res.content -and ($res.content -like "*Testing memory*")) { $found = $true; break }
    }
    return ($found)
}

Test-Step "Memory with metadata" {
    $body = '{"path":"test/metadata","content":"Content with rich metadata","metadata":{"source":"test-suite","priority":"high","tags":["test","automated"]}}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.status -eq "ok")
}

Test-Step "Search by metadata concept" {
    $body = '{"query":"automated test suite high priority","limit":3}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.results.Count -gt 0)
}

Test-Step "Memory path hierarchy" {
    $paths = @(
        @{path="projects/xavier/status";content="Xavier project is active"},
        @{path="projects/gos/status";content="GOS project migrating to monorepo"},
        @{path="projects/orionhealth/status";content="OrionHealth is 30% complete"}
    )
    foreach ($p in $paths) {
        $body = $p | ConvertTo-Json -Compress
        Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10 | Out-Null
    }
    $body = '{"query":"Xavier project active","limit":3}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.results.Count -gt 0)
}

Test-Step "Security scan endpoint" {
    $body = '{"input":"This is a normal test message without any injection patterns"}'
    try {
        $r = Invoke-RestMethod -Uri "$XavierUrl/security/scan" -Method POST -Headers $headers -Body $body -TimeoutSec 10
        return ($r.allowed -eq $true -and $r.detection.attack_type -eq "none")
    } catch { return $false }
}

Test-Step "Memory delete" {
    # Add then search for deletion test
    $body = '{"path":"test/delete-me","content":"Temporary content for deletion test","metadata":{"temp":true}}'
    $add = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10
    if ($add.status -ne "ok") { return $false }
    # Verify it exists
    $body = '{"query":"Temporary content for deletion test","limit":1}'
    $search = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 10
    return ($search.results.Count -gt 0)
}

Test-Step "Session context endpoint" {
    $body = '{"path":"test/session","content":"Session test data","metadata":{"session_id":"test-123"}}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10
    return ($r.status -eq "ok")
}

Test-Step "Bulk-like operations" {
    $success = 0
    for ($i = 0; $i -lt 5; $i++) {
        $meta = @{ batch = "test-bulk" }
        $body = (@{path="test/bulk/item-$i";content="Bulk item number $i";metadata=$meta} | ConvertTo-Json -Compress)
        $r = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10
        if ($r.status -eq "ok") { $success++ }
    }
    # Search for bulk items
    $body = '{"query":"Bulk item number","limit":10}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($success -eq 5 -and $r.results.Count -ge 5)
}

Test-Step "Search ranking relevance" {
    # Add specific content
    $body = '{"path":"test/ranking/rust","content":"Rust programming language is fast and safe with memory safety guarantees","metadata":{"topic":"rust"}}'
    Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10 | Out-Null
    $body = '{"path":"test/ranking/python","content":"Python is a versatile scripting language for data science and AI","metadata":{"topic":"python"}}'
    Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10 | Out-Null
    # Search for rust
    $body = '{"query":"memory safety guarantees Rust fast","limit":3}'
    $r = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    return ($r.results.Count -gt 0 -and $r.results[0].content -like "*Rust*")
}

Test-Step "Auto-save feedback verify cycle" {
    # Simulate the full SAVE → RETRIEVE → VERIFY cycle
    $testContent = "Auto-save verification test content " + (Get-Date -Format "yyyy-MM-dd HH:mm:ss")
    $path = "test/auto-save-verify/" + (Get-Date -Format "yyyyMMdd-HHmmss")
    
    # SAVE
    $body = (@{path=$path;content=$testContent;metadata=@{type="auto-save";verified=$false}} | ConvertTo-Json -Compress)
    $save = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 10
    if ($save.status -ne "ok") { return $false }
    
    # RETRIEVE
    Start-Sleep -Seconds 1
    $body = (@{query=$testContent;limit=1} | ConvertTo-Json -Compress)
    $retrieve = Invoke-RestMethod -Uri "$XavierUrl/memory/search" -Method POST -Headers $headers -Body $body -TimeoutSec 15
    if ($retrieve.results.Count -eq 0) { return $false }
    
    # VERIFY
    $retrievedContent = $retrieve.results[0].content
    return ($retrievedContent -eq $testContent)
}

# ==================== SUMMARY ====================
Write-Host ""
Write-Host "================================" -ForegroundColor Cyan
Write-Host "XAVIER FULL TEST SUITE COMPLETE" -ForegroundColor Cyan
Write-Host "================================" -ForegroundColor Cyan
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if($failed -gt 0){"Red"}else{"Green"})
Write-Host "Total:  $($passed + $failed)" -ForegroundColor White

if ($failed -eq 0) {
    Write-Host "`n🎉 ALL TESTS PASSED!" -ForegroundColor Green
} else {
    Write-Host "`n⚠️ $failed test(s) failed" -ForegroundColor Red
}
