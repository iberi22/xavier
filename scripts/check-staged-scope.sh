#!/usr/bin/env bash
# check-staged-scope.sh — impide que un commit de skills/docs revierta codigo sin querer.
#
# Origen: incidente real 2026-10-04. El commit e1b044c5
# ("fix(skills): sync two stale skill copies from the canonical Hermes tree")
# se declaro explicito como "No code touched, only skill documentation" y aun asi
# borro 111 lineas de src/health/mod.rs, revirtiendo por completo el fix de
# embedding_coverage que se habia commitado 30 segundos antes (04f4232a).
#
# Causa raiz: el agente hizo checkout/restore de ficheros completos (git checkout
# <commit> -- <path>) para sincronizar las copias de skills. Como src/health/mod.rs
# estaba modificado en el worktree, ese restore lo robo y lo revirto.
#
# Este gate no juzga si el cambio de codigo es BUENO. Solo exige que el mensaje del
# commit sea HONESTO: si dice skills/docs y hay src/ en staging, se bloquea.
# Precision > exhaustividad: un gate que siempre falla se bypassea y no protege nada.
#
#   bash scripts/check-staged-scope.sh            # staged contra HEAD
#   bash scripts/check-staged-scope.sh --message "texto"   # ademas valida el mensaje
#
# Codigo de salida: 0 = ok, 1 = bloqueado.
set -uo pipefail

MSG=""
# Acepta tanto `--message "texto"` como `--message="texto"`. El bucle `for` de
# arriba consumia el valor del primer caso y lo descartaba (MSG quedaba vacio),
# asi que el guard nunca comparaba contra el mensaje real.
while [ $# -gt 0 ]; do
    case "$1" in
        --message)
            if [ -n "${2:-}" ]; then MSG="$2"; shift 2; else shift; fi
            ;;
        --message=*)
            MSG="${1#--message=}"
            shift
            ;;
        *)
            shift
            ;;
    esac
done

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "$ROOT" ]; then
    echo "check-staged-scope: no se ejecuta dentro de un repositorio git" >&2
    exit 1
fi
cd "$ROOT" || exit 1

STAGED="$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true)"
[ -n "$STAGED" ] || exit 0   # nada en staging: nada que juzgar

# ¿El staging es solo documentacion/skills/config sin codigo de producto?
CODE_TOUCHED="$(printf '%s\n' "$STAGED" | grep -E '^(src|crates|code-graph|codegraph)/' || true)"
DOC_ONLY="$(printf '%s\n' "$STAGED" \
    | grep -vE '^(src|crates|code-graph|codegraph)/' \
    | grep -E '\.(md|markdown|txt|json|ya?ml|toml)$' || true)"

[ -n "$CODE_TOUCHED" ] || exit 0   # no hay codigo: nada que guardar

# --- Regla ---------------------------------------------------------------
# Si hay codigo de producto en staging Y el mensaje declara que no lo hay,
# el commit miente. No importa el balance docs/codigo: la condicion es
# "hay src/ staged" + "el mensaje dice que no". El conteo DOC==TOTAL nunca
# se cumplia en el incidente real (1 doc + 1 codigo = 2 ficheros) y por eso
# el gate no disparaba.
if [ -n "$CODE_TOUCHED" ]; then
    if printf '%s' "$MSG" | grep -qiE '(no code|only .*(skill|doc)|solo .*(skill|doc)|skills? only|sin codigo|documentacion unicamente)'; then
        echo "" >&2
        echo "❌ [SWAL] ALCANCE INCONSISTENTE: el mensaje dice 'sin codigo' pero hay src/ en staging." >&2
        echo "" >&2
        echo "   Mensaje : ${MSG:0:200}" >&2
        echo "   Codigo en staging:" >&2
        printf '%s\n' "$CODE_TOUCHED" | sed 's/^/     - /' >&2
        echo "" >&2
        echo "   Esto ya paso: e1b044c5 revirtio 111 lineas de src/health/mod.rs" >&2
        echo "   mientras se declaraba 'No code touched, only skill documentation'." >&2
        echo "" >&2
        echo "   Si el cambio de codigo ES intencional, corrige el mensaje del commit." >&2
        echo "   Si NO es intencional, unstage el codigo:" >&2
        echo "     git restore --staged <archivo>" >&2
        echo "" >&2
        exit 1
    fi
fi

exit 0
