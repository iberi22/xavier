# Script de Backup Manual - Xavier Data Stores
# Ejecutar: powershell -ExecutionPolicy Bypass -File C:\Users\belal\xavier-review\scripts\backup-xavier.ps1

$ErrorActionPreference = "Stop"
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backupDir = "E:\XAVIER-BACKUPS"
$logFile = "$backupDir\backup.log"
$homeDir = $env:USERPROFILE

# Crear directorio si no existe
New-Item -ItemType Directory -Path $backupDir -Force | Out-Null

Write-Output "[$timestamp] === INICIO BACKUP XAVIER ===" | Tee-Object -FilePath $logFile -Append

# Archivos a respaldar
$sources = @(
    "$homeDir\.openclaw\xavier_memory.db"
    "$homeDir\.openclaw\data\code_graph.db"
    "$homeDir\.openclaw\metrics.db"
    "$homeDir\.openclaw\temp_cortex.db"
    "$homeDir\.openclaw\trading.db"
)

$sessionDirs = @(
    "$homeDir\.openclaw\agents\coder\sessions\sessions.json"
    "$homeDir\.openclaw\agents\codex\sessions\sessions.json"
    "$homeDir\.openclaw\agents\ghost\sessions\sessions.json"
    "$homeDir\.openclaw\agents\inventario\sessions\sessions.json"
    "$homeDir\.openclaw\agents\lasantacruz\sessions\sessions.json"
)

$zipPath = "$backupDir\xavier-backup-$timestamp.zip"
$tempDir = "$backupDir\temp-$timestamp"

New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

$totalSize = 0
$fileCount = 0

foreach ($src in ($sources + $sessionDirs)) {
    if (Test-Path $src) {
        $relPath = $src.Substring($homeDir.Length).TrimStart('\')
        $destDir = Split-Path "$tempDir\$relPath" -Parent
        New-Item -ItemType Directory -Path $destDir -Force | Out-Null
        Copy-Item -Path $src -Destination "$tempDir\$relPath" -Force
        $size = (Get-Item $src).Length
        $totalSize += $size
        $fileCount++
        Write-Output "  OK  $relPath ($([math]::Round($size/1KB)) KB)" | Tee-Object -FilePath $logFile -Append
    } else {
        Write-Output "  MISS  $src (no encontrado)" | Tee-Object -FilePath $logFile -Append
    }
}

# Comprimir
Compress-Archive -Path "$tempDir\*" -DestinationPath $zipPath -Force

# Limpiar temp
Remove-Item -Path $tempDir -Force -Recurse -ErrorAction SilentlyContinue

# Rotación: mantener últimos 7 días
$cutoff = (Get-Date).AddDays(-7)
Get-ChildItem -Path $backupDir -Filter "xavier-backup-*.zip" | Where-Object { $_.LastWriteTime -lt $cutoff } | ForEach-Object {
    Remove-Item $_.FullName -Force
    Write-Output "  PURGED $($_.Name)" | Tee-Object -FilePath $logFile -Append
}

Write-Output "[$timestamp] === BACKUP COMPLETADO ===" | Tee-Object -FilePath $logFile -Append
Write-Output "  Archivos: $fileCount" | Tee-Object -FilePath $logFile -Append
Write-Output "  Tamaño total: $([math]::Round($totalSize/1MB, 2)) MB" | Tee-Object -FilePath $logFile -Append
Write-Output "  Destino: $zipPath" | Tee-Object -FilePath $logFile -Append
