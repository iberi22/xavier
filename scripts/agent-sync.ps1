param(
    [string]$AgentName = "",
    [switch]$Pull,
    [switch]$Quiet,
    [switch]$Json
)

# Configuration from environment or default path
$XavierBin = $env:XAVIER_BIN_PATH
if (-not $XavierBin) {
    $XavierBin = "C:\Users\belal\bin\xavier.exe"
}

if (-not (Test-Path $XavierBin)) {
    if (-not $Quiet) {
        Write-Error "Xavier binary not found at $XavierBin"
        Write-Host "Please set XAVIER_BIN_PATH environment variable."
    }
    exit 1
}

if (-not $Quiet) {
    Write-Host "=== Agent Memory Sync ==="
    Write-Host "Binary: $XavierBin"
    Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    Write-Host ""
}

$CommonArgs = @()
if ($Json) { $CommonArgs += "--json" }
if ($AgentName) {
    $CommonArgs += "--agent"
    $CommonArgs += $AgentName
}

# Phase 1: Scan
if (-not $Quiet) { Write-Host ">> Phase 1/3: Scanning agents..." }
& $XavierBin agent scan @CommonArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Phase 2: Index
if (-not $Quiet) {
    Write-Host ""
    Write-Host ">> Phase 2/3: Indexing memories..."
}
& $XavierBin agent index @CommonArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Phase 3: Sync (Push/Pull)
if ($Pull) {
    if (-not $Quiet) {
        Write-Host ""
        Write-Host ">> Phase 3/3: Pulling from Supabase..."
    }
    & $XavierBin agent pull @CommonArgs
} else {
    if (-not $Quiet) {
        Write-Host ""
        Write-Host ">> Phase 3/3: Pushing to Supabase..."
    }
    & $XavierBin agent push @CommonArgs
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Summary
if (-not $Quiet) {
    Write-Host ""
    Write-Host "=== Sync Complete ==="
}
