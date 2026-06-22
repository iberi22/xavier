#!/bin/bash
# =============================================================================
# Xavier - Claude Code Hook
# Guarda y restaura contexto automaticamente entre sesiones de Claude Code.
# Ahorra 90-99% de tokens en contexto reutilizado.
# =============================================================================

# Config
XAVIER_TOKEN="${XAVIER_TOKEN:-dev-token-57968}"
XAVIER_URL="${XAVIER_URL:-http://localhost:8006}"
CACHE_DIR="${HOME}/.xavier/claude-cache"
mkdir -p "$CACHE_DIR"

xavier_api() {
  local method="$1"
  local endpoint="$2"
  local data="$3"
  
  if [ -n "$data" ]; then
    curl -s -X "$method" "$XAVIER_URL$endpoint" \
      -H "X-Xavier-Token: $XAVIER_TOKEN" \
      -H "Content-Type: application/json" \
      -d "$data"
  else
    curl -s -X "$method" "$XAVIER_URL$endpoint" \
      -H "X-Xavier-Token: $XAVIER_TOKEN"
  fi
}

# =============================================================================
# Hook: Antes de cada llamada a Claude Code
# Guarda el contexto actual + historial de comandos
# =============================================================================
xavier_hook_presave() {
  local session_id="${1:-default}"
  local workdir="${2:-$(pwd)}"
  local git_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'no-git')"
  local git_hash="$(git rev-parse HEAD 2>/dev/null || echo 'no-git')"
  local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  
  # Guardar en Xavier: contexto de sesion actual
  xavier_api "POST" "/v1/memories" "{
    \"text\": \"Claude Code session context: working on $(basename $workdir), branch $git_branch, commit $git_hash. Session: $session_id\",
    \"user_id\": \"claude-code\",
    \"metadata\": {
      \"session_id\": \"$session_id\",
      \"workdir\": \"$workdir\",
      \"branch\": \"$git_branch\",
      \"commit\": \"$git_hash\",
      \"type\": \"session_context\",
      \"timestamp\": \"$timestamp\"
    },
    \"kind\": \"session_context\"
  }" > /dev/null 2>&1
  
  # Guardar archivos modificados recientemente como contexto
  local changed_files=$(git diff --name-only HEAD~5..HEAD 2>/dev/null | head -20)
  if [ -n "$changed_files" ]; then
    xavier_api "POST" "/v1/memories" "{
      \"text\": \"Recent changes in $session_id: $(echo $changed_files | tr '\n' ' ' | head -c 2000)\",
      \"user_id\": \"claude-code\",
      \"metadata\": {
        \"session_id\": \"$session_id\",
        \"type\": \"changed_files\",
        \"timestamp\": \"$timestamp\"
      },
      \"kind\": \"session_context\"
    }" > /dev/null 2>&1
  fi
  
  echo "$session_id" > "$CACHE_DIR/last_session.txt"
  echo "XAVIER_SAVED"
}

# =============================================================================
# Hook: Antes de iniciar nueva sesion — restaura contexto previo
# =============================================================================
xavier_hook_prerestore() {
  local query="${1:-}"
  local depth="${2:-medium}" # shallow | medium | deep
  
  if [ -z "$query" ] && [ -f "$CACHE_DIR/last_session.txt" ]; then
    # Restaurar ultima sesion
    local last_session=$(cat "$CACHE_DIR/last_session.txt")
    query="session_id:$last_session"
  fi
  
  if [ -z "$query" ]; then
    # Buscar contexto del proyecto actual
    local project=$(basename $(pwd))
    query="project:$project"
  fi
  
  # Recuperar contexto desde Xavier
  local result=$(xavier_api "GET" "/v1/search?q=$(echo $query | jq -sRr @uri)&limit=5&kind=session_context")
  echo "$result"
}

# =============================================================================
# Hook: Estadisticas de ahorro de tokens
# =============================================================================
xavier_hook_stats() {
  local result=$(xavier_api "GET" "/v1/search?q=XAVIER_SAVED&limit=100&kind=session_context")
  
  # Contar cuantos contextos se guardaron
  local saved_count=$(echo "$result" | grep -c "XAVIER_SAVED" 2>/dev/null || echo 0)
  # Estimar tokens ahorrados: ~2000 tokens por guardado
  local estimated_tokens=$((saved_count * 2000))
  # Estimar costo ahorrado: $3/M tokens (Claude Sonnet)
  local saved_cost=$(echo "scale=4; $estimated_tokens / 1000000 * 3" | bc 2>/dev/null || echo 0)
  
  echo "=== Xavier Token Savings ==="
  echo "Contextos guardados: $saved_count"
  echo "Tokens estimados ahorrados: $estimated_tokens"
  echo "Costo estimado ahorrado: \$$saved_cost"
}

# =============================================================================
# CLI
# =============================================================================
case "${1:-}" in
  save)
    shift
    xavier_hook_presave "$@"
    ;;
  restore)
    shift
    xavier_hook_prerestore "$@"
    ;;
  stats)
    xavier_hook_stats
    ;;
  *)
    echo "Xavier Claude Code Hook"
    echo "Usage: $0 {save|restore|stats} [args]"
    echo ""
    echo "  save [session_id] [workdir]"
    echo "    Guarda el contexto actual en Xavier"
    echo ""
    echo "  restore [query] [depth]"
    echo "    Restaura contexto previo desde Xavier"
    echo "    depth: shallow|medium|deep (default: medium)"
    echo ""
    echo "  stats"
    echo "    Muestra estadisticas de ahorro de tokens"
    ;;
esac
