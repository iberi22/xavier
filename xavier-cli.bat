@echo off
REM Xavier CLI Wrapper — Sets token and URL automatically

set XAVIER_TOKEN=test-token-local-2026
set XAVIER_URL=http://127.0.0.1:8006

.\target\debug\xavier.exe %*
