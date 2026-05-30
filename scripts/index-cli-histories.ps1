# Index CLI Assistant Histories into Xavier
# Processes conversation histories and sends to Xavier memory

param(
    [string]$XavierUrl = "http://localhost:8006",
    [string]$XavierToken = $env:XAVIER_TOKEN,
    [int]$MaxDaysBack = 30,
    [int]$MaxChunkSize = 8000,
    [int]$MaxFilesPerTool = 50
)

if (-not $XavierToken) {
    Write-Error "XAVIER_TOKEN environment variable is not set. A secure token is required for all operations."
    exit 1
}

$headers = @{
    "X-Xavier-Token" = $XavierToken
    "Content-Type" = "application/json"
}

$cutoffDate = (Get-Date).AddDays(-$MaxDaysBack)
$totalIndexed = 0
$totalChunks = 0

function Send-ToXavier($path, $content, $metadata) {
    $body = @{
        path = $path
        content = $content
        metadata = $metadata
    } | ConvertTo-Json -Depth 5 -Compress
    
    try {
        $resp = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 30
        return $resp.status -eq "ok"
    } catch {
        Write-Warning "Failed to send to Xavier: $_"
        return $false
    }
}

function Extract-TextFromClaude($filePath) {
    $texts = @()
    Get-Content $filePath | ForEach-Object {
        try {
            $obj = $_ | ConvertFrom-Json
            if ($obj.display) { $texts += $obj.display }
            if ($obj.text) { $texts += $obj.text }
            if ($obj.content -and $obj.content.text) { $texts += $obj.content.text }
        } catch {}
    }
    return $texts -join "\n"
}

function Extract-TextFromCodex($filePath) {
    $texts = @()
    Get-Content $filePath | ForEach-Object {
        try {
            $obj = $_ | ConvertFrom-Json
            if ($obj.text) { $texts += $obj.text }
            if ($obj.message -and $obj.message.content) { $texts += $obj.message.content }
        } catch {}
    }
    return $texts -join "\n"
}

function Extract-TextFromOpenClaw($filePath) {
    $texts = @()
    Get-Content $filePath | ForEach-Object {
        try {
            $obj = $_ | ConvertFrom-Json
            if ($obj.role -and ($obj.role -eq "user" -or $obj.role -eq "assistant")) {
                if ($obj.content) { $texts += $obj.content }
            }
            if ($obj.text) { $texts += $obj.text }
        } catch {}
    }
    return $texts -join "\n"
}

function Chunk-Text($text, $maxSize) {
    $chunks = @()
    $lines = $text -split "`n"
    $current = ""
    foreach ($line in $lines) {
        if ($current.Length + $line.Length + 1 -gt $maxSize) {
            $chunks += $current
            $current = $line
        } else {
            $current += $line + "`n"
        }
    }
    if ($current) { $chunks += $current }
    return $chunks
}

# ==================== CLAUDE CODE ====================
Write-Host "=== Processing Claude Code histories ===" -ForegroundColor Cyan
$claudeFiles = Get-ChildItem "C:\Users\belal\.claude" -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | 
    Where-Object { $_.LastWriteTime -gt $cutoffDate } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First $MaxFilesPerTool

$claudeProjectChunks = @{}
foreach ($file in $claudeFiles) {
    $project = if ($file.FullName -match "projects\\([^\\]+)") { $matches[1] } else { "unknown" }
    $text = Extract-TextFromClaude $file.FullName
    if ($text.Length -gt 100) {
        if (-not $claudeProjectChunks[$project]) { $claudeProjectChunks[$project] = "" }
        $claudeProjectChunks[$project] += "`n--- SESSION $($file.Name) ---`n$text"
    }
}

foreach ($proj in $claudeProjectChunks.Keys) {
    $content = $claudeProjectChunks[$proj]
    if ($content.Length -gt $MaxChunkSize) { $content = $content.Substring(0, $MaxChunkSize) + "... [truncated]" }
    $chunks = Chunk-Text $content $MaxChunkSize
    for ($i = 0; $i -lt $chunks.Count; $i++) {
        $path = "cli-history/claude/$proj/chunk-$i"
        $meta = @{
            tool = "claude-code"
            project = $proj
            chunk_index = $i
            total_chunks = $chunks.Count
            indexed_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ss")
        }
        if (Send-ToXavier $path $chunks[$i] $meta) {
            $totalIndexed++
        }
        $totalChunks++
    }
}
Write-Host "Claude: $totalIndexed chunks indexed" -ForegroundColor Green

# ==================== CODEX ====================
Write-Host "`n=== Processing Codex histories ===" -ForegroundColor Cyan
$codexFiles = Get-ChildItem "C:\Users\belal\.codex" -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | 
    Where-Object { $_.LastWriteTime -gt $cutoffDate } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First $MaxFilesPerTool

$codexChunks = @()
foreach ($file in $codexFiles) {
    $text = Extract-TextFromCodex $file.FullName
    if ($text.Length -gt 100) {
        $codexChunks += $text
    }
}

$combined = ($codexChunks -join "\n---\n")
if ($combined.Length -gt 0) {
    if ($combined.Length -gt $MaxChunkSize) { $combined = $combined.Substring(0, $MaxChunkSize) + "... [truncated]" }
    $chunks = Chunk-Text $combined $MaxChunkSize
    for ($i = 0; $i -lt $chunks.Count; $i++) {
        $path = "cli-history/codex/chunk-$i"
        $meta = @{
            tool = "codex-cli"
            chunk_index = $i
            total_chunks = $chunks.Count
            indexed_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ss")
        }
        if (Send-ToXavier $path $chunks[$i] $meta) {
            $totalIndexed++
        }
        $totalChunks++
    }
}
Write-Host "Codex: $totalIndexed chunks indexed" -ForegroundColor Green

# ==================== OPENCLAW ====================
Write-Host "`n=== Processing OpenClaw histories ===" -ForegroundColor Cyan
$ocFiles = Get-ChildItem "C:\Users\belal\.openclaw" -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | 
    Where-Object { $_.LastWriteTime -gt $cutoffDate } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First $MaxFilesPerTool

$ocChunks = @()
foreach ($file in $ocFiles) {
    $text = Extract-TextFromOpenClaw $file.FullName
    if ($text.Length -gt 100) {
        $ocChunks += $text
    }
}

$combined = ($ocChunks -join "\n---\n")
if ($combined.Length -gt 0) {
    if ($combined.Length -gt $MaxChunkSize) { $combined = $combined.Substring(0, $MaxChunkSize) + "... [truncated]" }
    $chunks = Chunk-Text $combined $MaxChunkSize
    for ($i = 0; $i -lt $chunks.Count; $i++) {
        $path = "cli-history/openclaw/chunk-$i"
        $meta = @{
            tool = "openclaw"
            chunk_index = $i
            total_chunks = $chunks.Count
            indexed_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ss")
        }
        if (Send-ToXavier $path $chunks[$i] $meta) {
            $totalIndexed++
        }
        $totalChunks++
    }
}
Write-Host "OpenClaw: $totalIndexed chunks indexed" -ForegroundColor Green

# ==================== SUMMARY ====================
Write-Host ""
Write-Host "=== INDEXING COMPLETE ===" -ForegroundColor Green
Write-Host "Total chunks sent: $totalChunks"
Write-Host "Successfully indexed: $totalIndexed"

