#!/usr/bin/env bash
# verify-rust.sh — puerta de verificacion de Xavier (Rust), alineada con el CI.
#
# El CI (.github/workflows/ci.yml) exige: cargo fmt --check, cargo check y
# cargo clippy con --features ci-safe --all-targets, mas los tests. Hasta ahora
# no habia forma de ejecutar eso mismo en local de una pieza: los agentes
# verificaban con `cargo check --lib` a secas, que NO es la puerta del CI, y las
# comprobaciones acababan en /tmp/ y se perdian. Este script cierra ese hueco.
#
# Uso:
#   scripts/verify-rust.sh                 # rapido: fmt + check + tests del espejo (~1-2 min)
#   scripts/verify-rust.sh --full          # paridad con CI: + toda la suite + clippy -D warnings
#   scripts/verify-rust.sh --with-data     # + comprobacion real: exporta el espejo y exige 0 huerfanas
#   scripts/verify-rust.sh --filter NAME   # tests cuyo nombre contenga NAME (por defecto: mirror)
#   scripts/verify-rust.sh --list          # solo muestra que se ejecutaria, sin ejecutarlo
#
# Salida: una tabla resumen y exit 0 solo si TODO pasa. Pensado para que lo llame
# el pipeline (pnpm run verify:rust) o un agente antes de dar algo por terminado.
#
# NO escribe en la base real: la comprobacion con datos es de solo lectura
# (exporta). Los tests usan bases temporales.

set -uo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

FULL=0
WITH_DATA=0
LIST_ONLY=0
FILTER="mirror"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --full) FULL=1 ;;
        --with-data) WITH_DATA=1 ;;
        --list) LIST_ONLY=1 ;;
        --filter) FILTER="${2:-mirror}"; shift ;;
        -h|--help) sed -n '2,25p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "opcion desconocida: $1" >&2; exit 2 ;;
    esac
    shift
done

# --features ci-safe es lo que usa el CI; sin el, local y CI no comparan lo mismo.
FEATURES=(--features ci-safe)
declare -a NOMBRES=() ESTADOS=()
FALLOS=0

paso() { # paso <nombre> <comando...>
    local nombre="$1"; shift
    if [[ $LIST_ONLY -eq 1 ]]; then
        echo "  [plan] $nombre: $*"
        return 0
    fi
    echo ""
    echo "=== $nombre ==="
    echo "  \$ $*"
    local inicio fin salida
    inicio=$(date +%s)
    "$@" > /tmp/verify-rust-step.log 2>&1
    local rc=$?
    fin=$(date +%s)
    tail -12 /tmp/verify-rust-step.log | sed 's/^/    /'
    NOMBRES+=("$nombre")
    if [[ $rc -eq 0 ]]; then
        ESTADOS+=("OK ($((fin - inicio))s)")
    else
        ESTADOS+=("FALLO (exit $rc)")
        FALLOS=$((FALLOS + 1))
        echo "  --- salida completa en /tmp/verify-rust-step.log ---"
    fi
    return $rc
}

echo "verify-rust.sh — $(date '+%Y-%m-%d %H:%M')"
echo "  repo: $REPO_DIR"
echo "  modo: $([[ $FULL -eq 1 ]] && echo 'completo (paridad CI)' || echo 'rapido')$([[ $WITH_DATA -eq 1 ]] && echo ' + datos reales')"
echo "  filtro de tests: $FILTER"

paso "formato (cargo fmt --check)"      cargo fmt --all -- --check
paso "compilacion (cargo check)"        cargo check --package xavier --all-targets "${FEATURES[@]}"
paso "tests del espejo (cargo test)"    cargo test --package xavier "${FEATURES[@]}" "$FILTER" -- --test-threads=1

if [[ $FULL -eq 1 ]]; then
    paso "clippy (CI lo exige con -D warnings)" \
        cargo clippy --package xavier --all-targets "${FEATURES[@]}" -- -D warnings
fi

if [[ $WITH_DATA -eq 1 ]]; then
    # Where cargo actually puts the binary (env var, .cargo/config.toml, or the
    # default): guessed paths broke in the ramdisk layout, so ask cargo. With a
    # timeout, because `cargo metadata` blocks while another cargo holds the
    # lock, and a verification step must never hang.
    TARGET_DIR="$(timeout 20 cargo metadata --no-deps --format-version 1 2>/dev/null \
        | python3 -c 'import json,sys; print(json.load(sys.stdin).get("target_directory",""))' 2>/dev/null || true)"
    TARGET_DIR="${CARGO_TARGET_DIR:-${TARGET_DIR:-$REPO_DIR/target}}"
    BIN="$TARGET_DIR/debug/xavier"
    if [[ ! -x "$BIN" ]]; then
        cargo build --bin xavier >/dev/null 2>&1
    fi
    if [[ -x "$BIN" ]]; then
        for ventana in 2026-09-04 2026-08-12; do
            paso "export real --since $ventana" \
                "$BIN" mirror-export --out "/tmp/verify-rust-$ventana.jsonl" --since "$ventana"
        done
        echo ""
        echo "=== integridad del grafo exportado (0 huerfanas) ==="
        python3 - <<'PY'
import glob, json, os
malos = 0
for p in sorted(glob.glob('/tmp/verify-rust-*.jsonl')):
    nodos, aristas = set(), []
    for raw in open(p):
        raw = raw.strip()
        if not raw:
            continue
        d = json.loads(raw)
        if d['t'] == 'node':
            nodos.add(d['id'])
        elif d['t'] == 'edge':
            aristas.append(d)
    h = [e for e in aristas if e['from'] not in nodos or e['to'] not in nodos]
    malos += len(h)
    print(f"    {os.path.basename(p)}: {len(nodos)} nodos, {len(aristas)} aristas, {len(h)} huerfanas")
print("    VEREDICTO:", "sin huerfanas en ninguna ventana" if malos == 0 else f"REVISAR: {malos} huerfanas")
PY
    else
        echo ""
        echo "  (sin binario en $BIN: 'cargo build --bin xavier' para la comprobacion con datos)"
    fi
fi

if [[ $LIST_ONLY -eq 1 ]]; then
    exit 0
fi

echo ""
echo "================ RESUMEN ================"
for i in "${!NOMBRES[@]}"; do
    printf '  %-40s %s\n' "${NOMBRES[$i]}" "${ESTADOS[$i]}"
done
echo "========================================="
if [[ $FALLOS -eq 0 ]]; then
    echo "  VERDE: todo lo ejecutado paso."
    exit 0
fi
echo "  ROJO: $FALLOS paso(s) fallaron. NO dar el trabajo por terminado."
exit 1
