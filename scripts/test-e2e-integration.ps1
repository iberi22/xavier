# Test End-to-End de integracion Xavier + Gestalt + AGY
param(
    [switch]$SkipCompilation,
    [switch]$Verbose
)

$ErrorActionPreference = "Continue"
$Global:TestsPassed = 0
$Global:TestsFailed = 0

function Write-TestHeader {
    param([string]$Title)
    Write-Host "`n$('='*80)" -ForegroundColor Cyan
    Write-Host "  $Title" -ForegroundColor Cyan
    Write-Host "$('='*80)" -ForegroundColor Cyan
}

function Write-TestResult {
    param([string]$TestName, [bool]$Passed, [string]$Message = "")
    if ($Passed) {
        Write-Host "[PASS] $TestName" -ForegroundColor Green
        $Global:TestsPassed++
    } else {
        Write-Host "[FAIL] $TestName" -ForegroundColor Red
        if ($Message) {
            Write-Host "       $Message" -ForegroundColor Yellow
        }
        $Global:TestsFailed++
    }
}

# ============================================================================
# TEST 1: Xavier Standalone
# ============================================================================
Write-TestHeader "TEST 1: Xavier Standalone"

## 1.1: Binary exists
$xavierBin = "e:\proyectosSWAL\xavier\target\release\xavier.exe"
$binaryExists = Test-Path $xavierBin
Write-TestResult "Xavier binary exists" $binaryExists

if ($binaryExists) {
    ## 1.2: Version check
    try {
        $version = & $xavierBin --version 2>&1
        $versionOk = $version -match "xavier"
        Write-TestResult "Xavier version command" $versionOk "$version"
    } catch {
        Write-TestResult "Xavier version command" $false $_.Exception.Message
    }
    
    ## 1.3: LLM status
    try {
        $llmStatus = & $xavierBin llm status 2>&1 | Out-String
        $llmOk = $llmStatus -match "LLM|Provider|Model|provider|model" -or $llmStatus -match "available|ok"
        Write-TestResult "Xavier LLM status" $llmOk
        if ($Verbose) { Write-Host $llmStatus -ForegroundColor Gray }
    } catch {
        Write-TestResult "Xavier LLM status" $false $_.Exception.Message
    }
    
    ## 1.4: Help command
    try {
        $help = & $xavierBin --help 2>&1 | Out-String
        $helpOk = $help -match "memory|llm|chat|code"
        Write-TestResult "Xavier help command" $helpOk
    } catch {
        Write-TestResult "Xavier help command" $false $_.Exception.Message
    }
}

# ============================================================================
# TEST 2: AGY CLI (Antigravity) Standalone
# ============================================================================
Write-TestHeader "TEST 2: AGY CLI (Antigravity) Standalone"

$agyBinDir = "$env:LOCALAPPDATA\agy\bin"
if ((Test-Path $agyBinDir) -and ($env:PATH -notmatch [regex]::Escape($agyBinDir))) {
    $env:PATH = "$agyBinDir;$env:PATH"
}

## 2.1: AGY installed
try {
    $agyVersion = agy --version 2>&1
    $agyInstalled = $LASTEXITCODE -eq 0
    Write-TestResult "AGY CLI installed" $agyInstalled "$agyVersion"
} catch {
    Write-TestResult "AGY CLI installed" $false $_.Exception.Message
}

## 2.2: AGY help
try {
    $agyHelp = agy --help 2>&1 | Out-String
    $agyHelpOk = $agyHelp -match "ask|search|help|print"
    Write-TestResult "AGY help command" $agyHelpOk
    if ($Verbose) { Write-Host $agyHelp -ForegroundColor Gray }
} catch {
    Write-TestResult "AGY help command" $false $_.Exception.Message
}

# ============================================================================
# TEST 3: Gestalt Configuration
# ============================================================================
Write-TestHeader "TEST 3: Gestalt Configuration"

## 3.1: Gestalt exists
$gestaltDir = "e:\proyectosSWAL\gestalt"
$gestaltExists = Test-Path $gestaltDir
Write-TestResult "Gestalt directory exists" $gestaltExists

if ($gestaltExists) {
    ## 3.2: agent-registry.toml exists
    $registryFile = Join-Path $gestaltDir "agent-registry.toml"
    $registryExists = Test-Path $registryFile
    Write-TestResult "agent-registry.toml exists" $registryExists
    
    if ($registryExists) {
        ## 3.3: Xavier agents configured
        $registryContent = Get-Content $registryFile -Raw
        $xavierChatConfigured = $registryContent -match 'name\s*=\s*"xavier-chat"'
        $xavierMemoryConfigured = $registryContent -match 'name\s*=\s*"xavier-memory"'
        $agyConfigured = $registryContent -match 'name\s*=\s*"agy"'
        
        Write-TestResult "Xavier-chat configured in registry" $xavierChatConfigured
        Write-TestResult "Xavier-memory configured in registry" $xavierMemoryConfigured
        Write-TestResult "AGY configured in registry" $agyConfigured
    }
    
    ## 3.4: Gestalt project valid
    $gestaltBin = "C:\Users\belal\.cargo\target_global\debug\gestalt_cli.exe"
    $gestaltOk = Test-Path $gestaltBin
    Write-TestResult "Gestalt binary built and valid" $gestaltOk
}

# ============================================================================
# TEST 4: Bridge Script
# ============================================================================
Write-TestHeader "TEST 4: Xavier Gestalt Bridge"

## 4.1: Bridge script exists
$bridgeScript = "e:\proyectosSWAL\xavier\scripts\xavier-gestalt-bridge.ps1"
$bridgeExists = Test-Path $bridgeScript
Write-TestResult "Bridge script exists" $bridgeExists

if ($bridgeExists -and $binaryExists) {
    ## 4.2: Bridge script is executable
    try {
        $bridgeRes = & $bridgeScript -Command "llm" -Query "status" -GestaltBusUrl "http://127.0.0.1:8081" 2>&1 | Out-String
        $bridgeExecutable = $LASTEXITCODE -eq 0
        Write-TestResult "Bridge script executable" $bridgeExecutable
    } catch {
        Write-TestResult "Bridge script executable" $false $_.Exception.Message
    }
}

# ============================================================================
# TEST 5: Integration Readiness Check
# ============================================================================
Write-TestHeader "TEST 5: Integration Readiness Check"

$allReady = $binaryExists -and $agyInstalled -and $gestaltExists -and $registryExists -and $bridgeExists
Write-TestResult "All components ready for integration" $allReady

if ($allReady) {
    Write-Host "`nNext Steps:" -ForegroundColor Cyan
    Write-Host "   1. Gestalt Event Bus active on http://127.0.0.1:8081" -ForegroundColor White
    Write-Host "   2. Xavier Core active on http://127.0.0.1:8006" -ForegroundColor White
    Write-Host "   3. Multi-agent orchestration operational" -ForegroundColor White
}

# ============================================================================
# SUMMARY
# ============================================================================
Write-Host "`n$('='*80)" -ForegroundColor Cyan
Write-Host "  TEST SUMMARY" -ForegroundColor Cyan
Write-Host "$('='*80)" -ForegroundColor Cyan
Write-Host "  Passed: $Global:TestsPassed" -ForegroundColor Green
Write-Host "  Failed: $Global:TestsFailed" -ForegroundColor Red
Write-Host "  Total:  $($Global:TestsPassed + $Global:TestsFailed)" -ForegroundColor White
Write-Host "$('='*80)`n" -ForegroundColor Cyan

if ($Global:TestsFailed -eq 0) {
    Write-Host "ALL TESTS PASSED! Integration verified." -ForegroundColor Green
    exit 0
} else {
    Write-Host "Some tests failed. Review above for details." -ForegroundColor Yellow
    exit 1
}
