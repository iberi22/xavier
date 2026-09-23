#!/usr/bin/env bash
# [S1.09] Stability watch script: one-command KPI corroboration (T1/T2)
#
# Read-only stability watcher for Xavier environment.
# Runs 7 verification checks, outputs machine-greppable key=value per line,
# and outputs final OVERALL=PASS|ALERT line.
#
# Defaults (overridable via environment):
#   OLLAMA_UNIT : default 'ollama'
#   XAVIER_SERVICE : default 'xavier'
#   XAVIER_URL  : default 'http://127.0.0.1:8006'
#   VEC_DB      : default 'data/vec-store.sqlite3'
#
# Normative Thresholds:
#   1. emb_per_min : <= 120 (SANO < 60, ALERT > 120)
#   2. hit_rate    : >= 0.90
#   3. n_restarts  : == 0
#   4. gpu_jct_c   : < 90 °C
#   5. quick_check : ok
#   6. vec_f32     : == 0
#   7. malformed   : == 0

set -euo pipefail

OLLAMA_UNIT="${OLLAMA_UNIT:-ollama}"
XAVIER_SERVICE="${XAVIER_SERVICE:-xavier}"
XAVIER_URL="${XAVIER_URL:-http://127.0.0.1:8006}"
VEC_DB="${VEC_DB:-data/vec-store.sqlite3}"

overall_alert=0

# Helper to print KPI result and update overall_alert
record_kpi() {
  local name="$1"
  local val="$2"
  local threshold="$3"
  local status="$4"

  echo "KPI ${name}=${val} threshold=${threshold} status=${status}"
  if [ "$status" = "ALERT" ]; then
    overall_alert=1
  fi
}

# --- Check 1: GIN rate (emb_per_min) ---
check_gin_rate() {
  local count=""
  if command -v journalctl >/dev/null 2>&1; then
    count="$(journalctl -u "$OLLAMA_UNIT" --since '60 seconds ago' 2>/dev/null | grep -c "GIN" || true)"
    if [ -z "$count" ] || ! [[ "$count" =~ ^[0-9]+$ ]]; then
      count="$(journalctl --user -u "$OLLAMA_UNIT" --since '60 seconds ago' 2>/dev/null | grep -c "GIN" || true)"
    fi
  fi

  if [ -n "$count" ] && [[ "$count" =~ ^[0-9]+$ ]]; then
    if [ "$count" -gt 120 ]; then
      record_kpi "emb_per_min" "$count" "60" "ALERT"
    else
      record_kpi "emb_per_min" "$count" "60" "PASS"
    fi
  else
    record_kpi "emb_per_min" "NA" "60" "SKIP"
  fi
}

# --- Check 2: /health cache hit rate ---
check_health_hit_rate() {
  local health_json=""
  health_json="$(curl -s --max-time 5 "$XAVIER_URL/health" 2>/dev/null || true)"

  local hit_rate="NA"
  if [ -n "$health_json" ]; then
    if command -v python3 >/dev/null 2>&1; then
      hit_rate="$(echo "$health_json" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    hr = d.get('cache_hit_rate', d.get('hit_rate', d.get('cache', {}).get('hit_rate', None)))
    if hr is not None:
        print(f'{float(hr):.2f}')
    else:
        print('NA')
except Exception:
    print('NA')
" 2>/dev/null || echo "NA")"
    else
      hit_rate="$(echo "$health_json" | grep -o '"hit_rate":\s*[0-9.]*' | grep -o '[0-9.]*' || echo "NA")"
    fi
  fi

  if [ "$hit_rate" != "NA" ]; then
    local is_pass="0"
    if command -v python3 >/dev/null 2>&1; then
      is_pass="$(python3 -c "print(1 if float('$hit_rate') >= 0.90 else 0)" 2>/dev/null || echo "0")"
    elif command -v awk >/dev/null 2>&1; then
      is_pass="$(awk -v hr="$hit_rate" 'BEGIN { print (hr >= 0.90) ? 1 : 0 }' 2>/dev/null || echo "0")"
    fi

    if [ "$is_pass" -eq 1 ]; then
      record_kpi "hit_rate" "$hit_rate" "0.90" "PASS"
    else
      record_kpi "hit_rate" "$hit_rate" "0.90" "ALERT"
    fi
  else
    # Server unreachable or hit_rate unavailable
    record_kpi "hit_rate" "NA" "0.90" "SKIP"
  fi
}

# --- Check 3: Service state (n_restarts) ---
check_service_restarts() {
  local n_restarts="NA"
  if command -v systemctl >/dev/null 2>&1; then
    local prop=""
    prop="$(systemctl --user show "$XAVIER_SERVICE" --property=NRestarts 2>/dev/null | cut -d= -f2 || true)"
    if [ -z "$prop" ]; then
      prop="$(systemctl show "$XAVIER_SERVICE" --property=NRestarts 2>/dev/null | cut -d= -f2 || true)"
    fi
    if [ -n "$prop" ] && [[ "$prop" =~ ^[0-9]+$ ]]; then
      n_restarts="$prop"
    fi
  fi

  if [ "$n_restarts" != "NA" ]; then
    if [ "$n_restarts" -eq 0 ]; then
      record_kpi "n_restarts" "$n_restarts" "0" "PASS"
    else
      record_kpi "n_restarts" "$n_restarts" "0" "ALERT"
    fi
  else
    record_kpi "n_restarts" "NA" "0" "SKIP"
  fi
}

# --- Check 4: GPU junction temp (gpu_jct_c) ---
check_gpu_temp() {
  local temp_file=""
  local temp_raw=""
  local temp_c="NA"

  # Auto-detect: card1 -> card0 fallback -> general sysfs hwmon
  for card in card1 card0; do
    for path in /sys/class/drm/"$card"/device/hwmon/hwmon*/temp2_input /sys/class/drm/"$card"/device/hwmon/hwmon*/temp1_input; do
      if [ -f "$path" ]; then
        temp_file="$path"
        break 2
      fi
    done
  done

  if [ -z "$temp_file" ]; then
    for path in /sys/class/hwmon/hwmon*/temp1_input; do
      if [ -f "$path" ]; then
        temp_file="$path"
        break
      fi
    done
  fi

  if [ -n "$temp_file" ] && [ -f "$temp_file" ]; then
    temp_raw="$(cat "$temp_file" 2>/dev/null || true)"
    if [ -n "$temp_raw" ] && [[ "$temp_raw" =~ ^[0-9]+$ ]]; then
      if [ "$temp_raw" -gt 1000 ]; then
        temp_c=$(( temp_raw / 1000 ))
      else
        temp_c="$temp_raw"
      fi
    fi
  fi

  if [ "$temp_c" != "NA" ]; then
    if [ "$temp_c" -lt 90 ]; then
      record_kpi "gpu_jct_c" "$temp_c" "90" "PASS"
    else
      record_kpi "gpu_jct_c" "$temp_c" "90" "ALERT"
    fi
  else
    record_kpi "gpu_jct_c" "NA" "90" "SKIP"
  fi
}

# --- Check 5: PRAGMA quick_check ---
check_sqlite_quick_check() {
  local res="NA"
  if [ -f "$VEC_DB" ] && command -v sqlite3 >/dev/null 2>&1; then
    res="$(sqlite3 "file:${VEC_DB}?mode=ro" "PRAGMA quick_check;" 2>/dev/null || echo "error")"
  elif [ -f "$VEC_DB" ] && command -v python3 >/dev/null 2>&1; then
    res="$(python3 -c "
import sqlite3
try:
    con = sqlite3.connect('file:${VEC_DB}?mode=ro', uri=True)
    cur = con.cursor()
    cur.execute('PRAGMA quick_check;')
    row = cur.fetchone()
    print(row[0] if row else 'error')
except Exception:
    print('error')
" 2>/dev/null || echo "error")"
  fi

  if [ "$res" = "ok" ]; then
    record_kpi "quick_check" "$res" "ok" "PASS"
  elif [ "$res" = "NA" ]; then
    record_kpi "quick_check" "NA" "ok" "SKIP"
  else
    record_kpi "quick_check" "$res" "ok" "ALERT"
  fi
}

# --- Check 6: vec_f32 new/unprocessed check ---
check_vec_f32() {
  local vec_f32="NA"
  if [ -f "$VEC_DB" ] && command -v sqlite3 >/dev/null 2>&1; then
    vec_f32="$(sqlite3 "file:${VEC_DB}?mode=ro" "SELECT COUNT(*) FROM memory_records WHERE length(embedding) = 0 OR embedding IS NULL;" 2>/dev/null || echo "NA")"
  elif [ -f "$VEC_DB" ] && command -v python3 >/dev/null 2>&1; then
    vec_f32="$(python3 -c "
import sqlite3
try:
    con = sqlite3.connect('file:${VEC_DB}?mode=ro', uri=True)
    cur = con.cursor()
    cur.execute('SELECT COUNT(*) FROM memory_records WHERE length(embedding) = 0 OR embedding IS NULL')
    print(cur.fetchone()[0])
except Exception:
    print('NA')
" 2>/dev/null || echo "NA")"
  fi

  if [ "$vec_f32" != "NA" ] && [[ "$vec_f32" =~ ^[0-9]+$ ]]; then
    if [ "$vec_f32" -eq 0 ]; then
      record_kpi "vec_f32" "$vec_f32" "0" "PASS"
    else
      record_kpi "vec_f32" "$vec_f32" "0" "ALERT"
    fi
  else
    record_kpi "vec_f32" "NA" "0" "SKIP"
  fi
}

# --- Check 7: Malformed error counts since boot ---
check_malformed_errors() {
  local malformed="0"
  if command -v journalctl >/dev/null 2>&1; then
    local count
    count="$(journalctl -u "$XAVIER_SERVICE" --since 'boot' 2>/dev/null | grep -ciE "malformed|corrupt|panic" || true)"
    if [ -n "$count" ] && [[ "$count" =~ ^[0-9]+$ ]]; then
      malformed="$count"
    fi
  fi

  if [ "$malformed" -eq 0 ]; then
    record_kpi "malformed" "$malformed" "0" "PASS"
  else
    record_kpi "malformed" "$malformed" "0" "ALERT"
  fi
}

main() {
  check_gin_rate
  check_health_hit_rate
  check_service_restarts
  check_gpu_temp
  check_sqlite_quick_check
  check_vec_f32
  check_malformed_errors

  if [ "$overall_alert" -eq 0 ]; then
    echo "OVERALL=PASS"
    exit 0
  else
    echo "OVERALL=ALERT"
    exit 1
  fi
}

main
