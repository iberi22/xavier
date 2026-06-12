@echo off
set XAVIER_TOKEN=xavier-local-token
set XAVIER_EMBEDDING_URL=http://localhost:7999/v1/embeddings
set XAVIER_EMBEDDING_MODEL=BAAI/bge-large-en-v1.5
set XAVIER_EMBEDDING_DIMENSIONS=1024
set XAVIER_EMBEDDING_PROVIDER_MODE=local
set XAVIER_EMBEDDING_API_FLAVOR=openai
set XAVIER_DATA_DIR=E:\scripts-python\xavier\data
set XAVIER_MEMORY_SQLITE_PATH=E:\scripts-python\xavier\data\memory.db
set XAVIER_MEMORY_VEC_PATH=E:\scripts-python\xavier\data\memory_vec.db
set XAVIER_WORKSPACE_DIR=E:\scripts-python\xavier\data\workspaces
set XAVIER_CODE_GRAPH_DB_PATH=E:\scripts-python\xavier\data\code_graph.db
set XAVIER_HOST=127.0.0.1
set XAVIER_PORT=8006
set XAVIER_LOG_LEVEL=info
set XAVIER_EMBEDDING_CACHE_ENABLED=true
set XAVIER_EMBEDDING_CACHE_DB_PATH=E:\scripts-python\xavier\data\embedding-cache.db
set XAVIER_OPENAI_KEY=sk-or-***7d81
set TAVILY_API_KEY=tvly-d***7Hn5

ECHO Xavier native server starting on port 8006...
start /B cmd /c "E:\scripts-python\xavier\target\debug\xavier.exe http 8006 > E:\scripts-python\xavier\xavier-backend.log 2>&1"
ECHO Xavier PID: %ERRORLEVEL%

ECHO Starting Panel UI...
cd E:\scripts-python\xavier\panel-ui
start /B cmd /c "pnpm run dev > E:\scripts-python\xavier\xavier-frontend.log 2>&1"
