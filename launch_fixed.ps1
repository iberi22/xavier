# Load embedding config from .env instead of hardcoding keys here.
# The .env file is the single source of truth (managed by `xavier setup --auto`).
if (Test-Path "$PSScriptRoot\.env") {
    Get-Content "$PSScriptRoot\.env" | ForEach-Object {
        if ($_ -match '^\s*([A-Z_]+)\s*=\s*(.+)$') {
            $name = $Matches[1]; $value = $Matches[2].Trim('"').Trim("'")
            # Only set env vars that aren't already defined in the shell.
            if (-not [Environment]::GetEnvironmentVariable($name, 'Process')) {
                [Environment]::SetEnvironmentVariable($name, $value, 'Process')
            }
        }
    }
}

Start-Process -NoNewWindow -FilePath "E:\scripts-python\xavier\target\debug\xavier.exe" -ArgumentList "http" -RedirectStandardOutput "xavier-stdout.log" -RedirectStandardError "xavier-stderr.log"

Write-Host "Xavier launched with config from .env"
