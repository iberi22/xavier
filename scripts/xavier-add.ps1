param(
  [Parameter(Mandatory=$true)]
  [string]$Path,
  [Parameter(Mandatory=$true)]
  [string]$Content
)

$AuthToken = "8ae8b432a2f42cffcdf26838d9646ab429ca6582f593af66bd42e61dab6991f7"
$Payload = @{ path = $Path; content = $Content } | ConvertTo-Json -Compress
$TempFile = Join-Path $env:TEMP "xavier-payload.json"
$Payload | Out-File -FilePath $TempFile -Encoding ascii -NoNewline

try {
  $Response = Invoke-RestMethod -Uri "http://localhost:8006/memory/add" `
    -Method Post `
    -Headers @{ "Authorization" = "Bearer $AuthToken" } `
    -ContentType "application/json" `
    -InFile $TempFile
  Write-Host ($Response | ConvertTo-Json -Compress)
} catch {
  Write-Error "XAVIER_ERROR: $_"
} finally {
  Remove-Item $TempFile -ErrorAction SilentlyContinue
}
