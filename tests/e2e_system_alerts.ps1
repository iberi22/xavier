$ErrorActionPreference = "Stop"

Write-Host "Starting E2E System Alerts Test..." -ForegroundColor Cyan

# 1. Force the embedding backend to fail by specifying a nonexistent model
$env:XAVIER_EMBEDDING_PROVIDER_MODE = "local-gllm"
$env:XAVIER_EMBEDDING_MODEL = "un_modelo_inexistente_12345"
# Avoid standard fallback
$env:XAVIER_EMBEDDING_URL = ""
$env:OPENAI_API_KEY = ""

# 2. Build the backend if not built, or use the last build
# Just checking if cargo build works, but to save time we run using `cargo run` or start the binary.
# Since the server needs to compile, let's run `cargo run --bin xavier` in the background.
# Alternatively we can start it with `Start-Process`.

Write-Host "Building and starting Xavier backend (this may take a moment)..." -ForegroundColor Yellow

$processInfo = New-Object System.Diagnostics.ProcessStartInfo
$processInfo.FileName = "cargo"
$processInfo.Arguments = "run --release --bin xavier --features `"local-gllm,cli-interactive`" --no-default-features"
$processInfo.RedirectStandardOutput = $true
$processInfo.RedirectStandardError = $true
$processInfo.UseShellExecute = $false
$processInfo.CreateNoWindow = $true

$process = New-Object System.Diagnostics.Process
$process.StartInfo = $processInfo
$process.Start() | Out-Null

$serverPid = $process.Id
Write-Host "Xavier started with PID: $serverPid"

try {
    # 3. Poll /system/alerts up to 10 times
    $maxAttempts = 15
    $attempt = 1
    $success = $false

    while ($attempt -le $maxAttempts) {
        Start-Sleep -Seconds 2
        Write-Host "Polling /system/alerts (Attempt $attempt of $maxAttempts)..."

        try {
            # Try to fetch alerts
            $response = Invoke-RestMethod -Uri "http://127.0.0.1:8006/system/alerts" -Method Get -ErrorAction Stop
            
            if ($response.alerts.Count -gt 0) {
                # 4. Analyze the JSON result
                $foundEmbeddingError = $false
                foreach ($alert in $response.alerts) {
                    if ($alert.message -match "Embedding backend unavailable" -or $alert.message -match "embedding backend unavailable") {
                        $foundEmbeddingError = $true
                        Write-Host "Expected alert found: $($alert.message)" -ForegroundColor Green
                        break
                    }
                }

                if ($foundEmbeddingError) {
                    $success = $true
                    break
                }
            }
        } catch {
            Write-Host "Server not ready yet or endpoint failed." -ForegroundColor Gray
        }

        $attempt++
    }

    if ($success) {
        Write-Host "E2E TEST PASS: System alerts functionality is working correctly." -ForegroundColor Green
    } else {
        Write-Host "E2E TEST FAIL: Could not find the expected embedding error alert." -ForegroundColor Red
        exit 1
    }

} finally {
    # 5. Clean up the process
    Write-Host "Stopping Xavier process (PID: $serverPid)..."
    try {
        Stop-Process -Id $serverPid -Force -ErrorAction SilentlyContinue
    } catch {
        Write-Host "Failed to kill process $serverPid" -ForegroundColor Yellow
    }
}
