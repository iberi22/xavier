#!/usr/bin/env bash
# check-staged-scope.sh — impide que un commit de skills/docs revierta codigo sin querer.
#
# Origen: incidente real 2026-10-04. El commit e1b044c5
# ("fix(skills): sync two stale skill copies from the canonical Hermes tree")
# se declaro explicito como "No code touched, only skill documentation" y aun asi
# borro 111 lineas de src/health/mod.rs, revirtiendo por completo el fix de
# embedding_coverage que se habia commitado 30 segundos antes (04f4232a).
#
# Causa raiz: el agente hizo checkout/restore de ficheros completos
# (git checkout <commit> -- <path>) para sincronizar las copias de skills. Como
# src/health/mod.rs estaba modificado en el worktree, ese restore lo robo y lo
# revirto, y el mensaje del commit no lo delato.
#
# DECISION DE DISENO (medida, no supuesta): este gate NO lee el mensaje del
# commit. Durante pre-commit el mensaje llega vacio — medido en este repo:
# COMMIT_EDITMSG aun no lo contiene y `git log -1` devuelve el del commit
# anterior. Comparar texto era fragil y fallo tres veces seguidas. En vez de eso
# juzga solo lo que es verificable en ese momento: el contenido del staging.
#
# REGLA: si el commit toca ficheros que son SOLO documentacion/skills Y ademas
# toca codigo de producto (src/, crates/, code-graph/), se bloquea. Ese es el
# perfil exacto de un sync de skills que arrastra codigo.
#
# Precision > exhaustividad: un gate que siempre falla se bypassea y no protege
# nada. Por eso NO bloquea un commit de solo codigo ni uno de solo docs: solo el
# patron mixto docs+codigo.
#
#   bash scripts/check-staged-scope.sh
#
# Codigo de salida: 0 = ok, 1 = bloqueado.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "$ROOT" ]; then
    echo "check-staged-scope: no se ejecuta dentro de un repositorio git" >&2
    exit 1
fi
cd "$ROOT" || exit 1

STAGED="$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true)"
[ -n "$STAGED" ] || exit 0   # nada en staging: nada que juzgar

CODE_RE='^(src|crates|code-graph|codegraph)/'
CODE_TOUCHED="$(printf '%s\n' "$STAGED" | grep -E "$CODE_RE" || true)"
[ -n "$CODE_TOUCHED" ] || exit 0   # no hay codigo: nada que guardar

# Todo lo demas staged debe ser documentacion/skills; si hay algo mas, el commit
# es mixto de verdad y no es el perfil que queremos cazar.
NON_CODE="$(printf '%s\n' "$STAGED" | grep -vE "$CODE_RE" || true)"
[ -n "$NON_CODE" ] || exit 0        # solo codigo: commit legitimo

DOC_RE='^(skills|docs|\.agents)/|\.(md|markdown|mdx|txt|rst)$'
for f in $NON_CODE; do
    printf '%s' "$f" | grep -qE "$DOC_RE" || exit 0   # algo no es doc: no es nuestro caso
done

{
echo "❌ [SWAL] ALCANCE INCONSISTENTE: commit de documentacion/skills tocando codigo."
echo ""
echo "   Este commit mezcla solo-docs (skills/, *.md, docs/) con codigo de producto"
echo "   (src/, crates/, code-graph/). Ese es el perfil exacto del incidente del"
echo "   2026-10-04: e1b044c5 se declaro \"No code touched, only skill documentation\""
echo "   y revirtio 111 lineas de src/health/mod.rs, deshaciendo el fix de"
echo "   embedding_coverage de 04f4232a (30 s antes). La causa fue un restore de"
echo "   fichero completo (git checkout <commit> -- <path>) con src/ modificado en el"
echo "   worktree."
echo ""
echo "   Que hacer:"
echo "     - Si el codigo NO es intencional:"
echo "         git restore --staged <archivo-de-codigo>"
echo "       y vuelve a stagear solo la documentacion."
echo "     - Si el codigo SI es intencional, separalo en dos commits:"
echo "         uno de docs/skills y otro de codigo, cada uno con su mensaje."
echo ""
echo "   Codigo en staging:";    printf '%s\n' "$CODE_TOUCHED" | sed 's/^/     - /'
echo "   Docs/skills en staging:"; printf '%s\n' "$NON_CODE"    | sed 's/^/     - /'
echo ""
} >&2
exit 1
