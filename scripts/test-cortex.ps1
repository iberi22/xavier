# ╔══════════════════════════════════════════════════════════════╗
# ║  DEPRECATED — CORTEX REMOVED (May 2026)                    ║
# ║  This script tested connectivity to Cortex (formerly on     ║
# ║  port 8003). Cortex has been fully removed from the SWAL   ║
# ║  stack. Xavier (the only memory service) now runs on       ║
# ║  port 8006. Use test-xavier.ps1 or similar tools.          ║
# ╚══════════════════════════════════════════════════════════════╝

$body = @{query="SWAL"; limit=5} | ConvertTo-Json -Compress
$temp = [System.IO.Path]::GetTempFileName() + ".json"
[System.IO.File]::WriteAllText($temp, $body)
$TOKEN = $env:XAVIER_TOKEN
if (-not $TOKEN) { $TOKEN = $env:XAVIER_API_KEY }
if (-not $TOKEN) { $TOKEN = $env:XAVIER_TOKEN }
if (-not $TOKEN) {
    throw "Missing Xavier token. Set XAVIER_TOKEN, XAVIER_API_KEY, or XAVIER_TOKEN."
}
curl.exe -s -X POST "http://localhost:8006/memory/search" -H "X-Xavier-Token: $TOKEN" -H "Content-Type: application/json" --data-binary "@$temp"
Remove-Item $temp
