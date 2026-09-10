#!/usr/bin/env bash
# check-strategy-leak.sh — impide volver a publicar estrategia/economía/PII del ecosistema SWAL.
#
# Contexto: auditoría 2026-09-10 encontró el whitepaper tokenómico (XAV), PRICING ($499/$1,999),
# gobernanza interna (70/20/5/5), el modelo económico del marketplace ($SWAL 90/10), roadmaps
# internos y PII (emails + rutas /home/belal) versionados en ESTE repo público (iberi22/xavier).
# Este gate bloquea la reincidencia. NO relajar sin ADR.
#
# Uso:  bash scripts/check-strategy-leak.sh            # revisa TODO lo trackeado
#       bash scripts/check-strategy-leak.sh --staged   # solo lo que entra al commit (hook)
set -uo pipefail

MODE="${1:-all}"
fail=0

if [[ "$MODE" == "--staged" ]]; then
  files=$(git diff --cached --name-only --diff-filter=ACMR)
else
  files=$(git ls-files)
fi
[[ -z "$files" ]] && { echo "check-strategy-leak: nada que revisar"; exit 0; }

# ── 1. Nombres de archivo que NO deben publicarse ────────────────────────────
NAME_RE='(ROADMAP|PRICING|WHITEPAPER|TOKENOMIC|GOVERNANCE_DAO|GOVERNANCE_VISION|COMPETITIVE_ANALYSIS|DATA-MARKETPLACE|BENCHMARK_PLAN|FUNDRAISING|INVESTOR|BUSINESS_PLAN|PLAN[_A-Z]*\.md$)'
# el patron de nombre solo aplica a zonas publicables (docs/, public/, .github/), no a codigo
NAME_ALLOW='^(docs/templates/plan\.md|docs/site/GITHUB_PAGES_PLAN\.md|docs/adr/TEMPLATE-ADR\.md|scripts/check-strategy-leak\.sh)'
hits_name=$(printf '%s\n' "$files" | grep -E '^(docs/|public/|\.github/)' | grep -inE "$NAME_RE" | grep -vE "$NAME_ALLOW" || true)

# ── 2. Rutas privadas y PII ─────────────────────────────────────────────────
PATH_RE='(/home/belal|C:\\\\Users\\\\belal|E:\\\\proyectosSWAL|/mnt/ssd-2tb|beri22@gmail\.com|belal@swal\.dev)'
hits_path=$(printf '%s\n' "$files" | grep -aInE "$PATH_RE" || true)

# ── 3. Contenido económico/estratégico en docs ─────────────────────────────
CONTENT_RE='(\$SWAL\b|\$XAV\b|\$KXAV|bonding curve|BondingCurve|tokenomics|revenue share|presale|pre-sale|\bTGE\b|Starter \$499|Pro \$|\bAPY\b|mint[[:space:]]+[0-9,]+[[:space:]]+\$)'
hits_content=$(printf '%s\n' "$files" | grep -aIvE '\.(png|jpg|jpeg|gif|webp|woff2?|ico|lock|svg)$' \
  | xargs -r grep -aiInE "$CONTENT_RE" 2>/dev/null \
  | grep -avE 'TEMPLATE-ADR|check-strategy-leak|LICENSE|XAVIER_TOKEN|XAVIER_VERSION' || true)

# ── 4. Secretos (chequeo barato; gitleaks corre aparte si está instalado) ───
SECRET_RE='(sk-or-v1-|sk-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|sbp_[A-Za-z0-9]{16,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)'
# allowlist: patrones de test, docs de referencia y las propias herramientas de escaneo
SECRET_ALLOW='^(docs/reference/CONFIG_REFERENCE\.md|docs/archive/SECURITY_LICENSE_SCAN\.md|docs/archive/HORMER_IMPL_PLAN\.md|scripts/check-secrets\.sh|scripts/check-strategy-leak\.sh|src/security/|tests/|src/nodes/audit\.rs)'
hits_secret=$(printf '%s\n' "$files" | grep -avE "$SECRET_ALLOW" | grep -aIvE '\.(png|jpg|woff2?|lock|svg)$' \
  | xargs -r grep -aInE "$SECRET_RE" 2>/dev/null || true)

report() {
  echo "❌ check-strategy-leak: $1"
  printf '%s\n' "$2" | head -25
  echo
  fail=1
}

[[ -n "$hits_name"    ]] && report "nombre de archivo estratégico (mover a docs privados)" "$hits_name"
[[ -n "$hits_path"    ]] && report "ruta personal o email en archivo trackeado"            "$hits_path"
[[ -n "$hits_content" ]] && report "contenido económico/estratégico en repo público"        "$hits_content"
[[ -n "$hits_secret"  ]] && report "posible secreto versionado"                             "$hits_secret"

if [[ $fail -eq 0 ]]; then
  echo "✅ check-strategy-leak: sin fugas de estrategia/PII detectadas ($(printf '%s\n' "$files" | wc -l) archivos)"
  exit 0
fi
cat <<'EOF'
Regla (auditoría 2026-09-10): los documentos estratégicos viven en
~/proyectosSWAL/docs/SWAL/ o ~/proyectosSWAL/docs/private/xavier-strategy/ (monorepo privado),
NUNCA en el repo público. Si el hallazgo es un falso positivo, exime la ruta en CONTENT_RE.
EOF
exit 1
