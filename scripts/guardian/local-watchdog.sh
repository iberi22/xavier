#!/usr/bin/env bash
# local-watchdog.sh — G9/R07 independent previous-binary watchdog for the local
# systemd install: retains a checksum-verified previous binary, restores it
# atomically after a deploy proved bad, escalates instead of retrying, and never
# rewinds runtime data or credentials. Guarantees (07-ROLLBACK-GUARDIAN.md): a
# lock shared with the deploy script, one attempt per deployed binary
# (incident.json, written before the swap), a sustained-unhealthy window that
# treats an unavailable probe as unknown and gives a fresh deploy warmup, and a
# dry-run default that writes nothing at all.
set -euo pipefail

BIN="$HOME/.local/bin/xavier"
STATE="${XAVIER_GUARDIAN_STATE:-$HOME/.local/state/xavier-guardian}"
UNIT="xavier.service"
UNIT_DIR="$HOME/.config/systemd/user"
HEALTH_URL="http://127.0.0.1:8006/health"
SMOKE_CMD=""
HEALTH_ATTEMPTS=30
HEALTH_SLEEP=2
MIN_UNHEALTHY_SECS=900
MAX_ATTEMPTS=1
EVIDENCE_MAX=200
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DRY_RUN=1
ACCEPTED="${XAVIER_GUARDIAN_ACCEPTED:-0}"
COMMAND=""
RETAINED_SHA=""
LAST_REASON=""

usage() {
    printf '%s\n' "Usage: local-watchdog.sh <command> [options]" "" "Commands:" \
        "  retain   Record the installed binary as the checksum-verified previous binary." \
        "  verify   Check the retained binary against its recorded sha256." \
        "  drill    Rehearse a restore: swap in the retained binary, restart, health+smoke." \
        "  check    Timer entry point: probe health, restore once if the deploy is bad." \
        "  reset    Clear incident.json so the next bad deploy can be handled again." \
        "  install  Install the systemd unit + timer (disabled until accepted)." "" "Options:" \
        "  --bin <path>  --state-dir <path>  --unit <name>  --unit-dir <path>  --health-url <url>" \
        "  --smoke-cmd <cmd>   Post-swap smoke command (default: <bin> --version)." \
        "  --health-attempts <n>  Health retries after the swap (default 30)." \
        "  --min-unhealthy-secs <n>  Sustained bad probes before restoring (default 900)." \
        "  --max-attempts <n>  Restore attempts before escalating (default 1)." \
        "  --now <iso8601>  Fixed timestamp, so evidence is deterministic in tests." \
        "  --apply  Write; --dry-run (default) only prints the plan."
}
log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }
plan() { printf 'plan: %s\n' "$*"; }
die() { printf 'local-watchdog: FATAL: %s\n' "$*" >&2; exit 2; }

while [ $# -gt 0 ]; do
    case "$1" in
        retain|verify|drill|check|install|reset) COMMAND="$1" ;;
        --bin) BIN="${2:?--bin needs a value}"; shift ;;
        --state-dir) STATE="${2:?--state-dir needs a value}"; shift ;;
        --unit) UNIT="${2:?--unit needs a value}"; shift ;;
        --unit-dir) UNIT_DIR="${2:?--unit-dir needs a value}"; shift ;;
        --health-url) HEALTH_URL="${2:?--health-url needs a value}"; shift ;;
        --smoke-cmd) SMOKE_CMD="${2:?--smoke-cmd needs a value}"; shift ;;
        --health-attempts) HEALTH_ATTEMPTS="${2:?--health-attempts needs a value}"; shift ;;
        --min-unhealthy-secs) MIN_UNHEALTHY_SECS="${2:?--min-unhealthy-secs needs a value}"; shift ;;
        --max-attempts) MAX_ATTEMPTS="${2:?--max-attempts needs a value}"; shift ;;
        --now) NOW="${2:?--now needs a value}"; shift ;;
        --apply) DRY_RUN=0 ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
    shift
done
[ -n "$COMMAND" ] || { usage >&2; exit 2; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

guard_paths() { # resolve symlinks, then refuse the live store and every data dir
    local data p
    STATE="$(realpath -m "$STATE")"; BIN="$(realpath -m "$BIN")"; UNIT_DIR="$(realpath -m "$UNIT_DIR")"
    data="$(realpath -m "${XDG_DATA_HOME:-$HOME/.local/share}/xavier")"
    for p in "$STATE" "$BIN" "$UNIT_DIR"; do
        case "$p" in
            ""|"/") die "refusing empty or root path: $p" ;;
            "$REPO/data"|"$REPO/data"/*|"$HOME/.xavier"|"$HOME/.xavier"/*|"$data"|"$data"/*|*.sqlite3*) die "refusing runtime data path: $p" ;;
        esac
    done
}
guard_paths
RETAINED_DIR="$STATE/retained"; RETAINED="$RETAINED_DIR/xavier"; DIGEST="$RETAINED_DIR/xavier.sha256"
FAILED_DIR="$STATE/failed"; EVIDENCE="$STATE/evidence.jsonl"; ESCALATIONS="$STATE/escalations.jsonl"
INCIDENT="$STATE/incident.json"; UNHEALTHY_SINCE="$STATE/unhealthy_since"; LOCK="$STATE/guardian.lock"

sha_of() { sha256sum "$1" | awk '{print $1}'; }
ctl() { systemctl --user "$@"; }
health_code() { # a real status, or exactly "000" when the probe could not answer
    local out rc=0
    out="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$HEALTH_URL" 2>/dev/null)" || rc=$?
    if [ "$rc" -ne 0 ]; then printf '000'; return 0; fi
    case "$out" in [0-9][0-9][0-9]) printf '%s' "$out";; *) printf '000';; esac
}
install_atomic() { # <src> <dst>: tmp file in the same directory, then rename
    local tmp="$2.watchdog-new"
    cp "$1" "$tmp" && chmod 0755 "$tmp" && mv -f "$tmp" "$2"
}
acquire_lock() { # one check/deploy at a time; busy means "no action", not a failure
    mkdir -p "$STATE"
    exec 8>"$LOCK" || die "cannot open lock file: $LOCK"
    if ! flock -n 8; then log "deploy or another check in progress — no action"; exit 0; fi
}
record() { # <json fields>: append evidence, trimmed to the newest EVIDENCE_MAX lines
    if [ "$DRY_RUN" -eq 1 ]; then plan "append evidence: $*"; return 0; fi
    mkdir -p "$STATE"
    printf '{"ts":"%s",%s}\n' "$NOW" "$*" >> "$EVIDENCE"
    tail -n "$EVIDENCE_MAX" "$EVIDENCE" > "$EVIDENCE.tmp" && mv -f "$EVIDENCE.tmp" "$EVIDENCE"
}
escalate() { # <reason>: one record, no retry, no data rewind
    if [ "$DRY_RUN" -eq 1 ]; then
        plan "escalate $UNIT reason=$1 retained_sha256=${RETAINED_SHA:-unknown} -> $ESCALATIONS"
        printf 'local-watchdog: ESCALATE (dry-run) %s — no retry, no data rewind; open the incident issue\n' "$1" >&2
        exit 3
    fi
    mkdir -p "$STATE"
    printf '{"ts":"%s","target":"local_binary","reason":"%s","unit":"%s","retained_sha256":"%s"}\n' \
        "$NOW" "$1" "$UNIT" "${RETAINED_SHA:-unknown}" >> "$ESCALATIONS"
    tail -n "$EVIDENCE_MAX" "$ESCALATIONS" > "$ESCALATIONS.tmp" && mv -f "$ESCALATIONS.tmp" "$ESCALATIONS"
    printf 'local-watchdog: ESCALATE %s — no retry, no data rewind; open the incident issue\n' "$1" >&2
    exit 3
}
incident_sha() { # the deployed binary this incident was opened for, empty when none
    if [ -f "$INCIDENT" ]; then
        sed -n 's/.*"deployed_sha":"\([^"]*\)".*/\1/p' "$INCIDENT"
    fi
}
record_incident() { # <deployed_sha> <outcome>: the decision, written before the swap
    if [ "$DRY_RUN" -eq 1 ]; then plan "record incident $1 outcome=$2 -> $INCIDENT"; return 0; fi
    mkdir -p "$STATE"
    printf '{"deployed_sha":"%s","attempted_at":"%s","outcome":"%s"}\n' "$1" "$NOW" "$2" > "$INCIDENT.tmp"
    mv -f "$INCIDENT.tmp" "$INCIDENT"
}
preflight() { # 0 ready, 1 nothing retained, 2 checksum mismatch.
    local recorded
    guard_paths
    if [ ! -f "$RETAINED" ] || [ ! -f "$DIGEST" ]; then return 1; fi
    RETAINED_SHA="$(sha_of "$RETAINED")"
    recorded="$(awk '{print $1}' "$DIGEST")"
    [ -n "$recorded" ] && [ "$recorded" = "$RETAINED_SHA" ] || return 2
}
cmd_retain() {
    local want
    guard_paths
    [ -f "$BIN" ] || die "installed binary missing: $BIN"
    want="$(sha_of "$BIN")"
    if [ -f "$RETAINED" ] && [ -f "$DIGEST" ] && [ "$(sha_of "$RETAINED")" = "$want" ] \
        && [ "$(awk '{print $1}' "$DIGEST")" = "$want" ]; then
        log "retain no-op: retained binary already sha256=$want"
        return 0
    fi
    if [ "$DRY_RUN" -eq 1 ]; then plan "retain $BIN (sha256=$want) -> $RETAINED"; return 0; fi
    acquire_lock
    mkdir -p "$RETAINED_DIR"
    cp "$BIN" "$RETAINED.watchdog-new" && chmod 0755 "$RETAINED.watchdog-new"
    mv -f "$RETAINED.watchdog-new" "$RETAINED"
    printf '%s  %s\n' "$want" "xavier" > "$DIGEST.watchdog-new"
    mv -f "$DIGEST.watchdog-new" "$DIGEST"
    log "retained $BIN (sha256=$want)"
}
cmd_verify() {
    local pre=0; preflight || pre=$?
    case "$pre" in
        1) die "no retained binary yet: run 'local-watchdog.sh retain --apply' after a healthy deploy" ;;
        2) die "retained binary does not match its recorded sha256: $DIGEST" ;;
    esac
    log "verified retained binary: $RETAINED sha256=$RETAINED_SHA"
}
smoke_ok() {
    if [ -n "$SMOKE_CMD" ]; then
        local -a parts=()
        read -r -a parts <<< "$SMOKE_CMD"
        "${parts[@]}" >/dev/null 2>&1
    else
        "$BIN" --version >/dev/null 2>&1
    fi
}
clear_window() { rm -f "$UNHEALTHY_SINCE"; }
unhealthy_window() { # <code>: 1 when the bad status persisted long enough AND past warmup
    local since now elapsed mtime
    now="$(date -u +%s)"
    since="$(cat "$UNHEALTHY_SINCE" 2>/dev/null || true)"
    if [ -z "$since" ]; then
        printf '%s\n' "$now" > "$UNHEALTHY_SINCE"
        log "check: $UNIT reports $1 — unhealthy window starts, $MIN_UNHEALTHY_SECS s required"
        return 1
    fi
    elapsed=$((now - since))
    mtime="$(stat -c %Y "$BIN")"
    if [ "$mtime" -gt "$since" ]; then
        clear_window
        log "check: deployed binary is newer than the unhealthy window — deploy warmup, no restore"
        return 1
    fi
    if [ "$elapsed" -lt "$MIN_UNHEALTHY_SECS" ]; then
        log "check: unhealthy for ${elapsed}s — waiting for $MIN_UNHEALTHY_SECS s"
        return 1
    fi
    return 0
}
# One restore attempt: 0 on success, 1 after recording the failure.
restore_once() { # <attempt>
    local attempt="$1" line before swapped code=000 smoke=fail restart_ok=true stamp i
    before="$(sha_of "$BIN")"
    stamp="$(printf '%s' "$NOW" | tr ':-' '--')"
    record_incident "$before" in_progress
    if [ "$before" = "$RETAINED_SHA" ]; then
        swapped=false
        log "installed binary is already the retained one (sha256=$before) — no restart"
    else
        mkdir -p "$FAILED_DIR"
        cp "$BIN" "$FAILED_DIR/xavier-$before-$stamp" || escalate "failed_binary_preserve_failed"
        install_atomic "$RETAINED" "$BIN" || escalate "install_retained_failed"
        swapped=true
        ctl restart "$UNIT" || restart_ok=false
    fi
    if [ "$restart_ok" = true ]; then
        i=0
        while [ "$i" -lt "$HEALTH_ATTEMPTS" ]; do
            code="$(health_code)"
            [ "$code" = "200" ] && break
            sleep "$HEALTH_SLEEP"; i=$((i + 1))
        done
        if [ "$code" = "200" ] && ctl is-active --quiet "$UNIT" && smoke_ok; then smoke=ok; fi
    fi
    if [ "$smoke" = ok ]; then LAST_REASON=""
    elif [ "$restart_ok" = false ]; then LAST_REASON=restart_failed
    elif [ "$code" != "200" ]; then LAST_REASON=health_failed
    elif ctl is-active --quiet "$UNIT"; then LAST_REASON=smoke_failed
    else LAST_REASON=unit_inactive; fi
    line="\"event\":\"restore_attempt\",\"attempt\":$attempt,\"unit\":\"$UNIT\",\"retained_sha256\":\"$RETAINED_SHA\",\"installed_before\":\"$before\",\"swapped\":$swapped,\"health_code\":\"$code\",\"smoke\":\"$smoke\","
    if [ "$smoke" = ok ]; then
        record_incident "$before" restored
        record "$line\"outcome\":\"restored\""
        log "restore attempt $attempt ok: $BIN sha256=$RETAINED_SHA health=200 smoke=ok"
        return 0
    fi
    record_incident "$before" "$LAST_REASON"
    record "$line\"outcome\":\"failed\",\"reason\":\"$LAST_REASON\""
    printf 'local-watchdog: restore attempt %s failed (health=%s smoke=%s reason=%s)\n' \
        "$attempt" "$code" "$smoke" "$LAST_REASON" >&2
    return 1
}
run_restore_ladder() {
    local attempt=1
    while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
        restore_once "$attempt" && return 0
        attempt=$((attempt + 1))
    done
    escalate "$LAST_REASON"
}
cmd_drill() {
    local pre=0; preflight || pre=$?
    case "$pre" in 1) escalate "no_retained_binary";; 2) escalate "retained_checksum_mismatch";; esac
    if [ "$DRY_RUN" -eq 1 ]; then
        plan "restore $BIN from $RETAINED (sha256=$RETAINED_SHA), restart $UNIT, probe $HEALTH_URL, smoke $BIN"
        return 0
    fi
    acquire_lock
    run_restore_ladder
}
cmd_check() {
    local pre=0 code
    preflight || pre=$?
    case "$pre" in 1) escalate "no_retained_binary";; 2) escalate "retained_checksum_mismatch";; esac
    if [ "$DRY_RUN" -eq 1 ]; then
        plan "probe $HEALTH_URL; a sustained bad result restores $RETAINED (sha256=$RETAINED_SHA) and restarts $UNIT"
        return 0
    fi
    acquire_lock
    code="$(health_code)"
    if [ "$code" = "200" ]; then
        clear_window
        log "check: $UNIT healthy (200); retained binary stays sha256=$RETAINED_SHA"
        return 0
    fi
    if [ "$code" = "000" ]; then
        clear_window
        log "check: probe unavailable (code=000) — unknown, not a bad deploy"
        return 0
    fi
    # One attempt per deployed binary: never loop between versions.
    if [ "$(incident_sha)" = "$(sha_of "$BIN")" ]; then
        log "incident already handled; run 'local-watchdog.sh reset --apply' after review"
        return 0
    fi
    if ! unhealthy_window "$code"; then return 0; fi
    log "check: $UNIT unhealthy (code=$code); restoring retained binary"
    run_restore_ladder
}
cmd_reset() {
    if [ "$DRY_RUN" -eq 1 ]; then plan "delete $INCIDENT so a new incident can be handled"; return 0; fi
    acquire_lock
    rm -f "$INCIDENT"
    log "incident cleared: $INCIDENT (a new deployed sha is a new incident)"
}
cmd_install() {
    local src unit escaped
    src="$(cd "$(dirname "${BASH_SOURCE[0]}")/../systemd" && pwd)"
    [ -f "$src/xavier-guardian.service" ] || die "unit template missing: $src/xavier-guardian.service"
    if [ "$DRY_RUN" -eq 1 ]; then
        plan "render $src/xavier-guardian.{service,timer} with REPO=$REPO -> $UNIT_DIR, daemon-reload, enable the timer"
        return 0
    fi
    # Installation stays disabled until the owner accepts the watchdog.
    [ "$ACCEPTED" = "1" ] || die "installation disabled until accepted (set XAVIER_GUARDIAN_ACCEPTED=1 after review)"
    acquire_lock
    escaped="$(printf '%s' "$REPO" | sed -e 's/[&|\\]/\\&/g')"
    mkdir -p "$UNIT_DIR"
    for unit in xavier-guardian.service xavier-guardian.timer; do
        sed -e "s|@REPO@|$escaped|g" "$src/$unit" > "$UNIT_DIR/$unit.watchdog-new"
        chmod 0644 "$UNIT_DIR/$unit.watchdog-new"
        mv -f "$UNIT_DIR/$unit.watchdog-new" "$UNIT_DIR/$unit"
    done
    ctl daemon-reload || die "systemctl --user daemon-reload failed"
    ctl enable --now xavier-guardian.timer || die "systemctl --user enable --now xavier-guardian.timer failed"
    log "installed xavier-guardian.{service,timer} in $UNIT_DIR"
}
case "$COMMAND" in
    retain) cmd_retain ;;
    verify) cmd_verify ;;
    drill) cmd_drill ;;
    check) cmd_check ;;
    reset) cmd_reset ;;
    install) cmd_install ;;
esac
