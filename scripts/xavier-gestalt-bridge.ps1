<#
.SYNOPSIS
    Bridge entre Xavier y Gestalt Event Bus
    
.DESCRIPTION
    Permite a Gestalt llamar a Xavier y enviar eventos de estado al bus de Gestalt.
    Soporta comandos: chat, memory, llm, code-scan
    
.PARAMETER Command
    Comando de Xavier a ejecutar (chat, memory, llm, code-scan)
    
.PARAMETER Query
    Query o prompt para Xavier
    
.PARAMETER GestaltBusUrl
    URL del Event Bus de Gestalt (default: http://localhost:8081)
    
.PARAMETER Model
    Modelo LLM a usar (default: auto)
    
.EXAMPLE
    .\xavier-gestalt-bridge.ps1 -Command "chat" -Query "What embedding models does Xavier support?"
    
.EXAMPLE
    .\xavier-gestalt-bridge.ps1 -Command "memory" -Query "vector search" -GestaltBusUrl "http://localhost:9000"
#>

param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("chat", "memory", "llm", "code-scan")]
    [string]$Command,
    
    [Parameter(Mandatory=$true)]
    [string]$Query,
    
    [string]$GestaltBusUrl = "http://127.0.0.1:8081",
    
    [string]$Model = "auto"
)

# Configurar paths
$XavierBin = if (Test-Path (Join-Path $PSScriptRoot "..\target\release\xavier.exe")) {
    Join-Path $PSScriptRoot "..\target\release\xavier.exe"
} elseif (Test-Path (Join-Path $PSScriptRoot "..\target\release\xavier")) {
    Join-Path $PSScriptRoot "..\target\release\xavier"
} elseif (Get-Command xavier -ErrorAction SilentlyContinue) {
    (Get-Command xavier).Source
} else {
    "xavier"
}
$ErrorActionPreference = "Continue"

# Función para enviar eventos a Gestalt Bus
function Send-GestaltEvent {
    param(
        [string]$EventType,
        [string]$State,
        [string]$Message,
        [object]$Data = $null
    )
    
    $cleanMsg = $Message -replace "\\", "/"
    $payload = @{
        agent = "xavier"
        event_type = $EventType
        state = $State
        summary = $cleanMsg
        message = $cleanMsg
        ts = (Get-Date).ToUniversalTime().ToString("o")
        timestamp = (Get-Date).ToUniversalTime().ToString("o")
    }
    
    if ($Data) {
        $sanitizedData = @{}
        foreach ($k in $Data.Keys) {
            $v = $Data[$k]
            if ($v -is [string]) {
                $sanitizedData[$k] = $v -replace "\\", "/"
            } else {
                $sanitizedData[$k] = $v
            }
        }
        $payload.metadata = $sanitizedData
        $payload.data = $sanitizedData
    }
    
    try {
        $json = $payload | ConvertTo-Json -Depth 10
        Invoke-RestMethod -Method POST `
            -Uri "$GestaltBusUrl/api/event" `
            -Body $json `
            -ContentType "application/json" `
            -TimeoutSec 5 `
            -ErrorAction SilentlyContinue | Out-Null
    } catch {
        Write-Warning "Failed to send event to Gestalt bus: $_"
    }
}

# Verificar que Xavier existe
if (-not (Test-Path $XavierBin)) {
    $errorMsg = "Xavier binary not found at: $XavierBin`nPlease compile Xavier first: cargo build --release --features local-gllm"
    Write-Error $errorMsg
    Send-GestaltEvent -EventType "run_failed" -State "Failed" -Message $errorMsg
    exit 1
}

# Enviar evento de inicio
Send-GestaltEvent -EventType "run_started" -State "Running" -Message "Xavier executing: $Command with query: $Query"

try {
    # Ejecutar Xavier según el comando
    $result = switch ($Command) {
        "chat" {
            if ($Model -and $Model -ne "auto") {
                & $XavierBin chat $Query --model $Model 2>&1
            } else {
                & $XavierBin chat $Query 2>&1
            }
        }
        "memory" {
            & $XavierBin search $Query 2>&1
        }
        "llm" {
            & $XavierBin llm status 2>&1
        }
        "code-scan" {
            & $XavierBin code scan --path $Query 2>&1
        }
    }
    
    # Capturar exit code
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0) {
        # Éxito
        $cleanResult = ($result | Out-String) -replace "$([char]27)\[[0-9;]*[a-zA-Z]", ""
        $summaryMsg = "Xavier $Command finished successfully"
        Send-GestaltEvent -EventType "run_finished" -State "Completed" -Message $summaryMsg -Data @{
            command = $Command
            query = $Query
            exit_code = $exitCode
        }
        Write-Output $cleanResult
    } else {
        # Error
        $cleanError = ($result | Out-String) -replace "$([char]27)\[[0-9;]*[a-zA-Z]", ""
        $summaryMsg = "Xavier $Command failed with exit code $exitCode"
        Send-GestaltEvent -EventType "run_failed" -State "Failed" -Message $summaryMsg -Data @{
            command = $Command
            query = $Query
            exit_code = $exitCode
        }
        Write-Error $cleanError
        exit $exitCode
    }
    
} catch {
    $errorMsg = "Xavier execution failed: $_"
    Send-GestaltEvent -EventType "run_failed" -State "Failed" -Message $errorMsg
    Write-Error $errorMsg
    exit 1
}
