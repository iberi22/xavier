@echo off
REM ============================================
REM  XAVIER v0.12.0 - Windows Launcher
REM  Panel UI: http://localhost:8006/panel
REM  Health:   http://localhost:8006/health
REM ============================================
echo.
echo === Xavier Cognitive Memory Runtime ===
echo.

set XAVIER_DATA_DIR=E:\scripts-python\xavier\data
set XAVIER_TOKEN=dev-token
set XAVIER_LOG_LEVEL=info
set XAVIER_EMBEDDING_PROVIDER_MODE=cloud

echo Starting Xavier HTTP server...
echo Panel: http://localhost:8006/panel
echo.

if not exist "%XAVIER_DATA_DIR%" mkdir "%XAVIER_DATA_DIR%"

start "Xavier Server" /B "E:\scripts-python\xavier\target\release\xavier.exe" http 8006

echo Xavier started! Opening panel...
timeout /t 5 /nobreak >nul
start http://localhost:8006/panel

echo.
echo Press any key to stop Xavier...
pause >nul
echo Shutting down...
taskkill /f /im xavier.exe 2>nul
echo Done.
