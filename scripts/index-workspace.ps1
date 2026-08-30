# Xavier Workspace Indexer - Index all agents into Xavier memory
# Usage: powershell -ExecutionPolicy Bypass .\index-workspace.ps1

param(
    [string]$XavierBin = "E:\scripts-python\xavier\target\release\xavier.exe",
    [string]$WorkspaceDir = "C:\Users\belal\clawd\agents",
    [string]$XavierUrl = "http://localhost:8006",
    [string]$Token = "0ca58c895bca2b08bffd7a548c97ef3d4c78c37e94bd566caf34f4b65c179fc7"
)

$ErrorActionPreference = "Continue"

function Add-Memory {
    param([string]$Content, [string]$Title, [string]$Kind="episodic", [string]$Tags="")
    
    $result = & $XavierBin add $Content $Title --kind $kind 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0 -or $result -match "Error") {
        Write-Warning "Failed to add: $Title"
        return $false
    }
    return $true
}

# Phase 1: Index agent personas (SOUL.md, USER.md, IDENTITY.md, MEMORY.md)
Write-Host "=== PHASE 1: Agent Personas ===" -ForegroundColor Cyan

$agents = Get-ChildItem $WorkspaceDir -Directory | Where-Object { 
    $_.Name -notin @('claude','coder','codex','developer','ghost','opencode-go','opencode-go-kimi-k2-5','sebastian-ceo')
} | Select-Object -ExpandProperty Name

foreach ($agent in $agents) {
    $agentDir = Join-Path $WorkspaceDir $agent
    
    # SOUL.md - Agent identity
    $soulPath = Join-Path $agentDir "SOUL.md"
    if (Test-Path $soulPath) {
        $content = Get-Content $soulPath -Raw
        Add-Memory -Content "===== AGENT: $agent =====`n$content" -Title "$agent SOUL.md" -Kind "semantic" -Tags "$agent,identity,prompt"
        Write-Host "  ✓ Indexed $agent/SOUL.md"
    }
    
    # USER.md - User profile
    $userPath = Join-Path $agentDir "USER.md"
    if (Test-Path $userPath) {
        $content = Get-Content $userPath -Raw
        Add-Memory -Content "===== AGENT USER: $agent =====`n$content" -Title "$agent USER.md" -Kind "semantic" -Tags "$agent,user,profile"
        Write-Host "  ✓ Indexed $agent/USER.md"
    }
    
    # IDENTITY.md
    $identityPath = Join-Path $agentDir "IDENTITY.md"
    if (Test-Path $identityPath) {
        $content = Get-Content $identityPath -Raw
        Add-Memory -Content "===== AGENT IDENTITY: $agent =====`n$content" -Title "$agent IDENTITY.md" -Kind "semantic" -Tags "$agent,identity"
        Write-Host "  ✓ Indexed $agent/IDENTITY.md"
    }
    
    # MEMORY.md - Long term memory
    $memoryPath = Join-Path $agentDir "MEMORY.md"
    if (Test-Path $memoryPath) {
        $content = Get-Content $memoryPath -Raw
        Add-Memory -Content "===== AGENT MEMORY: $agent =====`n$content" -Title "$agent MEMORY.md" -Kind "semantic" -Tags "$agent,memory,longterm"
        Write-Host "  ✓ Indexed $agent/MEMORY.md"
    }
    
    # HEARTBEAT.md - Task configuration
    $heartbeatPath = Join-Path $agentDir "HEARTBEAT.md"
    if (Test-Path $heartbeatPath) {
        $content = Get-Content $heartbeatPath -Raw
        Add-Memory -Content "===== AGENT TASKS: $agent =====`n$content" -Title "$agent HEARTBEAT.md" -Kind "procedural" -Tags "$agent,tasks,heartbeat"
        Write-Host "  ✓ Indexed $agent/HEARTBEAT.md"
    }
    
    # TOOLS.md - Local notes
    $toolsPath = Join-Path $agentDir "TOOLS.md"
    if (Test-Path $toolsPath) {
        $content = Get-Content $toolsPath -Raw
        Add-Memory -Content "===== AGENT TOOLS: $agent =====`n$content" -Title "$agent TOOLS.md" -Kind "procedural" -Tags "$agent,tools,config"
        Write-Host "  ✓ Indexed $agent/TOOLS.md"
    }
}

# Phase 2: Index daily memory files
Write-Host "`n=== PHASE 2: Daily Memory Files ===" -ForegroundColor Cyan

foreach ($agent in $agents) {
    $memoryDir = Join-Path (Join-Path $WorkspaceDir $agent) "memory"
    if (Test-Path $memoryDir) {
        $files = Get-ChildItem $memoryDir -Filter "*.md" | Sort-Object Name -Descending | Select-Object -First 50
        foreach ($file in $files) {
            $content = Get-Content $file.FullName -Raw
            $title = "$agent $($file.BaseName)"
            Add-Memory -Content "===== AGENT: $agent, DATE: $($file.BaseName) =====`n$content" -Title $title -Kind "episodic" -Tags "$agent,memory,daily"
            Write-Host "  ✓ Indexed $title"
        }
    }
}

# Phase 3: Index skills
Write-Host "`n=== PHASE 3: Skills ===" -ForegroundColor Cyan

foreach ($agent in $agents) {
    $skillDir = Join-Path (Join-Path $WorkspaceDir $agent) "skills"
    if (Test-Path $skillDir) {
        Get-ChildItem $skillDir -Recurse -Filter "*.md" | ForEach-Object {
            $content = Get-Content $_.FullName -Raw
            $relPath = $_.FullName.Substring($skillDir.Length + 1)
            Add-Memory -Content "===== SKILL: $agent/$relPath =====`n$content" -Title "$agent skill $relPath" -Kind "procedural" -Tags "$agent,skill,$relPath"
        }
        Write-Host "  ✓ Indexed skills for $agent"
    }
}

Write-Host "`n=== DONE ===" -ForegroundColor Green
Write-Host "Run: & $XavierBin search 'query' to verify indexing"
