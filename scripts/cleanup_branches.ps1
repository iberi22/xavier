<#
.SYNOPSIS
    Automated Stale Branch Cleanup Script
.DESCRIPTION
    Deletes stale remote branches that have been merged into main.
    Uses `gh pr list` to identify open PR branches (keeps them),
    and `git branch -r --merged` to find candidates for deletion.
.PARAMETER DryRun
    If set, only shows what would be deleted without actually deleting.
.PARAMETER KeepDays
    Minimum age in days for a merged branch to be considered stale. Default: 7.
.EXAMPLE
    .\scripts\cleanup_branches.ps1 -DryRun
    .\scripts\cleanup_branches.ps1 -KeepDays 14
#>

param(
    [switch]$DryRun = $false,
    [int]$KeepDays = 7
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $RepoRoot

Write-Host "=== Stale Branch Cleanup ===" -ForegroundColor Cyan
Write-Host "Repo: $((gh repo view --json nameWithOwner | ConvertFrom-Json).nameWithOwner)"
Write-Host "Mode: $(if ($DryRun) { 'DRY RUN' } else { 'LIVE' })" -ForegroundColor $(if ($DryRun) { 'Yellow' } else { 'Red' })
Write-Host ""

# 1. Get open PR branch names (protected)
$openPrBranches = gh pr list --state OPEN --json headRefName | ConvertFrom-Json | Select-Object -ExpandProperty headRefName
Write-Host "Open PRs: $($openPrBranches.Count)" -ForegroundColor Green

# 2. Get current branch
$currentBranch = git rev-parse --abbrev-ref HEAD
Write-Host "Current branch: $currentBranch" -ForegroundColor Cyan

# 3. Fetch latest
git fetch origin --prune 2>&1 | Out-Null
Write-Host "Fetched origin (pruned remote tracking)."

# 4. Find merged remote branches
$mergedBranches = git branch -r --merged origin/main | ForEach-Object { $_.Trim() }
$cutoffDate = (Get-Date).AddDays(-$KeepDays)

$deleted = 0
$skipped = 0

foreach ($branch in $mergedBranches) {
    # Skip origin/main, origin/HEAD
    if ($branch -eq "origin/main" -or $branch -eq "origin/HEAD" -or $branch -eq "origin/HEAD -> origin/main") {
        continue
    }

    # Extract short name
    $shortName = $branch -replace "^origin/", ""

    # Skip if has open PR
    if ($shortName -in $openPrBranches) {
        Write-Host "  SKIP (open PR): $shortName" -ForegroundColor Yellow
        $skipped++
        continue
    }

    # Skip if it's the current branch
    if ($shortName -eq $currentBranch) {
        Write-Host "  SKIP (current): $shortName" -ForegroundColor Yellow
        $skipped++
        continue
    }

    # Check age of last commit on branch
    $lastCommitDate = git log -1 --format=%ci origin/$shortName 2>$null
    if (-not $lastCommitDate) {
        Write-Host "  SKIP (no commits?): $shortName" -ForegroundColor Yellow
        $skipped++
        continue
    }

    $branchDate = [DateTime]::ParseExact($lastCommitDate.Substring(0,19), "yyyy-MM-dd HH:mm:ss", $null)
    $ageDays = [Math]::Floor(((Get-Date) - $branchDate).TotalDays)

    if ($ageDays -lt $KeepDays) {
        Write-Host "  SKIP (only $ageDays days old): $shortName" -ForegroundColor Yellow
        $skipped++
        continue
    }

    # Delete
    if ($DryRun) {
        Write-Host "  WOULD DELETE: $shortName (age: ${ageDays}d)" -ForegroundColor Magenta
    } else {
        Write-Host "  DELETING: $shortName (age: ${ageDays}d)" -ForegroundColor Red
        gh api -X DELETE "repos/$(gh repo view --json nameWithOwner | ConvertFrom-Json | Select-Object -ExpandProperty nameWithOwner)/git/refs/heads/$shortName" 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Host "    ✓ Deleted" -ForegroundColor Green
        } else {
            Write-Host "    ✗ Failed to delete (may not exist or permission)" -ForegroundColor Red
        }
    }
    $deleted++
}

Write-Host ""
Write-Host "=== Summary ===" -ForegroundColor Cyan
Write-Host "  Deleted: $deleted"
Write-Host "  Skipped: $skipped"
Write-Host "  Mode: $(if ($DryRun) { 'DRY RUN' } else { 'LIVE' })"
