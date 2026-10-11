#!/usr/bin/env bash
# Hermetic test for scripts/guardian/local-watchdog.sh (G9/R07).
# Fake systemctl/curl and a fake HOME live in a tempdir: no real service is
# touched, no real ~/.local/bin is read or written, no runtime data path opens.
# The suite counts FAILs and never aborts on the first one, so a mutated script
# reports every guarantee it breaks: one attempt per incident, probe-outage and
# deploy-warmup handling, restart-failure escalation, dry-run purity, the
# realpath data guard, the deploy lock, the checksum gate and the accept gate.
set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WATCHDOG="$DIR/../guardian/local-watchdog.sh"
REPO="$(cd "$DIR/../.." && pwd)"
[ -f "$WATCHDOG" ] || { printf 'FATAL: watchdog script missing: %s\n' "$WATCHDOG" >&2; exit 1; }

T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/home"
# shellcheck disable=SC2016  # the fake binaries are written literally, not expanded here
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> "${FAKE_SYSTEMCTL_LOG:?}"\nexit "${FAKE_SYSTEMCTL_RC:-0}"\n' > "$T/bin/systemctl"
# shellcheck disable=SC2016
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> "${FAKE_HTTP_LOG:?}"\nprintf "%%s" "${FAKE_HTTP_CODE-200}"\nexit "${FAKE_HTTP_RC:-0}"\n' > "$T/bin/curl"
chmod +x "$T/bin/systemctl" "$T/bin/curl"

export HOME="$T/home" PATH="$T/bin:$PATH" FAKE_SYSTEMCTL_LOG="$T/systemctl.log" FAKE_HTTP_LOG="$T/http.log"
BIN="$HOME/.local/bin/xavier"; mkdir -p "$(dirname "$BIN")"
printf '#!/usr/bin/env bash\necho xavier-good-1.0\n' > "$T/good"
printf '#!/usr/bin/env bash\necho xavier-bad-2.0\n' > "$T/bad"
printf '#!/usr/bin/env bash\nexit 1\n' > "$T/smoke-fail"
chmod +x "$T/good" "$T/bad" "$T/smoke-fail"; cp "$T/good" "$BIN"
GOOD_SHA="$(sha256sum "$BIN" | awk '{print $1}')"
UD="$HOME/.config/systemd/user"; ST="$T/state"; NOW="2026-10-07T00:00:00Z"
FLAGS=(--state-dir "$ST" --bin "$BIN" --unit xavier.service)
run() { bash "$WATCHDOG" "${FLAGS[@]}" "$@"; }
sha_of() { sha256sum "$1" | awk '{print $1}'; }
# scenario <name> [binary]: fresh state dir, and that binary installed
scenario() { ST="$T/$1"; FLAGS=(--state-dir "$ST" --bin "$BIN" --unit xavier.service); shift; [ $# -eq 0 ] || cp "$1" "$BIN"; }
retain_good() { scenario "$1" "$T/good"; run retain --now "$NOW" --apply >/dev/null 2>&1; }
deploy() { cp "$1" "$BIN"; touch -d '2000-01-01 00:00:00' "$BIN"; }  # deployed long before the window
probe() { # <code> <rc>: how the fake curl answers
    if [ -n "${1:-}" ]; then export FAKE_HTTP_CODE="$1"; else unset FAKE_HTTP_CODE; fi
    export FAKE_HTTP_RC="${2:-0}"
}
ctl_rc() { export FAKE_SYSTEMCTL_RC="$1"; }
window_since() { printf '%s\n' "$(( $(date -u +%s) - $1 ))" > "$ST/unhealthy_since"; }
restarts() { local n=0; [ -f "$FAKE_SYSTEMCTL_LOG" ] && n="$(grep -c 'restart xavier.service' "$FAKE_SYSTEMCTL_LOG" || true)"; printf '%s\n' "${n:-0}"; }
count_lines() { [ -f "$1" ] && grep -c "$2" "$1" 2>/dev/null || echo 0; }
count_all() { if [ -f "$1" ]; then wc -l < "$1" | tr -d ' '; else echo 0; fi; }

FAILS=0; PASSED=0
fail() { printf 'FAIL %s\n' "$1" >&2; FAILS=$((FAILS + 1)); }
ok() { PASSED=$((PASSED + 1)); }
assert() { if [ "$1" = "$2" ]; then ok; else fail "$3 (expected $2, got $1)"; fi; }
assert_file() { if [ -f "$2" ]; then ok; else fail "$1: missing $2"; fi; }
assert_absent() { if [ ! -e "$2" ]; then ok; else fail "$1: $2 exists"; fi; }
assert_grep() { if grep -q -- "$2" "$3" 2>/dev/null; then ok; else fail "$1: '$2' not in $3"; fi; }
assert_ngrep() { if grep -q -- "$2" "$3" 2>/dev/null; then fail "$1: '$2' present in $3"; else ok; fi; }
assert_fails() { if "$@" >/dev/null 2>&1; then fail "$1: expected non-zero exit"; else ok; fi; }
run_ok() { local name="$1"; shift; if run "$@" >"$T/$name.out" 2>&1; then ok; else fail "$name: expected zero exit"; fi; }
run_fails() { local name="$1"; shift; if run "$@" >"$T/$name.out" 2>&1; then fail "$name: expected non-zero exit"; else ok; fi; }

printf '== 1. retain records the checksum-verified previous binary\n'
retain_good retain
assert_file retain-digest "$ST/retained/xavier.sha256"
assert "$(sha_of "$ST/retained/xavier")" "$GOOD_SHA" retain-copy-matches
assert_grep retain-digest-value "$GOOD_SHA" "$ST/retained/xavier.sha256"

printf '== 2. retain is idempotent (second run is a no-op)\n'
BEFORE="$(cat "$ST/retained/xavier.sha256")"; EV_BEFORE="$(count_all "$ST/evidence.jsonl")"
run retain --now "$NOW" --apply > "$T/o2" 2>&1
assert "$(cat "$ST/retained/xavier.sha256")" "$BEFORE" retain-digest-unchanged
assert "$(count_all "$ST/evidence.jsonl")" "$EV_BEFORE" retain-no-new-evidence
assert_grep retain-noop-message 'no-op' "$T/o2"

printf '== 3. verify passes for an intact retained binary\n'
run verify --now "$NOW" > "$T/o3" 2>&1; assert_grep verify-digest "$GOOD_SHA" "$T/o3"

printf '== 4. drill restores the retained binary and records post-swap smoke\n'
retain_good drill; deploy "$T/bad"; : > "$FAKE_SYSTEMCTL_LOG"
run drill --now "$NOW" --apply > "$T/o4" 2>&1
assert "$(sha_of "$BIN")" "$GOOD_SHA" drill-restored-binary
assert "$(restarts)" 1 drill-restart
assert_grep drill-outcome '"outcome":"restored"' "$ST/evidence.jsonl"
assert_grep drill-smoke '"smoke":"ok"' "$ST/evidence.jsonl"
assert_grep drill-health '"health_code":"200"' "$ST/evidence.jsonl"

printf '== 5. a drill on the retained binary is a no-op, never a re-restart\n'
retain_good noop; deploy "$T/bad"; run drill --now "$NOW" --apply > "$T/o5" 2>&1
: > "$FAKE_SYSTEMCTL_LOG"; run drill --now "$NOW" --apply > "$T/o5b" 2>&1
assert "$(restarts)" 0 noop-no-restart
tail -n 1 "$ST/evidence.jsonl" > "$T/e1"; run drill --now "$NOW" --apply > "$T/o5c" 2>&1
tail -n 1 "$ST/evidence.jsonl" > "$T/e2"
if cmp -s "$T/e1" "$T/e2"; then ok; else fail 'drill-evidence-not-deterministic'; fi
assert_grep noop-swapped-false '"swapped":false' "$ST/evidence.jsonl"

printf '== 6. failed smoke stops retries and escalates without touching data\n'
retain_good smoke; deploy "$T/bad"
A_BEFORE="$(count_lines "$ST/evidence.jsonl" restore_attempt)"; E_BEFORE="$(count_all "$ST/escalations.jsonl")"
run_fails drill-failed-smoke drill --smoke-cmd "$T/smoke-fail" --now "$NOW" --apply
assert "$(count_lines "$ST/evidence.jsonl" restore_attempt)" "$((A_BEFORE + 1))" drill-single-attempt
assert "$(count_all "$ST/escalations.jsonl")" "$((E_BEFORE + 1))" escalation-recorded
assert_grep escalation-reason '"reason":"smoke_failed"' "$ST/escalations.jsonl"
assert_absent data-untouched "$HOME/data"

printf '== 7. checksum refusal, and check reports the same reason as drill\n'
scenario chk "$T/bad"; SHA_BEFORE="$(sha_of "$BIN")"
run_fails check-no-retained check --now "$NOW" --apply
assert "$(sha_of "$BIN")" "$SHA_BEFORE" no-retained-untouched
assert_grep check-no-retained-reason '"reason":"no_retained_binary"' "$ST/escalations.jsonl"
retain_good chk2; printf 'tampered\n' >> "$ST/retained/xavier"; SHA_BEFORE="$(sha_of "$BIN")"
run_fails drill-checksum drill --now "$NOW" --apply
assert "$(sha_of "$BIN")" "$SHA_BEFORE" tampered-binary-untouched
assert_grep drill-checksum-reason '"reason":"retained_checksum_mismatch"' "$ST/escalations.jsonl"
run_fails check-checksum check --now "$NOW" --apply
assert_grep check-checksum-reason '"reason":"retained_checksum_mismatch"' "$ST/escalations.jsonl"

printf '== 8. install stays disabled until accepted and renders the resolved repo path\n'
run_ok install-dry-run install --unit-dir "$UD"
assert_absent install-dry-run-files "$UD/xavier-guardian.timer"
assert_grep install-dry-run-plan 'plan: ' "$T/install-dry-run.out"
assert_fails run install --unit-dir "$UD" --apply
assert_absent install-unaccepted "$UD/xavier-guardian.service"
XAVIER_GUARDIAN_ACCEPTED=1 run install --unit-dir "$UD" --apply > "$T/o8" 2>&1
assert_file install-timer "$UD/xavier-guardian.timer"
assert_file install-service "$UD/xavier-guardian.service"
assert_grep install-enable 'enable --now xavier-guardian.timer' "$FAKE_SYSTEMCTL_LOG"
assert_grep install-repo-path "$REPO/scripts/guardian/local-watchdog.sh" "$UD/xavier-guardian.service"
assert_ngrep install-no-hardcoded-prefix 'proyectosSWAL' "$UD/xavier-guardian.service"

printf '== 9. runtime data paths are refused, even through a symlink\n'
ln -s "$REPO/data" "$T/repo-data-link"
run_fails data-symlink verify --state-dir "$T/repo-data-link"
assert_grep data-symlink-reason 'refusing runtime data path' "$T/data-symlink.out"
run_fails data-home-xavier drill --state-dir "$HOME/.xavier" --now "$NOW" --apply
assert_grep data-home-xavier-reason 'refusing runtime data path' "$T/data-home-xavier.out"
run_fails data-xdg-xavier drill --state-dir "$HOME/.local/share/xavier" --now "$NOW" --apply
assert_grep data-xdg-xavier-reason 'refusing runtime data path' "$T/data-xdg-xavier.out"
run_fails data-sqlite-bin drill --bin "$HOME/.local/share/xavier/memory-store.sqlite3" --now "$NOW" --apply
assert_grep data-sqlite-bin-reason 'refusing runtime data path' "$T/data-sqlite-bin.out"
assert_absent data-guard-no-home-xavier "$HOME/.xavier"
assert_absent data-guard-no-xdg-xavier "$HOME/.local/share/xavier"

printf '== 10. a healthy probe is not a rollback\n'
retain_good healthy; probe 200 0; window_since 5000; R0="$(restarts)"
run_ok check-healthy check --now "$NOW" --apply
assert "$(restarts)" "$R0" healthy-no-restart
assert_grep healthy-log 'healthy (200)' "$T/check-healthy.out"
assert_absent healthy-window-cleared "$ST/unhealthy_since"

printf '== 11. an unavailable probe is unknown, not a bad deploy\n'
retain_good outage; deploy "$T/bad"; window_since 5000; probe 000 7; R0="$(restarts)"
run_ok check-outage check --now "$NOW" --min-unhealthy-secs 60 --apply
assert "$(restarts)" "$R0" outage-no-restart
assert_grep outage-unknown 'code=000' "$T/check-outage.out"
assert_absent outage-no-evidence "$ST/evidence.jsonl"
assert_absent outage-no-escalation "$ST/escalations.jsonl"
assert_absent outage-window-cleared "$ST/unhealthy_since"
probe '' 6; run_ok check-outage-no-body check --now "$NOW" --min-unhealthy-secs 60 --apply
assert "$(restarts)" "$R0" outage-no-body-no-restart

printf '== 12. one attempt per incident: a sustained bad deploy restores once\n'
retain_good incident; deploy "$T/bad"; window_since 5000; probe 500 0; : > "$FAKE_SYSTEMCTL_LOG"
run_fails check-restores check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert "$(restarts)" 1 check-one-restart
assert "$(count_all "$ST/escalations.jsonl")" 1 check-one-escalation
deploy "$T/bad"
run_ok check-incident-blocked check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert "$(restarts)" 1 check-no-second-restart
assert "$(count_all "$ST/escalations.jsonl")" 1 check-no-second-escalation
assert_grep check-incident-blocked-log 'incident already handled' "$T/check-incident-blocked.out"

printf '== 13. deploy warmup and the sustained window are honoured\n'
retain_good warmup; window_since 5000; probe 500 0; : > "$FAKE_SYSTEMCTL_LOG"
run_ok check-warmup check --now "$NOW" --min-unhealthy-secs 0 --apply
assert "$(restarts)" 0 warmup-no-restart
assert_grep warmup-log 'warmup' "$T/check-warmup.out"
assert_absent warmup-no-incident "$ST/incident.json"
retain_good window; deploy "$T/bad"; window_since 60
run_ok check-window check --now "$NOW" --min-unhealthy-secs 900 --apply
assert "$(restarts)" 0 window-no-restart
assert_grep window-log 'waiting for 900 s' "$T/check-window.out"

printf '== 14. a failed restart escalates instead of reporting success\n'
retain_good restart; deploy "$T/bad"; window_since 5000; probe 500 0; ctl_rc 1
run_fails check-restart-failed check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert_grep restart-failed-escalation '"reason":"restart_failed"' "$ST/escalations.jsonl"
assert_grep restart-failed-incident '"outcome":"restart_failed"' "$ST/incident.json"
assert_grep restart-failed-evidence '"reason":"restart_failed"' "$ST/evidence.jsonl"
assert "$(sha_of "$BIN")" "$GOOD_SHA" restart-failed-swapped-back
ctl_rc 0

printf '== 15. dry-run writes nothing, escalations included\n'
scenario dry "$T/good"
run_ok dry-install install --unit-dir "$T/dry-ud"
run_ok dry-retain retain --now "$NOW"
run_fails dry-drill drill --now "$NOW"
run_fails dry-check check --now "$NOW"
run_ok dry-reset reset --now "$NOW"
assert_absent dry-run-state-dir "$ST"
assert_absent dry-run-unit-dir "$T/dry-ud"
retain_good escpaper; printf 'tampered\n' >> "$ST/retained/xavier"
FILES="$(find "$ST" -type f | sort)"
run_fails dry-escalate drill --now "$NOW"
assert "$(find "$ST" -type f | sort)" "$FILES" dry-escalate-no-write
assert_grep dry-escalate-plan 'plan: escalate' "$T/dry-escalate.out"
retain_good dry2; FILES="$(find "$ST" -type f | sort)"
run_ok dry2-check check --now "$NOW"
run_ok dry2-drill drill --now "$NOW"
assert "$(find "$ST" -type f | sort)" "$FILES" dry2-no-write
assert_grep dry2-check-plan 'plan: probe' "$T/dry2-check.out"
assert_grep dry2-drill-plan 'plan: restore' "$T/dry2-drill.out"

printf '== 16. reset re-arms an incident that was already handled\n'
retain_good rearm; deploy "$T/bad"; window_since 5000; probe 500 0; : > "$FAKE_SYSTEMCTL_LOG"
run_fails rearm-first check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert "$(restarts)" 1 rearm-first-restart
deploy "$T/bad"
run_ok rearm-blocked check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert_grep rearm-blocked-log 'incident already handled' "$T/rearm-blocked.out"
run_ok reset-apply reset --now "$NOW" --apply
assert_absent reset-incident-cleared "$ST/incident.json"
deploy "$T/bad"
run_fails reset-rearm check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert "$(restarts)" 2 reset-rearm-restart

printf '== 17. a deploy holding the lock blocks a concurrent check\n'
retain_good lock; deploy "$T/bad"; window_since 5000; probe 500 0; mkdir -p "$ST"; R0="$(restarts)"
exec 9>"$ST/guardian.lock"
if flock -n 9; then ok; else fail 'lock-take: cannot take the deploy lock in the test'; fi
run_ok check-locked check --now "$NOW" --min-unhealthy-secs 60 --health-attempts 1 --apply
assert "$(restarts)" "$R0" lock-no-restart
assert_grep lock-busy-message 'in progress' "$T/check-locked.out"
assert_absent lock-no-state "$ST/escalations.jsonl"
exec 9>&-

printf '\n'
if [ "$FAILS" -eq 0 ]; then
    printf 'All watchdog tests passed (%s assertions).\n' "$PASSED"
    exit 0
fi
printf '%s FAIL(S), %s assertions passed\n' "$FAILS" "$PASSED" >&2
exit 1
