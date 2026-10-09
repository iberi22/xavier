#!/bin/bash

BACKEND_A=""
BACKEND_B=""
WALL=""
QUIET=""
OUT=""
ARGV=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --backend-a) BACKEND_A="$2"; shift 2 ;;
    --backend-b) BACKEND_B="$2"; shift 2 ;;
    --wall) WALL="$2"; shift 2 ;;
    --quiet) QUIET="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --) shift; ARGV=("$@"); break ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$BACKEND_A" || -z "$BACKEND_B" || -z "$WALL" || -z "$QUIET" || -z "$OUT" || ${#ARGV[@]} -eq 0 ]]; then
  echo "Usage: $0 --backend-a <cmd> --backend-b <cmd> --wall <seconds> --quiet <seconds> --out <dir> -- <argv...>" >&2
  exit 1
fi

mkdir -p "$OUT"
ATTEMPTS_FILE="$OUT/attempts.jsonl"
HEAD_SHA=$(git rev-parse HEAD 2>/dev/null || echo "unknown")

run_attempt() {
  local backend_name="$1"
  local backend_cmd="$2"

  local stdout_file="$OUT/${backend_name}_stdout.log"
  local stderr_file="$OUT/${backend_name}_stderr.log"
  local exit_code_file="$OUT/${backend_name}_exit"
  local pid_file="$OUT/${backend_name}_pid"

  rm -f "$exit_code_file" "$pid_file" "$stdout_file" "$stderr_file"
  touch "$stdout_file" "$stderr_file"

  local start_ts=$(date +%s)

  # Launch the backend in its own process group
  (
    set -m
    ( "$backend_cmd" "${ARGV[@]}" ) </dev/null >"$stdout_file" 2>"$stderr_file" &
    child_pid=$!
    echo $child_pid > "$pid_file"
    wait $child_pid
    echo $? > "$exit_code_file"
  ) &
  local runner_pid=$!

  local reason=""
  local running=true
  local wall_elapsed=0
  local quiet_elapsed=0
  local last_stdout_size=0
  local last_stderr_size=0

  while [[ $wall_elapsed -lt $WALL ]]; do
    if ! kill -0 $runner_pid 2>/dev/null; then
      running=false
      break
    fi

    local current_stdout_size=$(wc -c < "$stdout_file" 2>/dev/null | awk '{print $1}')
    local current_stderr_size=$(wc -c < "$stderr_file" 2>/dev/null | awk '{print $1}')
    [[ -z "$current_stdout_size" ]] && current_stdout_size=0
    [[ -z "$current_stderr_size" ]] && current_stderr_size=0

    if [[ $current_stdout_size -eq $last_stdout_size && $current_stderr_size -eq $last_stderr_size ]]; then
      quiet_elapsed=$((quiet_elapsed + 1))
    else
      quiet_elapsed=0
      last_stdout_size=$current_stdout_size
      last_stderr_size=$current_stderr_size
    fi

    if [[ $quiet_elapsed -ge $QUIET ]]; then
      reason="quiet_limit"
      break
    fi

    sleep 1
    wall_elapsed=$((wall_elapsed + 1))
  done

  if [[ "$running" == true && -z "$reason" ]]; then
    reason="wall_limit"
  fi

  local backend_pid=""
  if [[ -f "$pid_file" ]]; then
    backend_pid=$(cat "$pid_file")
  fi

  if [[ -n "$reason" ]]; then
    if [[ -n "$backend_pid" ]]; then
      kill -- -"$backend_pid" 2>/dev/null || true
      sleep 1
      kill -KILL -- -"$backend_pid" 2>/dev/null || true
    fi
    kill -KILL "$runner_pid" 2>/dev/null || true
    wait "$runner_pid" 2>/dev/null || true
  else
    wait "$runner_pid" 2>/dev/null || true
  fi

  local end_ts=$(date +%s)

  local exit_code=1
  if [[ -f "$exit_code_file" ]]; then
    exit_code=$(cat "$exit_code_file")
  fi

  local final_stdout_size=$(wc -c < "$stdout_file" 2>/dev/null | awk '{print $1}')
  local final_stderr_size=$(wc -c < "$stderr_file" 2>/dev/null | awk '{print $1}')
  [[ -z "$final_stdout_size" ]] && final_stdout_size=0
  [[ -z "$final_stderr_size" ]] && final_stderr_size=0

  if [[ -z "$reason" && $exit_code -ne 0 ]]; then
    reason="nonzero_exit"
  elif [[ -z "$reason" && $exit_code -eq 0 && $final_stdout_size -eq 0 && $final_stderr_size -eq 0 ]]; then
    reason="zero_bytes"
  fi

  local reason_json="null"
  if [[ -n "$reason" ]]; then
    reason_json="\"$reason\""
  fi

  echo "{\"backend\":\"$backend_name\",\"start_ts\":$start_ts,\"end_ts\":$end_ts,\"exit_code\":$exit_code,\"stdout_bytes\":$final_stdout_size,\"stderr_bytes\":$final_stderr_size,\"reason\":$reason_json,\"head_sha\":\"$HEAD_SHA\"}" >> "$ATTEMPTS_FILE"

  if [[ -z "$reason" ]]; then
    return 0
  else
    echo "$reason" > "$OUT/${backend_name}_fail_reason"
    return 1
  fi
}

run_attempt "backend-a" "$BACKEND_A"
if [[ $? -eq 0 ]]; then
  exit 0
fi

FAIL_REASON_A=$(cat "$OUT/backend-a_fail_reason" 2>/dev/null)

run_attempt "backend-b" "$BACKEND_B"
if [[ $? -eq 0 ]]; then
  exit 0
fi

FAIL_REASON_B=$(cat "$OUT/backend-b_fail_reason" 2>/dev/null)

echo "Attempt A failed with $FAIL_REASON_A, Attempt B failed with $FAIL_REASON_B" >&2
exit 1
