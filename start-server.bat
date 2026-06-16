@echo off
REM Xavier HTTP Server Startup Script
REM Sets XAVIER_TOKEN and launches the HTTP server

set XAVIER_TOKEN=test-token-local-2026
set XAVIER_URL=http://127.0.0.1:8006
set XAVIER_EMBEDDING_PROVIDER_MODE=cloud
set XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1
set XAVIER_EMBEDDING_MODEL=openai/text-embedding-3-small
set OPENAI_API_KEY=***
set RUST_LOG=info

echo Starting Xavier HTTP Server...
echo Token: %XAVIER_TOKEN%
echo URL:   %XAVIER_URL%
echo.
.\target\debug\xavier.exe http 8006
