#!/usr/bin/env bash
# check-strategy-leak.sh — impide publicar estrategia, economía, precios o PII del ecosistema SWAL.
#
# Origen: auditoría 2026-09-10 — el repo público iberi22/xavier versionaba el whitepaper tokenómico,
# PRICING ($499/$1,999), gobernanza interna (70/20/5/5), el modelo del marketplace ($SWAL 90/10),
# roadmaps internos, precios de cliente (MantenApp) y PII (emails + rutas /home/belal).
# Este gate caza ESE tipo de documento, no cualquier mención técnica de la palabra "tokenomics".
#
# Precisión > exhaustividad: un gate que siempre falla se bypassea y no protege nada.
#
#   bash scripts/check-strategy-leak.sh            # tracked + untracked no ignorado
#   bash scripts/check-strategy-leak.sh --staged   # solo lo que entra al commit (hook)
#
# Falso positivo: añade el path a la allowlist correspondiente. NO relajes los patrones sin ADR.
set -uo pipefail

MODE="${1:-all}"
fail=0
is_bin='\.(png|jpg|jpeg|gif|webp|ico|svg|woff2?|pdf|zip|lock|map|min\.js)$'

if [[ "$MODE" == "--staged" ]]; then
  files=$(git diff --cached --name-only --diff-filter=ACMR)
else
  files=$( { git ls-files; git ls-files --others --exclude-standard; } | sort -u )
fi
[[ -z "$files" ]] && { echo "✅ check-strategy-leak: nada que revisar"; exit 0; }

# Zonas publicables (el código no se escanea por palabras: se escanea por nombre y por secretos).
DOC_SCOPE='^(docs/|public/|\.github/|README\.md|AGENTS\.md|LICENSE)'

# 1. Nombres de documento que NUNCA van al repo público.
NAME_RE='(ROADMAP|PRICING|WHITEPAPER|TOKENOMIC|GOVERNANCE_DAO|GOVERNANCE_VISION|COMPETITIVE_ANALYSIS|DATA-MARKETPLACE|BENCHMARK_PLAN|FUNDRAISING|INVESTOR|BUSINESS_PLAN|COMMERCIAL_STRATEGY|PLAN[_A-Z0-9]*\.md$|BENCHMARK\.md$)'
NAME_ALLOW='^(docs/templates/plan\.md|docs/site/GITHUB_PAGES_PLAN\.md|docs/adr/TEMPLATE-ADR\.md|scripts/check-strategy-leak\.sh|docs/SRS/INDEX\.md)$'
hits_name=$(printf '%s\n' "$files" | grep -E "$DOC_SCOPE" | grep -iE "$NAME_RE" | grep -vE "$NAME_ALLOW" || true)

# 2. PII: rutas de máquina y emails personales (en repo público esto es fuga siempre).
PII_RE='(/home/belal|C:\\\\Users\\\\belal|E:\\\\proyectosSWAL|/mnt/ssd-2tb|beri22@gmail\.com|belal@swal\.dev|@gmail\.com)'
PII_ALLOW='^(\.gitcore/MANIFEST\.json|benches/results/|docs/archive/)'
hits_pii=$(printf '%s\n' "$files" | grep -E "$DOC_SCOPE" | grep -vE "$PII_ALLOW" \
  | grep -avE "$is_bin" | xargs -r grep -aInE "$PII_RE" 2>/dev/null || true)

# 3. Marcadores económicos INEQUÍVOCOS (no términos técnicos como "tokenomics" que nombran módulos).
ECON_RE='(\$[0-9][0-9,]*(\.[0-9]+)?[[:space:]]*(/yr|/year|/mo|/month|/mes|per year)[[:space:]]|bonding[ _]curve|revenue[ _]share|[[:space:]]pre-?sale[[:space:]]|\bTGE\b|\bAPY\b|burn[ _]rate|mint(ed)?[[:space:]]+[0-9,]+[[:space:]]*\$|\$XAV[[:space:]]*(token|wallet)|token[[:space:]]+(sale|allocation))'
ECON_ALLOW='^(docs/SRS/|docs/auto-docs/|docs/api/|docs/reference/|docs/licenses/|docs/site/src/content/docs/(modules|manual)/|ACCOUNT|scripts/check-strategy-leak\.sh)'
hits_econ=$(printf '%s\n' "$files" | grep -E "$DOC_SCOPE" | grep -vE "$ECON_ALLOW" \
  | grep -avE "$is_bin" | xargs -r grep -aInE "$ECON_RE" 2>/dev/null || true)

# 4. Secretos (chequeo barato; scripts/check-secrets.sh + gitleaks cubren el resto).
SECRET_RE='(sk-or-v1-[A-Za-z0-9]{20,}|sk-[A-Za-z0-9]{32,}|ghp_[A-Za-z0-9]{20,}|sbp_[A-Za-z0-9]{16,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)'
SECRET_ALLOW='^(docs/reference/|docs/archive/|scripts/|src/security/|tests/|src/nodes/audit\.rs)'
hits_secret=$(printf '%s\n' "$files" | grep -vE "$SECRET_ALLOW" | grep -avE "$is_bin" \
  | xargs -r grep -aInE "$SECRET_RE" 2>/dev/null || true)

report() { echo "❌ check-strategy-leak: $1"; printf '%s\n' "$2" | head -20; echo; fail=1; }

[[ -n "$hits_name"   ]] && report "documento estratégico en zona publicable (mover a docs privados)" "$hits_name"
[[ -n "$hits_pii"    ]] && report "PII (ruta de máquina o email personal) en zona publicable"        "$hits_pii"
[[ -n "$hits_econ"   ]] && report "precio/tokenomics en zona publicable (mover o redactar)"          "$hits_econ"
[[ -n "$hits_secret" ]] && report "posible secreto versionado"                                       "$hits_secret"

if [[ $fail -eq 0 ]]; then
  echo "✅ check-strategy-leak: sin fugas detectadas ($(printf '%s\n' "$files" | wc -l) archivos revisados)"
  exit 0
fi
cat <<'EOF'
Regla (auditoría 2026-09-10): la estrategia vive en ~/proyectosSWAL/docs/SWAL/ o
~/proyectosSWAL/docs/private/xavier-strategy/ (monorepo privado), NUNCA en el repo público.
EOF
exit 1
