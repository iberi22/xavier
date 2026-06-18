# PowerShell script to index all OpenClaw agent configs into Xavier
$xavier = "E:\cortex\xavier\target\release\xavier.exe"
$agentDir = "C:\Users\belal\clawd\agents"

# Map of agents with their files
$agents = @{
    "developer" = @("AGENTS.md", "BOOTSTRAP.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
    "ghost" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
    "inventario" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SKILLS.md", "SOUL.md", "TOOLS.md", "USER.md")
    "sebastian-ceo" = @("MEMORY.md", "SOUL.md", "USER.md")
    "seo" = @("AGENTS.md", "BOOTSTRAP.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
    "trading" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SKILLS.md", "SOUL.md", "TOOLS.md", "USER.md")
    "ventas" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
    "ventas-leo" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
    "worldexams" = @("AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "MEMORY.md", "SOUL.md", "TOOLS.md", "USER.md")
}

foreach ($agent in $agents.Keys) {
    $agentPath = Join-Path $agentDir $agent
    if (Test-Path $agentPath) {
        foreach ($file in $agents[$agent]) {
            $filePath = Join-Path $agentPath $file
            if (Test-Path $filePath) {
                $content = Get-Content $filePath -Raw -ErrorAction SilentlyContinue
                if ($content.Length -gt 0) {
                    $trimmed = $content.Substring(0, [Math]::Min(200, $content.Length))
                    $trimmed = $trimmed.Replace('"', "'").Replace("`n", " ").Replace("`r", " ")
                    $title = "agent $agent $file"
                    $display = "$title $trimmed"
                    & $xavier add "$display" "$title" 2>&1 | Out-Null
                    Write-Host "Indexed: $agent/$file - $($content.Length) chars"
                }
            }
        }
    }
}

Write-Host "Done indexing all agents into Xavier!"
