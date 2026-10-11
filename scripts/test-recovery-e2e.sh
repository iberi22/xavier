#!/usr/bin/env bash
# test-recovery-e2e.sh — prueba E2E NO interactiva de `xavier recovery`.
#
# Recorre, con un binario real y en directorios temporales desechables:
#   seal -> perder record.key -> unseal -> restore -> la clave vuelve idéntica
# por la ruta de las 24 palabras y por la de la passphrase, más los casos
# negativos (palabras/frase incorrectas, sello manipulado, KCV ajeno, --out
# sobre archivo o symlink, sellar dos veces, sellar sin record.key, frase corta)
# y un escaneo de fugas (la clave nunca aparece en stdout/stderr).
#
# NUNCA toca ~/.xavier, el keyring del sistema ni datos reales:
#   - cada ejecución corre con `env -i` y HOME, XAVIER_DATA_DIR,
#     XAVIER_RECOVERY_DIR, XDG_* dentro del sandbox;
#   - DBUS_SESSION_BUS_ADDRESS apunta a un socket inexistente, así el keyring
#     (secret-service) es inalcanzable y nada se escribe en él;
#   - al final verifica que ~/.xavier real no cambió (solo metadatos, no lee
#     ningún secreto).
#
# Uso:
#   scripts/test-recovery-e2e.sh                     # usa target/debug/xavier
#   scripts/test-recovery-e2e.sh --bin ~/.local/bin/xavier   # binario instalado
#   scripts/test-recovery-e2e.sh --keep              # conserva el sandbox
# Con un binario anterior a `seal --passphrase-file/--words-out` corre solo la
# ruta de palabras (las parsea de stdout) y marca el resto como SKIP.
# Sale con 0 solo si no hay ningún FAIL.
set -uo pipefail

usage() { sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; }

BIN="${XAVIER_BIN:-}"
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "argumento desconocido: $1" >&2; usage >&2; exit 2 ;;
  esac
done
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "$BIN" ]; then
  BIN="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/debug/xavier"
fi
BIN="$(readlink -f "$BIN" 2>/dev/null || echo "$BIN")"
if [ ! -x "$BIN" ]; then
  echo "no encuentro un binario ejecutable de xavier en $BIN (compila con: cargo build --bin xavier)" >&2
  exit 2
fi

REAL_HOME="${HOME}"
SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/xavier-test-recovery.XXXXXX")"
chmod 700 "$SANDBOX"
cleanup() {
  if [ "$KEEP" = 1 ]; then
    echo "sandbox conservado en $SANDBOX"
  else
    chmod -R u+rwx "$SANDBOX" 2>/dev/null
    rm -rf "$SANDBOX"
  fi
}
trap cleanup EXIT

H="$SANDBOX/home"; D="$SANDBOX/data"; R="$SANDBOX/recovery"; W="$SANDBOX/work"; LOG="$SANDBOX/logs"
mkdir -p "$H" "$D/node" "$W" "$LOG" "$SANDBOX/run"
chmod 700 "$H" "$D" "$W" "$LOG" "$SANDBOX/run"
umask 077

PASS=0; FAIL=0; SKIP=0; RC=0
ok()   { PASS=$((PASS+1)); printf '  \033[32mOK\033[0m   %s\n' "$1"; }
ko()   { FAIL=$((FAIL+1)); printf '  \033[31mFAIL\033[0m %s\n' "$1"; [ -n "${2:-}" ] && [ -f "$2" ] && sed 's/^/         | /' "$2" | head -n 12; }
skip() { SKIP=$((SKIP+1)); printf '  \033[33mSKIP\033[0m %s\n' "$1"; }
step() { printf '\n== %s\n' "$1"; }
check() { if eval "$2"; then ok "$1"; else ko "$1" "${3:-}"; fi; }

# Ejecuta xavier herméticamente. Uso: run <nombre-log> [args...]
# Variables opcionales: DDIR (data dir) y RDIR (recovery dir).
run() {
  local name="$1"; shift
  ( cd "$SANDBOX" && env -i \
      PATH="/usr/bin:/bin:/run/current-system/sw/bin" \
      HOME="$H" \
      XAVIER_DATA_DIR="${DDIR:-$D}" \
      XAVIER_RECOVERY_DIR="${RDIR:-$R}" \
      XDG_RUNTIME_DIR="$SANDBOX/run" \
      XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" \
      XDG_CACHE_HOME="$H/.cache" XDG_STATE_HOME="$H/.local/state" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=$SANDBOX/no-such-bus" \
      RUST_LOG=warn LANG=C.UTF-8 \
      "$BIN" "$@" </dev/null >"$LOG/$name.log" 2>&1 )
  RC=$?
  return $RC
}

rand_hex() { head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
sha() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
mode() { stat -c '%a' "$1" 2>/dev/null; }
seed_key() { mkdir -p "$1/node"; printf '%s' "$2" > "$1/node/record.key"; chmod 600 "$1/node/record.key"; }
no_candidates() { ! find "$@" -name '*.candidate*' 2>/dev/null | grep -q .; }
# Invierte el primer nibble del ciphertext de un sello (sigue siendo hex válido).
tamper_ct() {
  local f="$1" ct first flipped
  ct="$(sed -nE 's/.*"ciphertext": *"([0-9a-f]+)".*/\1/p' "$f" | head -n1)"
  [ -n "$ct" ] || return 1
  first="${ct:0:1}"
  flipped="$(printf '%s' "$first" | tr '0123456789abcdef' '123456789abcdef0')"
  sed -i "s/$ct/${flipped}${ct:1}/" "$f"
}
real_fingerprint() {  # solo metadatos; jamás lee contenido
  for p in "$REAL_HOME/.xavier" "$REAL_HOME/.xavier/recovery" "$REAL_HOME/.xavier/master.key"; do
    if [ -e "$p" ]; then stat -c '%n %i %s %Y' "$p"; else echo "$p ABSENT"; fi
  done
}

REAL_BEFORE="$(real_fingerprint)"
echo "binario : $BIN"
echo "sandbox : $SANDBOX"

step "0. capacidades del binario"
if ! run help recovery --help; then ko "xavier recovery --help" "$LOG/help.log"; exit 1; fi
run seal-help recovery seal --help
run restore-help recovery restore --help
HAS_KEY_FILE=0
grep -q -- '--key-file' "$LOG/restore-help.log" && HAS_KEY_FILE=1
HAS_WORDS_OUT=0; HAS_PASS_FILE=0
grep -q -- '--words-out' "$LOG/seal-help.log" && HAS_WORDS_OUT=1
grep -q -- '--passphrase-file' "$LOG/seal-help.log" && HAS_PASS_FILE=1
NEW=$(( HAS_WORDS_OUT && HAS_PASS_FILE ))
echo "  seal --words-out: $HAS_WORDS_OUT | seal --passphrase-file: $HAS_PASS_FILE"
[ "$NEW" = 1 ] || echo "  (binario anterior: solo ruta de palabras; status no se usa porque genera record.key)"

KEY="$(rand_hex)"
seed_key "$D" "$KEY"
KEY_SHA="$(sha "$D/node/record.key")"
KEYFILE="$D/node/record.key"
PASS_TXT='frase larga de prueba 123'
printf '%s\r\n' "$PASS_TXT" > "$W/pass-crlf.txt"
printf '%s\r'   "$PASS_TXT" > "$W/pass-cr.txt"
printf '%s\n'   "$PASS_TXT" > "$W/pass-lf.txt"
printf 'otra frase que no es 999\n' > "$W/pass-wrong.txt"
printf 'corta\n' > "$W/pass-short.txt"

step "1. seal (no interactivo)"
if [ "$HAS_WORDS_OUT" = 1 ]; then
  run seal-m recovery seal --mnemonic --words-out "$W/words.txt"
  check "seal --mnemonic --words-out sale con 0" '[ $RC -eq 0 ]' "$LOG/seal-m.log"
  check "archivo de palabras 0600" '[ "$(mode "$W/words.txt")" = 600 ]'
  check "archivo de palabras con 24 palabras" '[ "$(wc -w < "$W/words.txt")" -eq 24 ]'
  if [ "$HAS_PASS_FILE" = 1 ]; then
    run seal-p recovery seal --passphrase --passphrase-file "$W/pass-crlf.txt"
    check "seal --passphrase --passphrase-file (CRLF) sale con 0" '[ $RC -eq 0 ]' "$LOG/seal-p.log"
  else
    skip "seal --passphrase-file (el binario no lo soporta)"
  fi
  check "seal no creó master.key en HOME (no acuña ni escribe keyring)" '[ ! -e "$H/.xavier/master.key" ]'
else
  run seal-m recovery seal --mnemonic
  check "seal --mnemonic (binario anterior) sale con 0" '[ $RC -eq 0 ]' "$LOG/seal-m.log"
  sed -nE 's/^ *[0-9]{1,2}\. ([^[:space:]]+) *$/\1/p' "$LOG/seal-m.log" | tr '\n' ' ' | sed 's/ $//' > "$W/words.txt"
  check "24 palabras leídas de stdout" '[ "$(wc -w < "$W/words.txt")" -eq 24 ]'
  skip "seal --passphrase-file (el binario no lo soporta)"
fi
check "sello de palabras 0600" '[ "$(mode "$R/record.mnemonic.seal.json")" = 600 ]'
[ "$NEW" = 1 ] && check "sello de passphrase 0600" '[ "$(mode "$R/record.passphrase.seal.json")" = 600 ]'
check "seal no tocó record.key" '[ "$(sha "$KEYFILE")" = "$KEY_SHA" ]'
M_SEAL_SHA="$(sha "$R/record.mnemonic.seal.json")"

if [ "$NEW" = 1 ]; then
  step "2. status reporta sellos abribles sin exponer la clave"
  run status-1 recovery status
  check "status sale con 0" '[ $RC -eq 0 ]' "$LOG/status-1.log"
  check "mnemonic seal: valid (0600)" 'grep -q "mnemonic seal *: valid (0600)" "$LOG/status-1.log"' "$LOG/status-1.log"
  check "passphrase seal: valid (0600)" 'grep -q "passphrase seal *: valid (0600)" "$LOG/status-1.log"' "$LOG/status-1.log"
  check "key-check value: matches live key" 'grep -q "matches live key" "$LOG/status-1.log"' "$LOG/status-1.log"

  step "3. sellar dos veces no destruye el primer papel"
  run seal-twice recovery seal --mnemonic --words-out "$W/words2.txt"
  check "segundo seal --mnemonic rechazado" '[ $RC -ne 0 ]' "$LOG/seal-twice.log"
  check "primer sello intacto" '[ "$(sha "$R/record.mnemonic.seal.json")" = "$M_SEAL_SHA" ]'
  check "no se creó un segundo archivo de palabras" '[ ! -e "$W/words2.txt" ]'
else
  skip "status/doble seal (el binario anterior acuña record.key en status)"
fi

step "4. negativos con la clave presente"
awk '{for(i=2;i<=NF;i++) printf "%s ", $i; print $1}' "$W/words.txt" > "$W/words-wrong.txt"
run unseal-wrong-words recovery unseal --mnemonic --words-file "$W/words-wrong.txt" --out "$W/never1.hex"
check "palabras incorrectas -> falla" '[ $RC -ne 0 ]' "$LOG/unseal-wrong-words.log"
check "palabras incorrectas no escriben --out" '[ ! -e "$W/never1.hex" ]'
if [ "$NEW" = 1 ]; then
  run unseal-wrong-pass recovery unseal --passphrase --passphrase-file "$W/pass-wrong.txt" --out "$W/never2.hex"
  check "passphrase incorrecta -> falla" '[ $RC -ne 0 ]' "$LOG/unseal-wrong-pass.log"
  check "passphrase incorrecta no escribe --out" '[ ! -e "$W/never2.hex" ]'
fi
printf 'contenido previo' > "$W/exists.hex"; EX_SHA="$(sha "$W/exists.hex")"
run out-exists recovery unseal --mnemonic --words-file "$W/words.txt" --out "$W/exists.hex"
check "--out sobre archivo existente -> rechazado" '[ $RC -ne 0 ]' "$LOG/out-exists.log"
check "archivo existente intacto" '[ "$(sha "$W/exists.hex")" = "$EX_SHA" ]'
ln -s "$W/nowhere.hex" "$W/dangling.hex"
run out-dangling recovery unseal --mnemonic --words-file "$W/words.txt" --out "$W/dangling.hex"
check "--out sobre symlink colgante -> rechazado" '[ $RC -ne 0 ]' "$LOG/out-dangling.log"
check "el destino del symlink no se creó" '[ ! -e "$W/nowhere.hex" ]'
ln -s "$W/exists.hex" "$W/link-existing.hex"
run out-link recovery unseal --mnemonic --words-file "$W/words.txt" --out "$W/link-existing.hex"
check "--out sobre symlink a archivo existente -> rechazado" '[ $RC -ne 0 ]' "$LOG/out-link.log"
check "el archivo apuntado sigue intacto" '[ "$(sha "$W/exists.hex")" = "$EX_SHA" ]'
check "record.key intacto tras los negativos" '[ "$(sha "$KEYFILE")" = "$KEY_SHA" ]'

step "5. se pierde record.key"
rm -f "$KEYFILE"
if [ "$NEW" = 1 ]; then
  run status-lost recovery status
  check "status sin record.key sale con 0" '[ $RC -eq 0 ]' "$LOG/status-lost.log"
  check "status NO acuña un record.key nuevo" '[ ! -e "$KEYFILE" ]'
  check "status avisa MISSING / restore possible" 'grep -qE "MISSING|live key missing" "$LOG/status-lost.log"' "$LOG/status-lost.log"
fi

step "6. unseal con las 24 palabras (sin TTY)"
run unseal-m recovery unseal --mnemonic --words-file "$W/words.txt"
check "unseal --mnemonic --words-file instala la clave" '[ $RC -eq 0 ]' "$LOG/unseal-m.log"
check "clave restaurada idéntica a la original" '[ "$(sha "$KEYFILE")" = "$KEY_SHA" ]'
check "clave restaurada 0600" '[ "$(mode "$KEYFILE")" = 600 ]'
check "sin archivos .candidate sobrantes" 'no_candidates "$D" "$R"'

step "7. restore idempotente: repetir no cambia nada"
run unseal-again recovery unseal --mnemonic --words-file "$W/words.txt"
check "segundo unseal rechazado (target_exists)" '[ $RC -ne 0 ] && grep -q target_exists "$LOG/unseal-again.log"' "$LOG/unseal-again.log"
if [ "$HAS_KEY_FILE" = 1 ]; then
  printf '%s\n' "$KEY" > "$W/restore.key"
  run restore-again recovery restore --key-file "$W/restore.key"
else
  run restore-again recovery restore --key-hex "$KEY"
fi
check "restore con clave presente rechazado" '[ $RC -ne 0 ]' "$LOG/restore-again.log"
check "record.key sin cambios" '[ "$(sha "$KEYFILE")" = "$KEY_SHA" ]'
check "sin archivos .candidate sobrantes" 'no_candidates "$D" "$R"'

if [ "$NEW" = 1 ]; then
  step "8. unseal con passphrase (archivo con CRLF y con \\r suelto)"
  rm -f "$KEYFILE"
  run unseal-p recovery unseal --passphrase --passphrase-file "$W/pass-lf.txt"
  check "unseal --passphrase-file (LF) instala la clave" '[ $RC -eq 0 ]' "$LOG/unseal-p.log"
  check "clave restaurada idéntica" '[ "$(sha "$KEYFILE")" = "$KEY_SHA" ]'
  run unseal-cr recovery unseal --passphrase --passphrase-file "$W/pass-cr.txt" --out "$W/out-cr.hex"
  check "passphrase con \\r suelto abre el sello" '[ $RC -eq 0 ]' "$LOG/unseal-cr.log"
  check "--out contiene exactamente la clave" '[ "$(cat "$W/out-cr.hex")" = "$KEY" ]'
  check "--out es 0600" '[ "$(mode "$W/out-cr.hex")" = 600 ]'
  rm -f "$W/out-cr.hex"
else
  skip "unseal por passphrase (no hay sello de passphrase con este binario)"
fi

step "9. sellos manipulados y KCV ajeno"
T="$SANDBOX/tampered"; cp -a "$R" "$T"
tamper_ct "$T/record.mnemonic.seal.json"
[ "$NEW" = 1 ] && tamper_ct "$T/record.passphrase.seal.json"
RDIR="$T" run tamper-m recovery unseal --mnemonic --words-file "$W/words.txt" --out "$W/never3.hex"
check "sello de palabras manipulado -> falla" '[ $RC -ne 0 ] && [ ! -e "$W/never3.hex" ]' "$LOG/tamper-m.log"
if [ "$NEW" = 1 ]; then
  RDIR="$T" run tamper-p recovery unseal --passphrase --passphrase-file "$W/pass-lf.txt" --out "$W/never4.hex"
  check "sello de passphrase manipulado -> falla" '[ $RC -ne 0 ] && [ ! -e "$W/never4.hex" ]' "$LOG/tamper-p.log"
  TR="$SANDBOX/truncated"; cp -a "$R" "$TR"
  head -c 40 "$R/record.passphrase.seal.json" > "$TR/record.passphrase.seal.json"
  RDIR="$TR" run status-trunc recovery status
  check "status detecta sello truncado (MALFORMED)" 'grep -q "passphrase seal *: MALFORMED" "$LOG/status-trunc.log"' "$LOG/status-trunc.log"

  # KCV de OTRA clave: se sella un segundo nodo y se copia su KCV.
  D2="$SANDBOX/data2"; R2="$SANDBOX/recovery2"; seed_key "$D2" "$(rand_hex)"
  DDIR="$D2" RDIR="$R2" run seal-other recovery seal --mnemonic --words-out "$W/words-other.txt"
  M="$SANDBOX/kcv-mismatch"; cp -a "$R" "$M"; cp "$R2/record.key.kcv" "$M/record.key.kcv"
  RDIR="$M" run status-mismatch recovery status
  check "status detecta KCV de otra clave (MISMATCH)" 'grep -q "MISMATCH" "$LOG/status-mismatch.log"' "$LOG/status-mismatch.log"
  RDIR="$M" run unseal-mismatch recovery unseal --mnemonic --words-file "$W/words.txt" --out "$W/never5.hex"
  check "unseal con KCV ajeno -> falla" '[ $RC -ne 0 ] && [ ! -e "$W/never5.hex" ]' "$LOG/unseal-mismatch.log"

  step "10. seal se niega a trabajar sin record.key o con frase corta"
  D3="$SANDBOX/data3"; R3="$SANDBOX/recovery3"; mkdir -p "$D3"
  DDIR="$D3" RDIR="$R3" run seal-nokey recovery seal --mnemonic --words-out "$W/words-nokey.txt"
  check "seal sin record.key -> rechazado" '[ $RC -ne 0 ]' "$LOG/seal-nokey.log"
  check "seal sin record.key no acuña una clave" '[ ! -e "$D3/node/record.key" ]'
  check "seal sin record.key no deja palabras" '[ ! -e "$W/words-nokey.txt" ]'
  D4="$SANDBOX/data4"; R4="$SANDBOX/recovery4"; seed_key "$D4" "$(rand_hex)"
  DDIR="$D4" RDIR="$R4" run seal-short recovery seal --passphrase --passphrase-file "$W/pass-short.txt"
  check "frase de menos de 12 caracteres -> rechazado" '[ $RC -ne 0 ]' "$LOG/seal-short.log"
  check "frase corta no deja KCV, manifiesto ni sello" '[ ! -e "$R4/record.key.kcv" ] && [ ! -e "$R4/recovery-manifest.json" ] && [ ! -e "$R4/record.passphrase.seal.json" ]'
fi

step "11. fugas: la clave nunca sale por stdout/stderr"
check "la clave en hex no aparece en ningún log" '! grep -rqiF "$KEY" "$LOG"'
if [ "$NEW" = 1 ]; then
  WORDS_LINE="$(tr -s ' \n' ' ' < "$W/words.txt" | sed 's/ $//')"
  check "las 24 palabras no aparecen en ningún log (--words-out)" '! grep -rqF "$WORDS_LINE" "$LOG"'
  check "la passphrase no aparece en ningún log" '! grep -rqF "$PASS_TXT" "$LOG"'
fi

step "12. datos reales intactos"
REAL_AFTER="$(real_fingerprint)"
check "~/.xavier real sin cambios (metadatos)" '[ "$REAL_BEFORE" = "$REAL_AFTER" ]'

printf '\nResultado: %d OK, %d FAIL, %d SKIP  (binario: %s)\n' "$PASS" "$FAIL" "$SKIP" "$BIN"
[ "$FAIL" -eq 0 ]
