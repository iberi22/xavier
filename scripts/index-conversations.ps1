# Index CLI conversation histories into Xavier
# Extracts actual user/assistant messages from Codex and OpenClaw

param(
    [string]$XavierUrl = "http://localhost:8006",
    [string]$XavierToken = $env:XAVIER_TOKEN,
    [int]$MaxChunkSize = 15000,
    [int]$MaxSessionsPerTool = 10
)

if (-not $XavierToken) {
    if ($env:XAVIER_DEV_MODE -eq "true" -or $env:XAVIER_DEV_MODE -eq "1") {
        $XavierToken = "dev-token"
    } else {
        Write-Error "XAVIER_TOKEN environment variable is not set. For development, set XAVIER_DEV_MODE=true to use the default dev-token."
        exit 1
    }
}

$headers = @{
    "X-Xavier-Token" = $XavierToken
    "Content-Type" = "application/json"
}

$global:totalIndexed = 0
$global:totalBytes = 0

function Sanitize-Text($text) {
    $result = ""
    foreach ($char in $text.ToCharArray()) {
        $code = [int]$char
        if (($code -ge 0x20 -and $code -le 0x10FFFF) -or $code -eq 0x09 -or $code -eq 0x0A -or $code -eq 0x0D) {
            if ($code -lt 0xD800 -or $code -gt 0xDFFF) {
                $result += $char
            }
        }
    }
    return $result
}

function Send-Chunk($path, $content, $metadata) {
    $cleanContent = Sanitize-Text $content
    $body = @{ path = $path; content = $cleanContent; metadata = $metadata } | ConvertTo-Json -Depth 5 -Compress
    try {
        $resp = Invoke-RestMethod -Uri "$XavierUrl/memory/add" -Method POST -Headers $headers -Body $body -TimeoutSec 30
        if ($resp.status -eq "ok") {
            $global:totalIndexed++
            $global:totalBytes += $content.Length
            return $true
        }
    } catch { Write-Warning "Xavier send failed: $_" }
    return $false
}

function Chunk-and-Send($text, $basePath, $meta) {
    $chunks = @()
    $current = ""
    foreach ($line in ($text -split "`n")) {
        if (($current + $line).Length -gt $MaxChunkSize) {
            if ($current) { $chunks += $current }
            $current = $line + "`n"
        } else {
            $current += $line + "`n"
        }
    }
    if ($current) { $chunks += $current }

    for ($i = 0; $i -lt $chunks.Count; $i++) {
        $chunkMeta = $meta.Clone()
        $chunkMeta.chunk_index = $i
        $chunkMeta.total_chunks = $chunks.Count
        Send-Chunk "$basePath/chunk-$i" $chunks[$i] $chunkMeta
    }
}

# ==================== CODEX ====================
Write-Host "=== Indexing Codex conversations ===" -ForegroundColor Cyan
$codexDir = "C:\Users\belal\.codex\sessions"
if (Test-Path $codexDir) {
    $sessionFiles = Get-ChildItem $codexDir -Recurse -Filter "*.jsonl" | 
        Sort-Object LastWriteTime -Descending | 
        Select-Object -First $MaxSessionsPerTool
    
    foreach ($file in $sessionFiles) {
        Write-Host "Processing Codex: $($file.Name) ($([math]::Round($file.Length/1KB,0)) KB)" -ForegroundColor Gray
        $sessionText = ""
        $lineCount = 0
        Get-Content $file.FullName | ForEach-Object {
            try {
                $obj = $_ | ConvertFrom-Json
                if ($obj.type -eq "event_msg" -or $obj.type -eq "response_item") {
                    $p = $obj.payload
                    if ($p.type -eq "message" -and ($p.role -eq "user" -or $p.role -eq "assistant")) {
                        $text = ""
                        if ($p.content -and $p.content.GetType().Name -eq "Object[]") {
                            foreach ($c in $p.content) { if ($c.text) { $text += $c.text + " " } }
                        } elseif ($p.content -is [string]) {
                            $text = $p.content
                        }
                        if ($text.Length -gt 5) {
                            $sessionText += "[$($p.role)]: $text`n`n"
                            $lineCount++
                        }
                    }
                }
            } catch {}
        }
        
        if ($sessionText.Length -gt 200) {
            $meta = @{
                tool = "codex-cli"
                session = $file.Name
                date = $file.LastWriteTime.ToString("yyyy-MM-dd")
                lines = $lineCount
            }
            Chunk-and-Send $sessionText "cli-history/codex/$($file.BaseName)" $meta
            Write-Host "  Indexed $lineCount messages" -ForegroundColor Green
        }
    }
}

# ==================== OPENCLAW ====================
Write-Host "`n=== Indexing OpenClaw conversations ===" -ForegroundColor Cyan
$ocDir = "C:\Users\belal\.openclaw\agents\main\sessions"
if (Test-Path $ocDir) {
    $sessionFiles = Get-ChildItem $ocDir -Filter "*.jsonl" | 
        Where-Object { $_.Name -notlike "*.trajectory*" -and $_.Name -notlike "*.checkpoint*" } |
        Sort-Object LastWriteTime -Descending | 
        Select-Object -First $MaxSessionsPerTool
    
    foreach ($file in $sessionFiles) {
        Write-Host "Processing OpenClaw: $($file.Name) ($([math]::Round($file.Length/1KB,0)) KB)" -ForegroundColor Gray
        $sessionText = ""
        $msgCount = 0
        
        Get-Content $file.FullName | ForEach-Object {
            try {
                $obj = $_ | ConvertFrom-Json
                if ($obj.type -eq "prompt.submitted" -and $obj.data) {
                    if ($obj.data.prompt) {
                        $sessionText += "[user]: $($obj.data.prompt)`n`n"
                        $msgCount++
                    }
                }
                if ($obj.type -eq "model.completed" -and $obj.data) {
                    if ($obj.data.assistantTexts -and $obj.data.assistantTexts.Length -gt 0) {
                        foreach ($txt in $obj.data.assistantTexts) {
                            if ($txt.Length -gt 5) {
                                $sessionText += "[assistant]: $txt`n`n"
                                $msgCount++
                            }
                        }
                    }
                }
            } catch {}
        }
        
        if ($sessionText.Length -gt 200) {
            $meta = @{
                tool = "openclaw"
                session = $file.Name
                date = $file.LastWriteTime.ToString("yyyy-MM-dd")
                messages = $msgCount
            }
            Chunk-and-Send $sessionText "cli-history/openclaw/$($file.BaseName)" $meta
            Write-Host "  Indexed $msgCount messages" -ForegroundColor Green
        }
    }
}

# ==================== CLAUDE CODE ====================
Write-Host "`n=== Indexing Claude Code operations ===" -ForegroundColor Cyan
$claudeDir = "C:\Users\belal\.claude\projects"
if (Test-Path $claudeDir) {
    $files = Get-ChildItem $claudeDir -Recurse -Filter "*.jsonl" | 
        Sort-Object LastWriteTime -Descending | 
        Select-Object -First $MaxSessionsPerTool
    
    foreach ($file in $files) {
        $project = if ($file.FullName -match "projects\\([^\\]+)") { $matches[1] } else { "unknown" }
        $ops = @()
        Get-Content $file.FullName | ForEach-Object {
            try {
                $obj = $_ | ConvertFrom-Json
                if ($obj.content -and $obj.content.Length -gt 20) {
                    $ops += $obj.content
                }
            } catch {}
        }
        if ($ops.Count -gt 0) {
            $text = ($ops -join "`n---`n")
            if ($text.Length -gt 200) {
                $meta = @{
                    tool = "claude-code"
                    project = $project
                    date = $file.LastWriteTime.ToString("yyyy-MM-dd")
                    operations = $ops.Count
                }
                Chunk-and-Send $text "cli-history/claude/$project/$($file.BaseName)" $meta
                Write-Host "  Indexed $($ops.Count) operations from $project" -ForegroundColor Green
            }
        }
    }
}

Write-Host ""
Write-Host "=== INDEXING COMPLETE ===" -ForegroundColor Green
Write-Host "Total chunks indexed: $global:totalIndexed"
Write-Host "Total bytes indexed: $([math]::Round($global:totalBytes/1KB,1)) KB"

