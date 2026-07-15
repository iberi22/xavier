#!/bin/bash
# codegraph-self-scan.sh
# Escanea el codebase completo de Xavier con code-graph y guarda resultados en .xavier/
# 
# FIX v3: DB movida a ~/.xavier/ (ext4) para evitar SQLite/NTFS "disk I/O error"
#         Escaneo desde raíz del repo (no por subdirectorios)
#
# Uso:
#   ./scripts/codegraph-self-scan.sh            # escaneo normal
#   ./scripts/codegraph-self-scan.sh --save     # escaneo + snapshot a .xavier/

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CODEGRAPH_BIN="$REPO_ROOT/target/release/code-graph"
XAVIER_DIR="${HOME}/.xavier"
DB_PATH="$XAVIER_DIR/code_graph.db"
XAVIER_DIR_REPO="$REPO_ROOT/.xavier"
SNAPSHOT_FILE="$XAVIER_DIR_REPO/codegraph.json"
COMMIT_HASH=$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo "unknown")
BRANCH=$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%S+00:00")

echo "=== code-graph: Auto-escaneo de Xavier ==="
echo "Repo:     $REPO_ROOT"
echo "Branch:   $BRANCH"
echo "Commit:   $COMMIT_HASH"
echo "DB:       $DB_PATH"
echo ""

# Verificar binario
if [ ! -f "$CODEGRAPH_BIN" ]; then
    echo "⚠️  code-graph binario no encontrado. Compilando..."
    cd "$REPO_ROOT/code-graph"
    cargo build --release 2>&1
    echo "✅ Build completo"
fi

# Asegurar que ~/.xavier/ existe
mkdir -p "$XAVIER_DIR"

# Escaneo completo desde la raíz del repo
# El tool ignora target/, .git/ automáticamente
echo "🔍 Escaneando codebase completo de Xavier..."
cd "$REPO_ROOT"
"$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT" 2>&1 || {
    echo "⚠️  scan del repo completo falló (exit=$?)"
    echo "   Debug: intentar escaneo por subdirectorios"
    "$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT/src" 2>&1
    "$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT/scripts" 2>&1
    "$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT/code-graph/src" 2>&1
    "$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT/tests" 2>&1
    "$CODEGRAPH_BIN" --db-path "$DB_PATH" scan --no-incremental "$REPO_ROOT/benches" 2>&1
}

# Estadísticas
echo ""
echo "📊 Estadísticas:"
"$CODEGRAPH_BIN" --db-path "$DB_PATH" stats 2>&1 || echo "  (stats no disponible)"

# Guardar snapshot en .xavier/ si se solicita --save
if [ "$1" == "--save" ]; then
    mkdir -p "$XAVIER_DIR_REPO"
    
    # Extraer stats del output de stats
    FILES=$("$CODEGRAPH_BIN" --db-path "$DB_PATH" stats 2>/dev/null | grep "Files:" | grep -oP '\d+')
    SYMBOLS=$("$CODEGRAPH_BIN" --db-path "$DB_PATH" stats 2>/dev/null | grep "Symbols:" | grep -oP '\d+')
    IMPORTS=$("$CODEGRAPH_BIN" --db-path "$DB_PATH" stats 2>/dev/null | grep "Imports:" | grep -oP '\d+')
    
    cat > "$SNAPSHOT_FILE" << SNAPSHOT
{
  "_meta": {
    "repo": "xavier",
    "branch": "$BRANCH",
    "commit": "$COMMIT_HASH",
    "scanned_at": "$TIMESTAMP",
    "version": "3.0",
    "engine": "code-graph v0.6.1-beta",
    "db_location": "~/.xavier/code_graph.db"
  },
  "stats": {
    "files": ${FILES:-0},
    "symbols": ${SYMBOLS:-0},
    "imports": ${IMPORTS:-0}
  },
  "note": "Full repo scan from root. DB stored on ext4 (~/.xavier/) para evitar SQLite/NTFS issues."
}
SNAPSHOT
    
    echo ""
    echo "✅ Snapshot guardado en: $SNAPSHOT_FILE"
    
    # También guardar snapshot por commit para historial
    COMMIT_SNAPSHOT="$XAVIER_DIR_REPO/codegraph-${COMMIT_HASH:0:12}.json"
    cp "$SNAPSHOT_FILE" "$COMMIT_SNAPSHOT" 2>/dev/null || true
    echo "📁 Snapshot por commit: $COMMIT_SNAPSHOT"
fi

echo ""
echo "✅ Auto-escaneo completado."
echo "   DB:    $DB_PATH (ext4, versionado en .gitignore)"
echo "   Snapshot: .xavier/codegraph.json (versionado en git)"
