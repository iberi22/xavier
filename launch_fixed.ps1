$env:XAVIER_EMBEDDING_PROVIDER_MODE="cloud"
$env:XAVIER_EMBEDDING_API_KEY="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"
$env:XAVIER_EMBEDDING_URL="https://openrouter.ai/api/v1/embeddings"
$env:XAVIER_EMBEDDING_MODEL="text-embedding-3-small"

Start-Process -NoNewWindow -FilePath "E:\scripts-python\xavier\target\debug\xavier.exe" -ArgumentList "http" -RedirectStandardOutput "xavier-stdout.log" -RedirectStandardError "xavier-stderr.log"

Write-Host "Xavier launched with cloud embedding config"
