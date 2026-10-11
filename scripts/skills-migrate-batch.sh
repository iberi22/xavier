#!/bin/bash
set -euo pipefail

XAVIER_BIN="${XAVIER_BIN:-xavier}"

MANIFEST=""
BATCH=""
STATE_DIR=""
EXECUTE=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --manifest)
      MANIFEST="$2"
      shift 2
      ;;
    --batch)
      BATCH="$2"
      shift 2
      ;;
    --state)
      STATE_DIR="$2"
      shift 2
      ;;
    --execute)
      EXECUTE=1
      shift 1
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

if [[ -z "$MANIFEST" || -z "$BATCH" || -z "$STATE_DIR" ]]; then
  echo "Usage: $0 --manifest M --batch N --state DIR [--execute]"
  exit 1
fi

BATCH_DIR="$STATE_DIR/batch-$BATCH"
DONE_FILE="$BATCH_DIR/done"
PLAN_FILE="$BATCH_DIR/plan.json"
BACKUP_TAR="$BATCH_DIR/backup.tar"
APPLY_FILE="$BATCH_DIR/apply.json"

if [[ -f "$DONE_FILE" ]]; then
  echo "Batch $BATCH already applied (state file $DONE_FILE exists). Skipping."
  exit 0
fi

mkdir -p "$BATCH_DIR"

"$XAVIER_BIN" skills plan --manifest "$MANIFEST" > "$PLAN_FILE"

if [[ $EXECUTE -eq 0 ]]; then
  echo "Dry-run plan summary:"
  jq '.entries | length' "$PLAN_FILE" | sed 's/^/Entries: /'
  exit 0
fi

DEST_PATHS=$(jq -r '.entries[] | "\(.root)/\(.name)"' "$PLAN_FILE")

# Backup existing paths safely
BACKUP_TARGETS=()
for path in $DEST_PATHS; do
  # Check if it exists or is a broken symlink
  if [[ -e "$path" || -h "$path" ]]; then
    BACKUP_TARGETS+=("$path")
  fi
done

if [[ ${#BACKUP_TARGETS[@]} -gt 0 ]]; then
  tar -cf "$BACKUP_TAR" "${BACKUP_TARGETS[@]}"
  sha256sum "$BACKUP_TAR" > "$BACKUP_TAR.sha256"
else
  # create empty tar to avoid errors and represent empty backup
  tar -cf "$BACKUP_TAR" -T /dev/null
  sha256sum "$BACKUP_TAR" > "$BACKUP_TAR.sha256"
fi

set +e
"$XAVIER_BIN" skills apply --manifest "$MANIFEST" --plan "$PLAN_FILE" --apply > "$APPLY_FILE"
APPLY_STATUS=$?
set -e

TX_ID=$(jq -r '.tx_id // empty' "$APPLY_FILE" 2>/dev/null || echo "")

VERIFICATION_FAILED=0
if [[ $APPLY_STATUS -eq 0 ]]; then
  for path in $DEST_PATHS; do
    if [[ ! -e "$path" && ! -h "$path" ]]; then
      echo "Verification failed: $path does not exist after apply."
      VERIFICATION_FAILED=1
    fi
  done
fi

if [[ $APPLY_STATUS -ne 0 || $VERIFICATION_FAILED -ne 0 ]]; then
  echo "Failure detected (Apply Status: $APPLY_STATUS, Verification: $VERIFICATION_FAILED). Initiating rollback..."
  set +e
  "$XAVIER_BIN" skills rollback --manifest "$MANIFEST" --tx-id "${TX_ID:-unknown}" --apply > "$BATCH_DIR/rollback.json"
  ROLLBACK_STATUS=$?
  set -e

  # Simple conflict check from output, if rollback failed we restore backup
  if [[ $ROLLBACK_STATUS -ne 0 ]] || grep -iq 'conflict' "$BATCH_DIR/rollback.json"; then
    echo "Rollback failed or conflicts reported. Restoring from tar backup..."
    tar -xf "$BACKUP_TAR" -C /
  fi
  exit 1
fi

touch "$DONE_FILE"
echo "Batch $BATCH successfully applied."
exit 0
