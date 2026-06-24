<#
.SYNOPSIS
    Automated backup script for Xavier data stores.

.DESCRIPTION
    Backs up Xavier SQLite databases and agent session logs into a compressed ZIP file.
    Includes rotation to maintain only the last N days of backups.

.PARAMETER BackupDestination
    Directory where backups will be stored. Default: E:\XAVIER-BACKUPS

.PARAMETER RetentionDays
    Number of days to keep backups. Default: 7

.PARAMETER SourceBaseDir
    The base user directory containing the .openclaw folder. Default: C:\Users\belal

.EXAMPLE
    .\backup-xavier.ps1 -BackupDestination "D:\Backups" -RetentionDays 14
#>

param (
    [string]$BackupDestination = "E:\XAVIER-BACKUPS",
    [int]$RetentionDays = 7,
    [string]$SourceBaseDir = "C:\Users\belal"
)

$ErrorActionPreference = "Stop"

# Ensure backup destination exists
if (-not (Test-Path -Path $BackupDestination)) {
    New-Item -Path $BackupDestination -ItemType Directory -Force | Out-Null
}

$LogFile = Join-Path $BackupDestination "backup.log"

function Write-Log {
    param (
        [string]$Message,
        [string]$Level = "INFO"
    )
    $Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    $LogEntry = "[$Timestamp] [$Level] $Message"
    Write-Host $LogEntry
    $LogEntry | Out-File -FilePath $LogFile -Append -Encoding UTF8
}

Write-Log "Starting Xavier backup process..."
Write-Log "Destination: $BackupDestination"
Write-Log "Retention: $RetentionDays days"

try {
    $Timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $StagingDir = Join-Path $env:TEMP "xavier-backup-staging-$Timestamp"
    New-Item -Path $StagingDir -ItemType Directory -Force | Out-Null

    $OpenClawDir = Join-Path $SourceBaseDir ".openclaw"

    $FilesToBackup = @(
        "xavier_memory.db",
        "data\code_graph.db",
        "metrics.db",
        "temp_cortex.db"
    )

    foreach ($RelativePath in $FilesToBackup) {
        $SourcePath = Join-Path $OpenClawDir $RelativePath
        if (Test-Path -Path $SourcePath) {
            $DestPath = Join-Path $StagingDir $RelativePath
            $DestParent = Split-Path -Parent $DestPath
            if (-not (Test-Path -Path $DestParent)) {
                New-Item -Path $DestParent -ItemType Directory -Force | Out-Null
            }
            Copy-Item -Path $SourcePath -Destination $DestPath -Force
            Write-Log "Backed up: $RelativePath"
        } else {
            if ($RelativePath -ne "temp_cortex.db") {
                Write-Log "Warning: $RelativePath not found at $SourcePath" "WARN"
            }
        }
    }

    # Backup agent sessions (.jsonl)
    $AgentsDir = Join-Path $OpenClawDir "agents"
    if (Test-Path -Path $AgentsDir) {
        $SessionFiles = Get-ChildItem -Path $AgentsDir -Filter "*.jsonl" -Recurse | Where-Object { $_.FullName -match "sessions" }
        foreach ($File in $SessionFiles) {
            $RelativePath = $File.FullName.Substring($OpenClawDir.Length + 1)
            $DestPath = Join-Path $StagingDir $RelativePath
            $DestParent = Split-Path -Parent $DestPath
            if (-not (Test-Path -Path $DestParent)) {
                New-Item -Path $DestParent -ItemType Directory -Force | Out-Null
            }
            Copy-Item -Path $File.FullName -Destination $DestPath -Force
            Write-Log "Backed up session: $RelativePath"
        }
    } else {
        Write-Log "Warning: Agents directory not found at $AgentsDir" "WARN"
    }

    # Compress the staging directory
    $ZipFileName = "xavier-backup-$Timestamp.zip"
    $ZipPath = Join-Path $BackupDestination $ZipFileName
    Write-Log "Creating backup ZIP: $ZipFileName"
    Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath -Force

    # Cleanup staging
    Remove-Item -Path $StagingDir -Recurse -Force

    Write-Log "Backup completed successfully: $ZipPath"

    # Rotation: keep last X days
    Write-Log "Cleaning up backups older than $RetentionDays days..."
    $LimitDate = (Get-Date).AddDays(-$RetentionDays)
    $OldBackups = Get-ChildItem -Path $BackupDestination -Filter "xavier-backup-*.zip" | Where-Object { $_.LastWriteTime -lt $LimitDate }

    foreach ($OldBackup in $OldBackups) {
        Write-Log "Removing old backup: $($OldBackup.Name)"
        Remove-Item -Path $OldBackup.FullName -Force
    }

    exit 0
} catch {
    Write-Log "Backup failed: $($_.Exception.Message)" "ERROR"
    if (Test-Path -Path $StagingDir) {
        Remove-Item -Path $StagingDir -Recurse -Force
    }
    exit 1
}
