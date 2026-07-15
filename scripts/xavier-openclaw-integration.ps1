# =============================================================================
# xavier-openclaw-integration.ps1
# Sincroniza memorias de agentes OpenClaw con Xavier
# Corre cada hora via cron
# =============================================================================

$ErrorActionPreference = "Continue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$XavierRoot = Resolve-Path "$ScriptDir\.."
$LogDir = "$XavierRoot\data\logs"
$null = New-Item -ItemType Directory -Force -Path $LogDir
$LogFile = "$LogDir\openclaw-sync.log"
$Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

function Log {
    param([string]$Msg, [string]$Level = "INFO")
    $line = "[$Timestamp] [$Level] $Msg"
    Add-Content -Path $LogFile -Value $line
    Write-Host $line
}

Log "=== OpenClaw Xavier Sync START ==="

# Verificar que Xavier esta corriendo
try {
    $ready = Invoke-RestMethod -Uri "http://localhost:8006/v1/health/ready" -UseBasicParsing -TimeoutSec 5 -ErrorAction SilentlyContinue
    if (-not ($ready.status -eq "ok" -or $ready.status -eq "unhealthy")) {
        Log "Xavier no responde en puerto 8006" "WARN"
        Log "=== OpenClaw Xavier Sync END (Xavier offline) ==="
        exit 0
    }
} catch {
    Log "Xavier no esta corriendo en localhost:8006" "WARN"
    Log "=== OpenClaw Xavier Sync END (Xavier offline) ==="
    exit 0
}

Log "Xavier OK en puerto 8006"

# Token
$token = $env:XAVIER_TOKEN
if (-not $token) { $token = $env:XAVIER_API_KEY }
if (-not $token) { $token = "" }
if ($token) { Log "Token configurado" } else { Log "Sin token - modo sin auth" "WARN" }

# HEADERS
$headers = @{ "Content-Type" = "application/json" }
if ($token) { $headers["X-Xavier-Token"] = $token }

# AGENTES a sincronizar
$agents = @(
    @{ Name = "lasantacruz"; Dir = "C:\Users\belal\clawd\agents\lasantacruz" }
)

$totalSynced = 0

foreach ($agent in $agents) {
    $agentName = $agent.Name
    $agentDir = $agent.Dir
    Log "Procesando agente: $agentName"

    # --- 1. MEMORY.md ---
    $memoryFile = "$agentDir\MEMORY.md"
    if (Test-Path $memoryFile) {
        try {
            $content = Get-Content $memoryFile -Raw
            if ($content.Length -gt 4000) { $content = $content.Substring(0, 4000) }
            $body = @{
                content = $content
                path = "openclaw/agent/$agentName/memory-md"
                metadata = @{
                    type = "memory-md"
                    source = "openclaw"
                    agent = $agentName
                    synced_at = (Get-Date -Format "o")
                }
            } | ConvertTo-Json -Compress

            $resp = Invoke-RestMethod -Uri "http://localhost:8006/memory/add" -Method Post -Headers $headers -Body $body -UseBasicParsing -TimeoutSec 10
            Log "  MEMORY.md synced"
            $totalSynced++
        } catch {
            Log "  MEMORY.md failed: $_" "WARN"
        }
    }

    # --- 2. Daily memory files (ultimos 5) ---
    $memoryDir = "$agentDir\memory"
    if (Test-Path $memoryDir) {
        $dailyFiles = Get-ChildItem $memoryDir -Filter "*.md" | Sort-Object Name -Descending | Select-Object -First 5
        foreach ($file in $dailyFiles) {
            try {
                if ($file.Length -gt 200KB) {
                    Log "  SKIP ${file.Name} (>200KB)"
                    continue
                }
                $content = Get-Content $file.FullName -Raw
                if ($content.Length -gt 5000) { $content = $content.Substring(0, 5000) }
                $date = $file.BaseName
                $body = @{
                    content = $content
                    path = "openclaw/agent/$agentName/daily/$date"
                    metadata = @{
                        type = "daily-memory"
                        source = "openclaw"
                        agent = $agentName
                        date = $date
                        synced_at = (Get-Date -Format "o")
                    }
                } | ConvertTo-Json -Compress

                Invoke-RestMethod -Uri "http://localhost:8006/memory/add" -Method Post -Headers $headers -Body $body -UseBasicParsing -TimeoutSec 10
                $totalSynced++
            } catch {
                Log "  ${file.Name} failed: $_" "WARN"
            }
            Start-Sleep -Milliseconds 200
        }
        Log "  Daily files: $($dailyFiles.Count) processed"
    }

    # --- 3. SOUL.md ---
    $soulFile = "$agentDir\SOUL.md"
    if (Test-Path $soulFile) {
        try {
            $content = Get-Content $soulFile -Raw
            if ($content.Length -gt 2000) { $content = $content.Substring(0, 2000) }
            $body = @{
                content = $content
                path = "openclaw/agent/$agentName/soul"
                metadata = @{
                    type = "agent-soul"
                    source = "openclaw"
                    agent = $agentName
                    synced_at = (Get-Date -Format "o")
                }
            } | ConvertTo-Json -Compress

            Invoke-RestMethod -Uri "http://localhost:8006/memory/add" -Method Post -Headers $headers -Body $body -UseBasicParsing -TimeoutSec 10
            Log "  SOUL.md synced"
            $totalSynced++
        } catch {
            Log "  SOUL.md failed: $_" "WARN"
        }
    }
}

Log "=== OpenClaw Xavier Sync END - Total: $totalSynced items ==="
Write-Host "Total items synced: $totalSynced"
