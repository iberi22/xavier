#!/usr/bin/env pwsh
# Xavier backup script.
# Creates a dated backup containing SQLite databases plus memory/ and config/.

param(
    [string]$BackupRoot = 'E:\backups\xavier',
    [int]$RetentionDays = 30
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir '..')).Path
$DateStamp = Get-Date -Format 'yyyy-MM-dd'
$BackupRootPath = [System.IO.Path]::GetFullPath($BackupRoot)
$Destination = Join-Path $BackupRootPath $DateStamp

function Get-RelativePath {
    param(
        [string]$BasePath,
        [string]$TargetPath
    )

    $base = [System.IO.Path]::GetFullPath($BasePath)
    if (-not $base.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
        $base = "$base$([System.IO.Path]::DirectorySeparatorChar)"
    }

    $baseUri = [System.Uri]::new($base)
    $targetUri = [System.Uri]::new([System.IO.Path]::GetFullPath($TargetPath))
    return [System.Uri]::UnescapeDataString(
        $baseUri.MakeRelativeUri($targetUri).ToString()
    ).Replace('/', [System.IO.Path]::DirectorySeparatorChar)
}

function Write-Info {
    param([string]$Message)
    Write-Host "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] $Message"
}

function Copy-DirectoryIfExists {
    param(
        [string]$Name,
        [string]$TargetRoot
    )

    $source = Join-Path $ProjectRoot $Name
    if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        Write-Info "Skipping missing directory: $Name"
        return
    }

    $target = Join-Path $TargetRoot $Name
    if (Test-Path -LiteralPath $target) {
        Remove-Item -LiteralPath $target -Recurse -Force
    }
    Copy-Item -LiteralPath $source -Destination $TargetRoot -Recurse -Force
    Write-Info "Copied directory: $Name"
}

function Copy-DatabaseFile {
    param(
        [System.IO.FileInfo]$File,
        [string]$TargetRoot
    )

    $relativePath = Get-RelativePath -BasePath $ProjectRoot -TargetPath $File.FullName
    $target = Join-Path (Join-Path $TargetRoot 'databases') $relativePath
    $targetDir = Split-Path -Parent $target

    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Copy-Item -LiteralPath $File.FullName -Destination $target -Force

    foreach ($suffix in @('-wal', '-shm')) {
        $sidecar = "$($File.FullName)$suffix"
        if (Test-Path -LiteralPath $sidecar -PathType Leaf) {
            Copy-Item -LiteralPath $sidecar -Destination "$target$suffix" -Force
        }
    }
}

New-Item -ItemType Directory -Path $Destination -Force | Out-Null
Write-Info "Backup destination: $Destination"

$databases = Get-ChildItem -Path $ProjectRoot -Filter '*.db' -File -Recurse |
    Where-Object {
        -not $_.FullName.StartsWith($BackupRootPath, [System.StringComparison]::OrdinalIgnoreCase) -and
        -not $_.FullName.StartsWith((Join-Path $ProjectRoot 'target'), [System.StringComparison]::OrdinalIgnoreCase)
    }

foreach ($database in $databases) {
    Copy-DatabaseFile -File $database -TargetRoot $Destination
}

Write-Info "Copied database files: $($databases.Count)"

Copy-DirectoryIfExists -Name 'memory' -TargetRoot $Destination
Copy-DirectoryIfExists -Name 'config' -TargetRoot $Destination

$cutoff = (Get-Date).AddDays(-$RetentionDays)
Get-ChildItem -Path $BackupRootPath -Directory -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Name -match '^\d{4}-\d{2}-\d{2}$' -and
        $_.LastWriteTime -lt $cutoff
    } |
    ForEach-Object {
        Write-Info "Removing expired backup: $($_.FullName)"
        Remove-Item -LiteralPath $_.FullName -Recurse -Force
    }

Write-Info "Backup completed."
