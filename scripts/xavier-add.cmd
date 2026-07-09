@echo off
setlocal enabledelayedexpansion
set XPATH=%~1
set XCONTENT=%~2
set XAUTH=8ae8b432a2f42cffcdf26838d9646ab429ca6582f593af66bd42e61dab6991f7
echo {"path":"%XPATH%","content":"%XCONTENT%"} > "%TEMP%\xavier-payload.json"
curl -s -X POST http://localhost:8006/memory/add -H "Authorization: Bearer %XAUTH%" -H "Content-Type: application/json" -d @"%TEMP%\xavier-payload.json"
del "%TEMP%\xavier-payload.json" 2>nul
