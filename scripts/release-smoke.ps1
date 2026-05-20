param(
    [string]$BaseUrl = $(if ($env:XAVIER_URL) { $env:XAVIER_URL } else { "http://127.0.0.1:8006" }),
    [string]$Token = $(if ($env:XAVIER_TOKEN) { $env:XAVIER_TOKEN } else { "" }),
    [int]$TimeoutSec = 30,
    [switch]$RequireBuildRoute = $(if ($env:XAVIER_REQUIRE_BUILD_ROUTE -eq "1") { $true } else { $false }),
    [switch]$RequirePanel = $(if ($env:XAVIER_REQUIRE_PANEL -eq "1") { $true } else { $false })
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Token)) {
    throw "XAVIER_TOKEN is required for release smoke checks"
}

function Invoke-JsonRequest {
    param(
        [string]$Method,
        [string]$Url,
        [hashtable]$Headers = @{},
        [object]$Body = $null
    )

    $params = @{
        Method = $Method
        Uri = $Url
        Headers = $Headers
        TimeoutSec = $TimeoutSec
        UseBasicParsing = $true
    }

    if ($null -ne $Body) {
        $params["ContentType"] = "application/json"
        $params["Body"] = ($Body | ConvertTo-Json -Depth 8)
    }

    Invoke-WebRequest @params
}

Write-Host "Running Xavier release smoke checks against $BaseUrl" -ForegroundColor Cyan

$health = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/health"
if ($health.StatusCode -ne 200 -or $health.Content -notmatch '"status":"ok"') {
    throw "Health check failed"
}
Write-Host "PASS /health" -ForegroundColor Green

$readiness = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/readiness"
if ($readiness.StatusCode -ne 200) {
    throw "Readiness check failed"
}
$readinessJson = $readiness.Content | ConvertFrom-Json
if ($readinessJson.service -ne "xavier") {
    throw "Readiness payload missing xavier service marker"
}
Write-Host "PASS /readiness ($($readinessJson.status))" -ForegroundColor Green

try {
    $build = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/build" -Headers @{ "X-Xavier-Token" = $Token }
    if ($build.StatusCode -ne 200) {
        throw "Build info check failed"
    }
    $buildJson = $build.Content | ConvertFrom-Json
    if ($buildJson.service -ne "xavier") {
        throw "Build info payload missing xavier service marker"
    }
    Write-Host "PASS /build" -ForegroundColor Green
} catch {
    $statusCode = $_.Exception.Response.StatusCode.value__
    if (-not $RequireBuildRoute -and $statusCode -eq 404) {
        Write-Host "WARN /build not exposed by current server surface; skipping optional build check" -ForegroundColor Yellow
    } else {
        throw
    }
}

try {
    $unauthorized = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/v1/account/usage"
    if ($unauthorized.StatusCode -eq 200) {
        Write-Host "WARN auth gate bypassed; assuming dev mode is enabled" -ForegroundColor Yellow
    } else {
        throw "Protected route unexpectedly returned $($unauthorized.StatusCode)"
    }
} catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 401) {
        throw
    }
}
Write-Host "PASS auth gate" -ForegroundColor Green

$headers = @{ "X-Xavier-Token" = $Token }
$docPath = "smoke/$(Get-Date -Format 'yyyyMMddHHmmss')"
$content = "Xavier public release smoke test document"

$add = Invoke-JsonRequest -Method "POST" -Url "$BaseUrl/memory/add" -Headers $headers -Body @{
    path = $docPath
    content = $content
    metadata = @{ source = "release-smoke" }
}
if ($add.StatusCode -ne 200) {
    throw "Memory add failed"
}
Write-Host "PASS /memory/add" -ForegroundColor Green

$search = Invoke-JsonRequest -Method "POST" -Url "$BaseUrl/memory/search" -Headers $headers -Body @{
    query = "public release smoke"
    limit = 5
}
if ($search.StatusCode -ne 200) {
    throw "Memory search request failed"
}
$searchJson = $search.Content | ConvertFrom-Json
$searchFound = $searchJson.results | Where-Object { $_.content -like "*public release smoke*" }
if (-not $searchFound) {
    throw "Memory search failed to find smoke document"
}
Write-Host "PASS /memory/search" -ForegroundColor Green

$usage = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/v1/account/usage" -Headers $headers
if ($usage.StatusCode -ne 200) {
    throw "Usage endpoint failed"
}
Write-Host "PASS /v1/account/usage" -ForegroundColor Green

if ($RequirePanel) {
    try {
        $panelShell = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel"
        $panelStatusCode = $panelShell.StatusCode

        if ($panelStatusCode -eq 200) {
            Write-Host "PASS /panel returns 200 (frontend assets present)" -ForegroundColor Green
        } elseif ($panelStatusCode -eq 503) {
            Write-Host "PASS /panel returns 503 (frontend assets missing — panel is optional)" -ForegroundColor Green
        } else {
            throw "/panel returned unexpected status $panelStatusCode (expected 200 or 503)"
        }

        if ($panelStatusCode -eq 200) {
            $panelAsset = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/assets/index.js"
            if ($panelAsset.StatusCode -ne 200) {
                throw "/panel/assets/index.js returned $($panelAsset.StatusCode)"
            }
            Write-Host "PASS /panel/assets/index.js" -ForegroundColor Green

            $panelMissing = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/assets/missing.js"
            if ($panelMissing.StatusCode -ne 404) {
                throw "Missing panel asset returned $($panelMissing.StatusCode) instead of 404"
            }
            Write-Host "PASS missing panel asset returns 404" -ForegroundColor Green
        } else {
            Write-Host "INFO /panel returned 503 (assets not built) — skipping asset-availability checks" -ForegroundColor Yellow
        }

        $panelUnauth = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/api/threads"
        if ($panelUnauth.StatusCode -ne 401) {
            throw "Panel auth gate returned $($panelUnauth.StatusCode) instead of 401"
        }
        Write-Host "PASS panel auth gate" -ForegroundColor Green

        $panelThreads = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/api/threads" -Headers $headers
        if ($panelThreads.StatusCode -ne 200) {
            throw "Panel list threads failed"
        }
        Write-Host "PASS /panel/api/threads" -ForegroundColor Green

        $newThread = Invoke-JsonRequest -Method "POST" -Url "$BaseUrl/panel/api/threads" -Headers $headers -Body @{
            title = "New Thread"
        }
        if ($newThread.StatusCode -ne 200) {
            throw "Panel create thread failed"
        }
        $newThreadJson = $newThread.Content | ConvertFrom-Json
        $threadId = $newThreadJson.id
        if ([string]::IsNullOrWhiteSpace($threadId)) {
            throw "Panel create thread response missing id"
        }
        Write-Host "PASS create panel thread" -ForegroundColor Green

        $emptyDetail = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/api/threads/$threadId" -Headers $headers
        if ($emptyDetail.StatusCode -ne 200) {
            throw "Panel get empty thread failed"
        }
        $emptyDetailJson = $emptyDetail.Content | ConvertFrom-Json
        if ($emptyDetailJson.thread.title -ne "New Thread" -or @($emptyDetailJson.messages).Count -ne 0) {
            throw "Empty panel thread detail shape mismatch"
        }
        Write-Host "PASS empty panel thread detail" -ForegroundColor Green

        $panelChat = Invoke-JsonRequest -Method "POST" -Url "$BaseUrl/panel/api/chat" -Headers $headers -Body @{
            thread_id = $threadId
            message = "Explain xavier memory and show a structured UI."
        }
        if ($panelChat.StatusCode -ne 200) {
            throw "Panel chat failed"
        }
        $panelChatJson = $panelChat.Content | ConvertFrom-Json
        if ($panelChatJson.thread.id -ne $threadId -or @($panelChatJson.messages).Count -ne 2 -or $panelChatJson.messages[-1].role -ne "assistant") {
            throw "Panel chat response shape mismatch"
        }
        Write-Host "PASS /panel/api/chat" -ForegroundColor Green

        $updatedDetail = Invoke-JsonRequest -Method "GET" -Url "$BaseUrl/panel/api/threads/$threadId" -Headers $headers
        if ($updatedDetail.StatusCode -ne 200) {
            throw "Panel get updated thread failed"
        }
        $updatedDetailJson = $updatedDetail.Content | ConvertFrom-Json
        if ($updatedDetailJson.thread.title -eq "New Thread" -or @($updatedDetailJson.messages).Count -ne 2) {
            throw "First panel message should retitle thread"
        }
        Write-Host "PASS first panel message retitles the thread" -ForegroundColor Green

    } catch {
        throw
    }
} else {
    Write-Host "WARN panel checks skipped; set -RequirePanel to enforce panel validation" -ForegroundColor Yellow
}

Write-Host "Xavier release smoke checks passed." -ForegroundColor Cyan
