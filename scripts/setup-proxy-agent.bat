@echo off
REM scripts/setup-proxy-agent.bat
REM Sets up OpenCode and Claude Code to use Xavier as a proxy with a leased API key.

setlocal enabledelayedexpansion

set XAVIER_BIN=xavier
set TTL=86400
set PROXY_URL=http://localhost:8006/v1
set MODEL=glm-5.2

echo --- Xavier Proxy Setup for OpenCode ^& Claude Code ---

echo ^>^> Requesting secret lease from Xavier...

REM Check if xavier is in PATH
where %XAVIER_BIN% >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo ^>^> Xavier not in PATH, trying cargo run...
    if exist "Cargo.toml" (
        for /f "tokens=*" %%i in ('cargo run --quiet --bin xavier -- secrets lend ZAI_API_KEY opencode --ttl %TTL%') do (
            echo %%i | findstr "Lease Token:" >nul
            if !ERRORLEVEL! EQU 0 (
                set OUTPUT=%%i
            )
        )
    ) else (
        echo Error: Xavier binary not found and no Cargo.toml in current directory.
        exit /b 1
    )
) else (
    for /f "tokens=*" %%i in ('%XAVIER_BIN% secrets lend ZAI_API_KEY opencode --ttl %TTL%') do (
        echo %%i | findstr "Lease Token:" >nul
        if !ERRORLEVEL! EQU 0 (
            set OUTPUT=%%i
        )
    )
)

if "%OUTPUT%"=="" (
    echo Error: Failed to obtain Lease Token from Xavier.
    echo Make sure Xavier is running and ZAI_API_KEY is set in the vault.
    exit /b 1
)

REM Extract token from "Lease Token: "b5c1a9d6...""
for /f "tokens=2 delims=:" %%a in ("%OUTPUT%") do (
    set RAW_TOKEN=%%a
    set TOKEN=!RAW_TOKEN:"=!
    set TOKEN=!TOKEN: =!
)

echo ^>^> Obtained Lease Token: %TOKEN%

REM Configure OpenCode
set OPENCODE_DIR=%USERPROFILE%\.config\opencode
if not exist "%OPENCODE_DIR%" mkdir "%OPENCODE_DIR%"
set OPENCODE_CONFIG=%OPENCODE_DIR%\config.json

echo {> "%OPENCODE_CONFIG%"
echo   "base_url": "%PROXY_URL%",>> "%OPENCODE_CONFIG%"
echo   "api_key": "%TOKEN%",>> "%OPENCODE_CONFIG%"
echo   "model": "%MODEL%">> "%OPENCODE_CONFIG%"
echo }>> "%OPENCODE_CONFIG%"
echo ^>^> Configured OpenCode at %OPENCODE_CONFIG%

REM Configure Claude Code
set CLAUDE_DIR=%USERPROFILE%\.claude
if not exist "%CLAUDE_DIR%" mkdir "%CLAUDE_DIR%"
set CLAUDE_CONFIG=%CLAUDE_DIR%\settings.json

echo {> "%CLAUDE_CONFIG%"
echo   "apiBaseUrl": "%PROXY_URL%",>> "%CLAUDE_CONFIG%"
echo   "apiKey": "%TOKEN%",>> "%CLAUDE_CONFIG%"
echo   "model": "%MODEL%">> "%CLAUDE_CONFIG%"
echo }>> "%CLAUDE_CONFIG%"
echo ^>^> Configured Claude Code at %CLAUDE_CONFIG%

echo.
echo --- Setup Complete ---
echo The agents are now configured to use Xavier proxy.
echo The lease will expire in 24 hours.
echo.
echo To revoke this lease manually, run:
echo   xavier secrets revoke %TOKEN%
echo.

endlocal
