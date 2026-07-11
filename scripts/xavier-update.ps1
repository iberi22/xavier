<#
.SYNOPSIS
    Xavier Auto-Update Script — Windows
.DESCRIPTION
    Checks for newer versions of Xavier, downloads or builds, and hot-swaps.
    
    Modes:
      1. GitHub Release (fast path) — downloads pre-built ZIP
      2. Git pull + cargo build (fallback) — builds from source
    
    Usage:
      .\scripts\xavier-update.ps1              # interactive update
      .\scripts\xavier-update.ps1 -Check       # check-only (no update)
      .\scripts\xavier-update.ps1 -Force       # force rebuild even if latest
      .\scripts\xavier-update.ps1 -Cron        # silent cron mode (logs only)
    
    Schedule: Register as daily task via Task Scheduler
#>

param(
    [switch]$Check,
    [switch]$Force,
    [switch]$Cron
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$XavierRoot = Resolve-Path "$ScriptDir\.."
$BinaryName = "xavier.exe"
$BinaryPath = "$XavierRoot\target\release\$BinaryName"
$LogFile = "$XavierRoot\data\logs\update.log"
$MutexName = "XavierAutoUpdate"

# ─── Logger ─────────────────────────────────────────────────────────────────
$null = New-Item -ItemType Directory -Force -Path (Split-Path $LogFile -Parent)
function Log($msg) {
    $line = "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] $msg"
    Add-Content -Path $LogFile -Value $line
    if (-not $Cron) { Write-Host $line }
}
function Die($msg) { Log "FATAL: $msg"; exit 1 }

# ─── Prevent concurrent runs ───────────────────────────────────────────────
$Mutex = New-Object System.Threading.Mutex($false, $MutexName)
if (-not $Mutex.WaitOne(0)) {
    if ($Cron) { exit 0 } else { Die "Another update is already running" }
}

try {
    # ─── Get current version ────────────────────────────────────────────────
    $CurrentVersion = ""
    if (Test-Path $BinaryPath) {
        try {
            $verOutput = & $BinaryPath --version 2>&1 | Out-String
            if ($verOutput -match '(\d+\.\d+\.\d+)') { $CurrentVersion = $Matches[1] }
        } catch { }
    }
    Log "Current version: $(if ($CurrentVersion) { "v$CurrentVersion" } else { 'unknown' })"
    Log "Repository: $XavierRoot"

    # ─── Check GitHub Releases ──────────────────────────────────────────────
    $RemoteTag = ""
    $RemoteVersion = ""
    try {
        $releaseUrl = "https://api.github.com/repos/iberi22/xavier/releases/latest"
        $ghResponse = Invoke-RestMethod -Uri $releaseUrl -ErrorAction SilentlyContinue
        if ($ghResponse -and $ghResponse.tag_name) {
            $RemoteTag = $ghResponse.tag_name
            $RemoteVersion = $RemoteTag -replace '^v', ''
            Log "Remote release: $RemoteTag (v$RemoteVersion)"
        }
    } catch {
        Log "No GitHub Release found (this is normal)"
    }

    # ─── Determine if update is needed ──────────────────────────────────────
    $NeedsUpdate = $false

    if ($RemoteVersion -and $CurrentVersion -and ($RemoteVersion -ne $CurrentVersion)) {
        $curParts = $CurrentVersion -split '\.'
        $remParts = $RemoteVersion -split '\.'
        for ($i = 0; $i -lt 3; $i++) {
            $c = [int](if ($curParts[$i]) { $curParts[$i] } else { 0 })
            $r = [int](if ($remParts[$i]) { $remParts[$i] } else { 0 })
            if ($r -gt $c) { $NeedsUpdate = $true; break }
            elseif ($r -lt $c) { break }
        }
    }

    if (-not $NeedsUpdate -and (Test-Path "$XavierRoot\.git")) {
        # Check git remote
        try {
            Push-Location $XavierRoot
            git fetch --tags origin 2>$null | Out-Null
            $localHash = git rev-parse HEAD
            $remoteHash = git rev-parse origin/main 2>$null
            if ($remoteHash -and ($localHash -ne $remoteHash)) {
                $commitsBehind = git rev-list --count "$localHash..origin/main" 2>$null
                if ([int]$commitsBehind -gt 0) {
                    Log "Git repo is $commitsBehind commit(s) behind origin/main"
                    $NeedsUpdate = $true
                }
            }
            Pop-Location
        } catch {
            Log "Git check failed: $_"
        }
    }

    if ($Force) { $NeedsUpdate = $true; Log "Force update requested" }

    # ─── Check-only mode ────────────────────────────────────────────────────
    if ($Check) { Log "Check complete. Update needed: $NeedsUpdate"; return }

    if (-not $NeedsUpdate -and -not $Force) {
        Log "Already up to date (v$CurrentVersion). Nothing to do."
        return
    }

    Log "=== Update started ==="

    # ─── Strategy A: Download pre-built binary ──────────────────────────────
    $Downloaded = $false
    if ($RemoteTag) {
        $platform = "xavier-windows"
        $archiveUrl = "https://github.com/iberi22/xavier/releases/download/$RemoteTag/$platform.zip"
        $tmpDir = Join-Path $env:TEMP "xavier-update-$(Get-Random)"

        Log "Downloading $archiveUrl ..."
        try {
            $zipPath = "$tmpDir\xavier.zip"
            $null = New-Item -ItemType Directory -Force -Path $tmpDir
            Invoke-WebRequest -Uri $archiveUrl -OutFile $zipPath -ErrorAction SilentlyContinue
            
            if ((Get-Item $zipPath).Length -gt 1MB) {
                Log "Download successful, extracting..."
                Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force
                $dlBinary = Get-ChildItem $tmpDir -Recurse -Filter $BinaryName | Select-Object -First 1
                
                if ($dlBinary) {
                    # Verify
                    try {
                        $verCheck = & $dlBinary.FullName --version 2>&1 | Out-String
                        if ($verCheck -match '\d+\.\d+') {
                            Log "Pre-built binary verified"
                            $Downloaded = $true
                        }
                    } catch { }
                }
            }
        } catch {
            Log "No pre-built binary at $archiveUrl"
        }

        if ($Downloaded) {
            # Hot-swap
            $backupPath = "$XavierRoot\target\release\$BinaryName.backup"
            if (Test-Path $BinaryPath) { Copy-Item $BinaryPath $backupPath -Force }
            Copy-Item $dlBinary.FullName $BinaryPath -Force
            Log "Binary replaced (backup at $backupPath)"
            Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
        } else {
            Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
            Log "Falling back to source build..."
        }
    }

    # ─── Strategy B: Git pull + cargo build ─────────────────────────────────
    if (-not $Downloaded) {
        $cargoAvailable = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cargoAvailable) {
            $rustupAvailable = Get-Command rustup -ErrorAction SilentlyContinue
            if (-not $rustupAvailable) {
                Die "Neither pre-built binary nor Rust toolchain available. Install Rust from https://rustup.rs"
            }
        }

        Log "Pulling latest source..."
        Push-Location $XavierRoot
        try {
            git pull origin main 2>$null | Out-Null
        } catch {
            Log "git pull failed (will try to build from current source anyway)"
        }

        Log "Building Xavier (release + local-gllm)..."
        $buildOutput = cargo build --release --bin xavier --features "local-gllm,cli-interactive" --no-default-features 2>&1
        $buildOutput | ForEach-Object { Add-Content -Path $LogFile -Value "  BUILD: $_" }
        
        Pop-Location

        if (-not (Test-Path $BinaryPath)) {
            Die "Build failed — binary not found at $BinaryPath"
        }
        Log "Build successful"
    }

    # ─── Verify new binary ──────────────────────────────────────────────────
    $newVersion = ""
    try {
        $verOut = & $BinaryPath --version 2>&1 | Out-String
        if ($verOut -match '(\d+\.\d+\.\d+)') { $newVersion = $Matches[1] }
    } catch { }
    Log "Updated to version: v$newVersion"

    # ─── Restart Xavier ─────────────────────────────────────────────────────
    $xavierProc = Get-Process -Name "xavier" -ErrorAction SilentlyContinue
    if ($xavierProc) {
        Log "Restarting Xavier server..."
        $xavierProc | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        
        $startCmd = "Start-Process -FilePath '$BinaryPath' -ArgumentList 'http 8006' -WorkingDirectory '$XavierRoot' -WindowStyle Hidden"
        Invoke-Expression $startCmd
        Start-Sleep -Seconds 3
        
        $newProc = Get-Process -Name "xavier" -ErrorAction SilentlyContinue
        if ($newProc) {
            Log "Xavier restarted successfully (PID $($newProc.Id))"
        } else {
            Die "Xavier failed to restart after update"
        }
    } else {
        Log "Xavier was not running — update complete, ready for manual start"
    }

    Log "=== Update completed successfully ==="
}
finally {
    $Mutex.Dispose()
}
