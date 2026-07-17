# Scripts Directory

This directory contains utility scripts for Xavier development and operations.

## Organization

- enchmarks/ - Benchmarking scripts for memory systems
- dev/ - Development utilities
- release/ - Release and deployment scripts

## Scripts

| Script | Purpose |
|--------|---------|
| backup.ps1 | Creates dated backups of Xavier databases, memory/, and config/ |
| xavier-service.ps1 | Windows service management |
| xavier_client.ps1 | Client/CLI testing |
| xavier-optimizer.ps1 | Performance optimization |
| locomo_benchmark*.ps1 | LOCOMO benchmark suite |
| cortex_cli.py | Cortex CLI utilities (⚠️ deprecated, project removed) |
| enchmark_*.py | Various benchmark scripts |
| smoke_test_local.sh | Bash script for local-first Xavier auto-contained smoke testing |
| smoke_test_local.ps1 | PowerShell script for local-first Xavier auto-contained smoke testing |

## Local Smoke Tests

Local auto-contained smoke test scripts are provided to confirm that Xavier starts, accepts connections, and responds correctly, without requiring external infrastructure or model providers:

- `scripts/smoke_test_local.sh`: Bash script (runs on Linux/macOS)
- `scripts/smoke_test_local.ps1`: PowerShell script (runs on Windows/Linux/macOS)

### Usage

**Bash:**
```bash
# Run with default settings (port 18006, ./target/debug/xavier)
bash scripts/smoke_test_local.sh

# Run with custom binary and port
XAVIER_BIN=./target/release/xavier XAVIER_PORT=19000 bash scripts/smoke_test_local.sh
```

**PowerShell:**
```powershell
# Run with default settings
.\scripts\smoke_test_local.ps1

# Run with custom binary and port
.\scripts\smoke_test_local.ps1 -XavierBin "./target/release/xavier" -XavierPort 19000
```

## Usage

Most scripts require:
- PowerShell 5+ (Windows)
- Python 3.8+ (some scripts)
- Cargo and Rust toolchain

See individual script headers for specific requirements.

## Xavier Backups

Run a manual backup:

```powershell
cd E:\scripts-python\xavier
.\scripts\backup.ps1
```

Optional parameters:

```powershell
.\scripts\backup.ps1 -BackupRoot E:\backups\xavier -RetentionDays 30
```

The backup script creates `YYYY-MM-DD` folders under `-BackupRoot`, copies every `*.db`
file it finds in the repository, includes matching SQLite `-wal` and `-shm` sidecars,
and copies `memory/` and `config/`. It removes dated backup folders older than
`-RetentionDays`.

### Windows Scheduled Task

Create a daily scheduled backup at 2:00 AM:

```powershell
$Action = New-ScheduledTaskAction `
  -Execute "powershell.exe" `
  -Argument "-NoProfile -ExecutionPolicy Bypass -File `"E:\scripts-python\xavier\scripts\backup.ps1`" -BackupRoot `"E:\backups\xavier`" -RetentionDays 30"

$Trigger = New-ScheduledTaskTrigger -Daily -At 2:00AM
$Settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries

Register-ScheduledTask `
  -TaskName "XavierBackup" `
  -Action $Action `
  -Trigger $Trigger `
  -Settings $Settings `
  -Description "Daily Xavier backup"
```

Run it on demand:

```powershell
Start-ScheduledTask -TaskName "XavierBackup"
```

Check task state:

```powershell
Get-ScheduledTask -TaskName "XavierBackup"
Get-ScheduledTaskInfo -TaskName "XavierBackup"
```

Remove the task:

```powershell
Unregister-ScheduledTask -TaskName "XavierBackup" -Confirm:$false
```
