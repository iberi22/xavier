# start-xavier-rag.ps1
# Script de arranque rápido para Xavier RAG Backend

$XAVIER_VERSION = "0.11.0"
$PORT = 8006
$TOKEN = [Guid]::NewGuid().ToString()

Write-Host "🚀 Iniciando Xavier v$XAVIER_VERSION RAG Backend..." -ForegroundColor Cyan

# Verificación de requisitos
if (-not (Get-Command "xavier" -ErrorAction SilentlyContinue)) {
    if (Test-Path "./xavier.exe") {
        $XAVIER_BIN = "./xavier.exe"
    } elseif (Test-Path "./target/release/xavier.exe") {
        $XAVIER_BIN = "./target/release/xavier.exe"
    } else {
        Write-Host "❌ Error: No se encontró el binario 'xavier'. Por favor, instálalo o compílalo primero." -ForegroundColor Red
        exit 1
    }
} else {
    $XAVIER_BIN = "xavier"
}

# Configuración de entorno
$env:XAVIER_TOKEN = $TOKEN
$env:XAVIER_EMBEDDING_PROVIDER_MODE = "local-gllm"
$env:XAVIER_GLLM_MODEL = "all-mpnet-base-v2"

Write-Host "🔑 Token de seguridad: $TOKEN" -ForegroundColor Yellow
Write-Host "🧠 Modo de embeddings: Local GLLM (all-mpnet-base-v2)" -ForegroundColor Cyan

# Lanzar servidor en segundo plano
Write-Host "🌐 Servidor escuchando en http://localhost:$PORT" -ForegroundColor Green
Start-Process $XAVIER_BIN -ArgumentList "http", $PORT -NoNewWindow

# Esperar a que el servidor esté listo
Write-Host "⏳ Esperando a que el sistema esté listo..." -ForegroundColor Gray
$maxRetries = 30
$retryCount = 0
$ready = $false

while (-not $ready -and $retryCount -lt $maxRetries) {
    try {
        $resp = Invoke-RestMethod -Uri "http://localhost:$PORT/v1/health/ready" -ErrorAction SilentlyContinue
        if ($resp.status -eq "ok") {
            $ready = $true
        }
    } catch {
        # Ignorar errores de conexión mientras arranca
    }
    if (-not $ready) {
        Start-Sleep -Seconds 1
        $retryCount++
    }
}

if ($ready) {
    Write-Host "✅ Xavier está listo!" -ForegroundColor Green
    Write-Host "📊 Panel de Control: http://localhost:$PORT/panel" -ForegroundColor Cyan
    Write-Host "🛠️ MCP Endpoint: http://localhost:$PORT/mcp" -ForegroundColor Cyan

    # Abrir panel web si es posible
    # Start-Process "http://localhost:$PORT/panel"
} else {
    Write-Host "⚠️ El servidor tardó demasiado en responder. Revisa los logs." -ForegroundColor Yellow
}
