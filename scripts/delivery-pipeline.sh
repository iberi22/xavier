#!/usr/bin/env bash
# 8-stage delivery driver: runs stage commands for one immutable SHA, writes a
# sealed JSON evidence bundle, then calls done-check. Never edits the repo or pushes.
#
#   delivery-pipeline.sh run --sha SHA --task ID --out BUNDLE [--base REF]
#   delivery-pipeline.sh verify BUNDLE
#
# Env: PIPELINE_STAGE_<1..8>_CMD (required, fail closed), PIPELINE_REPO,
#      PIPELINE_ARTIFACT_DIR, PIPELINE_EXECUTOR, PIPELINE_REVIEWER,
#      PIPELINE_DECLARED_FILES, DONE_CHECK, DONE_REVIEW_CMD.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DONE_CHECK="${DONE_CHECK:-$SCRIPT_DIR/done-check.sh}"
STAGE_NAMES=(intake research design task-dag implementation verification integration release)

die() { echo "delivery-pipeline: $*" >&2; exit 2; }
sha256() { sha256sum "$1" | cut -d' ' -f1; }
seal_of() { jq -S -c 'del(.seal)' "$1" | sha256sum | cut -d' ' -f1; }

# Ship/show/ask from the diff, 01-DESIGN: unknown or mixed paths are ask.
classify() { # classify <files-file> <lines>
    local files="$1" lines="$2" n f
    n=$(grep -c . "$files" || true)
    if [ "$n" -eq 0 ]; then echo ask; return; fi
    local all_ship=true all_show=true
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        case "$f" in
            docs/*|.gitcore/*|.github/*|.husky/*|Cargo.toml|Cargo.lock|.env.example|scripts/check-secrets.sh|scripts/verify-pipeline.sh|panel-ui/*|migrations/*|*/migrations/*|*.sql)
                echo ask; return ;;
        esac
        case "$f" in *.md) ;; *) all_ship=false ;; esac
        case "$f" in src/*|crates/*|code-graph/*|tests/*) ;; *) all_show=false ;; esac
    done < "$files"
    if [ "$n" -gt 4 ] || [ "$lines" -gt 400 ]; then echo ask; return; fi
    if $all_ship; then echo ship; elif $all_show; then echo show; else echo ask; fi
}

verify_bundle() {
    local b="$1"
    [ -f "$b" ] || die "bundle not found: $b"
    jq -e 'type == "object" and has("seal") and (.stages | type == "array")' "$b" >/dev/null 2>&1 \
        || { echo "FAIL: malformed bundle" >&2; return 1; }
    [ "$(jq -r .seal "$b")" = "$(seal_of "$b")" ] \
        || { echo "FAIL: seal mismatch (bundle tampered)" >&2; return 1; }
    [ "$(jq '.stages | length' "$b")" -eq 8 ] \
        && [ "$(jq '[.stages[].exit] | all(. == 0)' "$b")" = true ] \
        || { echo "FAIL: not all 8 stages passed" >&2; return 1; }
    local dir path want
    dir=$(jq -r .artifact_dir "$b")
    while IFS=$'\t' read -r path want; do
        [ -f "$dir/$path" ] && [ "$(sha256 "$dir/$path")" = "$want" ] \
            || { echo "FAIL: artifact $path missing or altered" >&2; return 1; }
    done < <(jq -r '.stages[].artifacts[] | [.path, .sha256] | @tsv' "$b")
    local repo
    repo=$(jq -r .worktree "$b")
    (cd "$repo" && BASE_SHA="$(jq -r .base "$b")" "$DONE_CHECK" "$b" --json)
}

cmd_run() {
    local sha="" task="" out="" base="origin/main"
    while [ $# -gt 0 ]; do
        case "$1" in
            --sha) sha="$2"; shift 2 ;;
            --task) task="$2"; shift 2 ;;
            --out) out="$2"; shift 2 ;;
            --base) base="$2"; shift 2 ;;
            *) die "unknown argument: $1" ;;
        esac
    done
    [ -n "$sha" ] && [ -n "$task" ] && [ -n "$out" ] || die "usage: run --sha SHA --task ID --out BUNDLE [--base REF]"

    local repo art
    repo="$(cd "${PIPELINE_REPO:-.}" && pwd)"
    [ "$(git -C "$repo" rev-parse --verify -q "$sha^{commit}" || true)" = "$sha" ] || die "sha is not a full commit id: $sha"
    [ "$(git -C "$repo" rev-parse HEAD)" = "$sha" ] || die "HEAD is not $sha (candidate must be checked out)"
    mkdir -p "${PIPELINE_ARTIFACT_DIR:-$(dirname "$out")/artifacts}" "$(dirname "$out")"
    art="$(cd "${PIPELINE_ARTIFACT_DIR:-$(dirname "$out")/artifacts}" && pwd)"
    rm -f "$art"/stage-*.log "$art/claim.txt"

    local tmp; tmp=$(mktemp -d); trap 'rm -rf "$tmp"' RETURN
    local stages="[]" commands="[]" failed=0 i var c rc log
    for i in 1 2 3 4 5 6 7 8; do
        var="PIPELINE_STAGE_${i}_CMD"
        c="${!var:-}"
        log="$art/stage-$i.log"
        if [ -z "$c" ]; then
            echo "stage $i: no command configured" > "$log"; rc=127
        else
            set +e
            (cd "$repo" && PIPELINE_SHA="$sha" bash -c "$c" </dev/null >"$log" 2>&1)
            rc=$?
            set -e
        fi
        stages=$(jq -c --argjson s "$stages" --argjson i "$i" --arg name "${STAGE_NAMES[$((i-1))]}" --arg sha "$sha" \
            --arg c "$c" --argjson rc "$rc" --arg h "$(sha256 "$log")" \
            -n '$s + [{stage:$i, name:$name, sha:$sha, command:$c, exit:$rc, artifacts:[{path:("stage-"+($i|tostring)+".log"), sha256:$h}]}]')
        commands=$(jq -c -n --argjson a "$commands" --arg c "$c" '$a + [$c]')
        if [ "$rc" -ne 0 ]; then failed=1; echo "stage $i (${STAGE_NAMES[$((i-1))]}) failed: exit $rc" >&2; break; fi
    done

    git -C "$repo" diff --name-only "$base...$sha" | LC_ALL=C sort > "$tmp/files"
    local lines
    lines=$(git -C "$repo" diff --numstat "$base...$sha" | awk '{ if ($1 ~ /^[0-9]+$/) s += $1; if ($2 ~ /^[0-9]+$/) s += $2 } END { print s + 0 }')
    local class; class=$(classify "$tmp/files" "$lines")
    local changed declared
    changed=$(jq -R -c -s 'split("\n") | map(select(length > 0))' "$tmp/files")
    if [ -n "${PIPELINE_DECLARED_FILES:-}" ]; then
        declared=$(printf '%s\n' $PIPELINE_DECLARED_FILES | LC_ALL=C sort | jq -R -c -s 'split("\n") | map(select(length > 0))')
    else
        declared="$changed"
    fi
    printf 'task=%s\nsha=%s\n' "$task" "$sha" > "$art/claim.txt"

    local passed=true secret=pass
    [ "$failed" -eq 0 ] || { passed=false; secret=fail; }
    jq -S -c -n --arg task "$task" --arg sha "$sha" --arg base "$base" --arg class "$class" \
        --arg repo "$repo" --arg art "$art" --arg claim "$art/claim.txt" \
        --arg ex "${PIPELINE_EXECUTOR:-executor}" --arg rv "${PIPELINE_REVIEWER:-reviewer}" \
        --argjson stages "$stages" --argjson commands "$commands" --argjson changed "$changed" \
        --argjson declared "$declared" --argjson lines "$lines" --argjson passed "$passed" --arg secret "$secret" \
        '{task_id:$task, sha:$sha, base:$base, class:$class, artifact_dir:$art, worktree:$repo, claim_file:$claim,
          declared_files:$declared, changed_files:$changed, changed_lines:$lines,
          scans:{secret_scan:$secret}, commands:$commands, stages:$stages,
          atlas:{passed:$passed, dod_complete:$passed, evidence_count:($stages|map(select(.exit==0))|length)},
          review:{verdict:"pending", subject_sha:$sha, backend:$rv, executor:$ex}}' > "$tmp/bundle.json"
    jq -S -c --arg seal "$(seal_of "$tmp/bundle.json")" '. + {seal:$seal}' "$tmp/bundle.json" > "$tmp/sealed.json"
    mv "$tmp/sealed.json" "$out"

    [ "$failed" -eq 0 ] || exit 1
    verify_bundle "$out"
}

case "${1:-}" in
    run) shift; cmd_run "$@" ;;
    verify) [ $# -eq 2 ] || die "usage: verify BUNDLE"; verify_bundle "$2" ;;
    *) die "usage: $0 run|verify ..." ;;
esac
