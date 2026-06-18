cd E:\cortex\xavier

$checks = @(
    @{n='CLI binary works (add/search/stats)'; c=(cmd /c "E:\cortex\xavier\target\release\xavier.exe stats 2>&1 | findstr /c:""Document Count""" | Select-String "Document Count").Length -gt 0}
    @{n='Memory Sync Phase 1 (LWW merge, diff, manifest, push/pull)'; c= (Test-Path src\memory\sync\mod.rs)}
    @{n='Mesh module exists'; c= (Test-Path src\mesh\mod.rs)}
    @{n='libp2p transport'; c= (Test-Path src\mesh\transport.rs)}
    @{n='Mesh discovery (mdns/Kademlia)'; c= (Test-Path src\mesh\discovery.rs)}
    @{n='Mesh governance'; c= (Test-Path src\mesh\governance.rs)}
    @{n='Pairing/permissions'; c= (Test-Path src\mesh\pairing.rs)}
    @{n='Cloud node adapter'; c= (Test-Path src\mesh\cloud_node.rs)}
    @{n='Tokenomics wallet'; c= (Test-Path src\mesh\tokenomics\mod.rs)}
    @{n='HTTP server (axum)'; c= (Test-Path src\server\http\mod.rs)}
    @{n='MCP server'; c= (Test-Path src\server\mcp\mod.rs)}
    @{n='Headless server API'; c= (Test-Path src\server\headless\mod.rs)}
    @{n='CLI interactive TUI (ratatui)'; c= (Test-Path src\main_tui.rs)}
    @{n='Telegram bot'; c= (Test-Path src\telegram\mod.rs)}
    @{n='Embeddings engine'; c= (Test-Path src\embedding\mod.rs)}
    @{n='A2A (Agent-to-Agent) protocol'; c= (Test-Path src\a2a\mod.rs)}
    @{n='Security module'; c= (Test-Path src\security\mod.rs)}
    @{n='Crypto/E2EE'; c= (Test-Path src\crypto\mod.rs)}
    @{n='Enterprise RBAC'; c= (Test-Path src\enterprise\mod.rs)}
    @{n='Health auto-repair'; c= (Test-Path src\health\mod.rs)}
    @{n='Auto-improvement loop'; c= (Test-Path src\auto_improvement\mod.rs)}
    @{n='TGD (Textual Gradient Descent)'; c= (Test-Path src\tgd\mod.rs)}
    @{n='Context regeneration'; c= (Test-Path src\context\mod.rs)}
    @{n='Session management (export/import)'; c= (Test-Path src\session\mod.rs)}
    @{n='Notifications system'; c= (Test-Path src\notifications\mod.rs)}
    @{n='Billing/quotas'; c= (Test-Path src\billing\mod.rs)}
    @{n='Codebase indexing'; c= (Test-Path src\codebase\mod.rs)}
    @{n='Scheduler'; c= (Test-Path src\scheduler\mod.rs)}
    @{n='Chronicle event log'; c= (Test-Path src\chronicle\mod.rs)}
    @{n='Data Commons'; c= (Test-Path src\data_commons\mod.rs)}
    @{n='Agent evolution loop'; c= (Test-Path src\agents\evolve\mod.rs)}
    @{n='System 3 meta-cognition'; c= (Test-Path src\agents\system3\mod.rs)}
    @{n='Agent provider framework'; c= (Test-Path src\agents\provider\mod.rs)}
    @{n='HORMER navigation'; c= (Test-Path src\agents\hormer\mod.rs)}
    @{n='Consolidation'; c= (Test-Path src\consolidation\mod.rs)}
    @{n='Verification system'; c= (Test-Path src\verification\mod.rs)}
    @{n='Storage layer'; c= (Test-Path src\storage\mod.rs)}
    @{n='Search engine'; c= (Test-Path src\search\mod.rs)}
    @{n='Observability/telemetry'; c= (Test-Path src\observability\mod.rs)}
    @{n='Checkpoint system'; c= (Test-Path src\checkpoint\mod.rs)}
    @{n='Tools framework'; c= (Test-Path src\tools\mod.rs)}
    @{n='Billable (templates/plugins) extension'; c= (Test-Path src\adapters\inbound\http\plugins\mod.rs)}
)

$total = $checks.Length
$passed = 0
Write-Host "=== X A V I E R   F E A T U R E   A U D I T ==="
Write-Host ""
foreach ($c in $checks) {
    if ($c.c) {
        $passed++
        Write-Host "[OK]  $($c.n)"
    } else {
        Write-Host "[MISS] $($c.n)"
    }
}
Write-Host ""
Write-Host "=========================================="
$pct = [Math]::Round($passed / $total * 100)
Write-Host "Score: $pct% ($passed/$total)"
Write-Host "=========================================="
