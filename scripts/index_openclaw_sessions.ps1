# Index OpenClaw sessions into Xavier memory
# Runs from any directory, iterates all agent session files

$ErrorActionPreference = "Continue"
$XAVIER_URL = "http://localhost:8006"
$TOKEN = "dev-token"
$HEADERS = @{ "Content-Type" = "application/json"; "X-Xavier-Token" = $TOKEN }
$BATCH_SIZE = 5   # items per batch POST
$MAX_ITEMS_PER_FILE = 200  # max items to index per file (prevent overload)
$INTER_SESSION_DELAY_MS = 50

Write-Host "=== Xavier OpenClaw Session Indexer ==="
Write-Host "Target: $XAVIER_URL/memory/add"
Write-Host ""

# Verify Xavier is running
try {
    $health = Invoke-RestMethod -Uri "$XAVIER_URL/health" -Method Get -ErrorAction Stop
    Write-Host "✅ Xavier HEALTH OK" -ForegroundColor Green
} catch {
    Write-Host "❌ Xavier NOT RESPONDING at $XAVIER_URL" -ForegroundColor Red
    Write-Host "Start Xavier first!"
    exit 1
}

# Trace all sessions.json files
$agentDirs = @(
    "lasantacruz",
    "xavier",
    "main",
    "worldexams",
    "ventas",
    "pgheart",
    "inventario",
    "coder",
    "codex",
    "ghost"
)

$allStats = @{
    totalFiles = 0
    totalEntries = 0
    totalIndexed = 0
    totalFailed = 0
    totalSkipped = 0
    agents = @{}
}

$sessionFiles = @()
foreach ($agent in $agentDirs) {
    $sessionPath = "$env:USERPROFILE\.openclaw\agents\$agent\sessions\sessions.json"
    if (Test-Path $sessionPath) {
        $size = (Get-Item $sessionPath).Length
        $sessionFiles += @{
            agent = $agent
            path = $sessionPath
            sizeKB = [math]::Round($size / 1KB, 1)
        }
        Write-Host "📄 $agent : $($sizeKB)KB" -ForegroundColor Cyan
    } else {
        Write-Host "⚠️  No sessions for $agent" -ForegroundColor Yellow
    }
}

Write-Host "`nFound $($sessionFiles.Count) session files to process`n" -ForegroundColor Cyan

# Process each file
$globalIndexed = 0
$globalFailed = 0
$globalSkipped = 0

foreach ($sf in $sessionFiles) {
    $agentName = $sf.agent
    $filePath = $sf.path
    Write-Host "`n--- Processing agent: $agentName ---" -ForegroundColor Yellow

    try {
        $json = Get-Content $filePath -Raw -ErrorAction Stop
        $data = $json | ConvertFrom-Json -ErrorAction Stop
    } catch {
        Write-Host "  ❌ Failed to parse $filePath : $_" -ForegroundColor Red
        $globalFailed++
        continue
    }

    # Determine structure
    $entries = @()
    if ($data.PSObject.Properties.Name -contains "items") {
        $entries = $data.items
    } elseif ($data.PSObject.Properties.Name -contains "sessions") {
        $entries = $data.sessions
    } elseif ($data -is [System.Array]) {
        $entries = $data
    } else {
        $entries = @($data)
    }

    $count = [Math]::Min($entries.Count, $MAX_ITEMS_PER_FILE)
    Write-Host "  📊 $($entries.Count) total entries, indexing $count" -ForegroundColor Gray

    $agentTotal = 0
    $agentFailed = 0
    $agentSkipped = 0

    for ($i = 0; $i -lt $count; $i++) {
        $entry = $entries[$i]
        $content = ""

        # Extract content from various session formats
        if ($entry.content -and $entry.content -is [string]) {
            $content = $entry.content
        } elseif ($entry.messages) {
            # Format messages
            $msgs = @()
            foreach ($m in $entry.messages) {
                $role = $m.role
                $text = ""
                if ($m.content -is [string]) {
                    $text = $m.content
                } elseif ($m.content -is [System.Array]) {
                    $text = ($m.content | Where-Object { $_.type -eq "text" } | ForEach-Object { $_.text }) -join " "
                }
                if ($text -and $text.Length -gt 10 -and -not $text.StartsWith("/")) {
                    $msgs += "$role: $($text.Substring(0, [Math]::Min($text.Length, 500)))"
                }
            }
            if ($msgs.Count -gt 0) {
                $content = $msgs -join "`n"
            }
        } elseif ($entry.text) {
            $content = $entry.text
        } elseif ($entry.message) {
            $content = $entry.message
        } elseif ($entry.prompt) {
            $content = $entry.prompt
        } elseif ($entry.response) {
            $content = $entry.response
        } elseif ($entry.PSObject.Properties.Name -match "role|content") {
            # Could be a single message object
            $role = $entry.role
            $text = ""
            if ($entry.content -is [string]) {
                $text = $entry.content
            } elseif ($entry.content -is [System.Array]) {
                $text = ($entry.content | Where-Object { $_.type -eq "text" } | ForEach-Object { $_.text }) -join " "
            }
            if ($text) { $content = "$role: $text" }
        }

        # Skip empty or too short
        if ([string]::IsNullOrWhiteSpace($content) -or $content.Length -lt 50) {
            $agentSkipped++
            continue
        }

        # Build path
        $slug = if ($entry.id) { $entry.id.Substring(0, [Math]::Min($entry.id.Length, 20)) } else { "entry-$i" }
        $slug = $slug -replace '[^a-zA-Z0-9_-]', ''
        if ([string]::IsNullOrWhiteSpace($slug)) { $slug = "entry-$i" }

        $timestamp = if ($entry.timestamp -or $entry.created_at -or $entry.created) {
            ($entry.timestamp ?? $entry.created_at ?? $entry.created)
        } else {
            (Get-Date -Format "yyyy-MM-dd")
        }
        $dateStr = if ($timestamp -match '\d{4}-\d{2}-\d{2}') { $matches[0] } else { (Get-Date -Format "yyyy-MM-dd") }

        $path = "sessions/openclaw/$agentName/$dateStr/$slug"

        # Build tags
        $tags = @("openclaw", "session", $agentName)
        if ($content -match "data.+)commons") { $tags += "data-commons" }
        if ($content -match "wallet|token|blockchain|crypto") { $tags += "wallet" }
        if ($content -match "governance|vote|dao|proposal") { $tags += "governance" }
        if ($content -match "rust|compil|error|bug|fix") { $tags += "code" }
        if ($content -match "memory|rag|search|embed") { $tags += "memory" }
        if ($content -match "docker|deploy|devops") { $tags += "devops" }
        if ($content -match "api|endpoint|rest") { $tags += "api" }
        if ($content -match "config|configure|setting") { $tags += "config" }
        if ($content -match "pr|merge|commit|pull") { $tags += "git" }
        if ($content -match "benchmark|test|perf") { $tags += "testing" }

        $payload = @{
            path = $path
            content = $content.Substring(0, [Math]::Min($content.Length, 8000))
            metadata = @{
                agent_id = $agentName
                source = "openclaw"
                entry_type = "session"
                tags = $tags -join ","
                indexed_at = (Get-Date -Format "o")
                original_id = if ($entry.id) { $entry.id } else { "" }
            }
        }

        # POST to Xavier
        try {
            $resp = Invoke-RestMethod -Uri "$XAVIER_URL/memory/add" -Method Post -Body ($payload | ConvertTo-Json -Depth 5 -Compress) -ContentType "application/json" -Headers $HEADERS -ErrorAction Stop
            $agentTotal++
            $globalIndexed++
        } catch {
            Write-Host "    ❌ Error indexing entry $i : $_" -ForegroundColor DarkRed
            $agentFailed++
            $globalFailed++
        }

        # Progress every 20 items
        if ($i % 20 -eq 0 -and $i -gt 0) {
            Write-Host "  ⏳ $agentName: $i/$count indexed ($agentTotal OK, $agentFailed failed, $agentSkipped skipped)" -ForegroundColor Gray
        }

        # Small delay between items
        Start-Sleep -Milliseconds $INTER_SESSION_DELAY_MS
    }

    Write-Host "  ✅ $agentName done: $agentTotal indexed, $agentFailed failed, $agentSkipped skipped" -ForegroundColor Green
    $allStats.agents[$agentName] = @{ indexed = $agentTotal; failed = $agentFailed; skipped = $agentSkipped }
}

Write-Host "`n==============================" -ForegroundColor Cyan
Write-Host "INDEXING COMPLETE" -ForegroundColor Green
Write-Host "==============================" -ForegroundColor Cyan
Write-Host "Total files processed: $($sessionFiles.Count)"
Write-Host "Total indexed: $globalIndexed"
Write-Host "Total failed: $globalFailed"
Write-Host "Total skipped: $globalSkipped"

Write-Host "`nBreakdown by agent:"
foreach ($agent in $allStats.agents.Keys | Sort-Object) {
    $s = $allStats.agents[$agent]
    Write-Host "  $agent : $($s.indexed) indexed | $($s.failed) failed | $($s.skipped) skipped"
}
