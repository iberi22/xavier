#!/usr/bin/env bash
# verify-gate.sh — suite de verificación del gate anti-fuga (assert-based, sin dependencias).
# Uso: bash /tmp/verify-gate.sh   (se ejecuta desde el repo xavier)
set -uo pipefail
cd /home/belal/proyectosSWAL/apps/xavier
GATE="bash scripts/check-strategy-leak.sh"
pass=0; fail=0
check() { # check <desc> <esperado> <obtenido>
  if [[ "$2" == "$3" ]]; then echo "  PASS: $1 (exit=$3)"; pass=$((pass+1));
  else echo "  FAIL: $1 (esperado exit=$2, obtuvo exit=$3)"; fail=$((fail+1)); fi
}

echo "T1 — sintaxis"
bash -n scripts/check-strategy-leak.sh; check "bash -n" 0 $?

echo "T2 — HEAD saneado debe PASAR"
$GATE >/tmp/v_all.txt 2>&1; check "modo all sobre HEAD saneado" 0 $?

echo "T3 — --staged con árbol limpio debe PASAR"
$GATE --staged >/dev/null 2>&1; check "--staged limpio" 0 $?

echo "T4 — debe DETECTAR nombre estratégico (staged)"
printf '# PRICING\nStarter $499/yr\n' > docs/archive/PRICING.md
git add -f docs/archive/PRICING.md >/dev/null 2>&1
$GATE --staged >/tmp/v_name.txt 2>&1; check "detecta PRICING.md" 1 $?
grep -q "PRICING.md" /tmp/v_name.txt && echo "  PASS: cita el archivo" || { echo "  FAIL: no cita el archivo"; fail=$((fail+1)); }
git rm -q --cached docs/archive/PRICING.md >/dev/null 2>&1; rm -f docs/archive/PRICING.md

echo "T5 — debe DETECTAR precio en doc publicable (staged)"
printf '# notas\nEl plan Pro cuesta $1,999/yr para clientes.\n' > docs/_t5.md
git add -f docs/_t5.md >/dev/null 2>&1
$GATE --staged >/tmp/v_econ.txt 2>&1; check "detecta precio" 1 $?
git rm -q --cached docs/_t5.md >/dev/null 2>&1; rm -f docs/_t5.md

echo "T6 — debe DETECTAR PII (staged)"
printf 'path: /home/belal/proyectosSWAL\nowner: iberi22@gmail.com\n' > docs/_t6.md
git add -f docs/_t6.md >/dev/null 2>&1
$GATE --staged >/tmp/v_pii.txt 2>&1; check "detecta PII" 1 $?
git rm -q --cached docs/_t6.md >/dev/null 2>&1; rm -f docs/_t6.md

echo "T7 — NO debe dar falso positivo con términos técnicos legítimos (staged)"
printf '# Mesh\nModulos: `src/mesh/tokenomics/economy.rs` y el economic core $SWAL en L0-L1.\n' > docs/_t7.md
git add -f docs/_t7.md >/dev/null 2>&1
$GATE --staged >/tmp/v_fp.txt 2>&1; check "sin falso positivo tecnico" 0 $?
[[ -s /tmp/v_fp.txt ]] && grep -q "fugas" /tmp/v_fp.txt && echo "  PASS: reportó limpio" || echo "  (revisar /tmp/v_fp.txt)"
git rm -q --cached docs/_t7.md >/dev/null 2>&1; rm -f docs/_t7.md

echo
echo "RESULTADO: $pass PASS / $fail FAIL"
git status --short | head -5
[[ $fail -eq 0 ]] && exit 0 || exit 1
