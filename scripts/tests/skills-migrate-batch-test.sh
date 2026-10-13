#!/bin/bash
set -euo pipefail

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
export PATH="$TEMP_DIR:$PATH"

export MOCK_XAVIER="$TEMP_DIR/xavier"
export XAVIER_LOG="$TEMP_DIR/xavier_log"

export MOCK_MANIFEST="$TEMP_DIR/manifest.toml"
touch "$MOCK_MANIFEST"

cat << 'MOCK_EOF' > "$MOCK_XAVIER"
#!/bin/bash
echo "xavier $@" >> "$XAVIER_LOG"

if [[ "$*" == *"skills plan"* ]]; then
  # Output fake plan.json
  local_root="${MOCK_PLAN_ROOT:-/tmp/tools/skills}"
  local_name="${MOCK_PLAN_NAME:-demo}"
  cat << JSON
{
  "manifest_version": 1,
  "manifest_hash": "abc",
  "entries": [
    {
      "name": "$local_name",
      "root": "$local_root",
      "mode": "symlink"
    }
  ],
  "removals": [],
  "foreign": []
}
JSON
  exit 0
elif [[ "$*" == *"skills apply"* ]]; then
  if [[ "$MOCK_APPLY_FAIL" == "1" ]]; then
    exit 1
  fi
  # Output fake apply.json and create projected path to satisfy verification
  cat << 'JSON'
{
  "tx_id": "fake-tx-123",
  "dry_run": false,
  "applied": [],
  "removed": []
}
JSON
  if [[ "$MOCK_APPLY_MISSING_DEST" != "1" ]]; then
    local_root="${MOCK_PLAN_ROOT:-/tmp/tools/skills}"
    local_name="${MOCK_PLAN_NAME:-demo}"
    mkdir -p "$local_root"
    touch "$local_root/$local_name"
  fi
  exit 0
elif [[ "$*" == *"skills rollback"* ]]; then
  if [[ "$MOCK_ROLLBACK_FAIL" == "1" ]]; then
    echo "conflict"
    exit 1
  fi
  cat << 'JSON'
{
  "tx_id": "fake-tx-123",
  "dry_run": false,
  "applied": [],
  "removed": []
}
JSON
  exit 0
else
  exit 0
fi
MOCK_EOF
chmod +x "$MOCK_XAVIER"
sed -i 's/return 0 2>\/dev\/null || true/exit 0/g' "$MOCK_XAVIER"
sed -i 's/return 1 2>\/dev\/null || false/exit 1/g' "$MOCK_XAVIER"

export XAVIER_BIN="$MOCK_XAVIER"
SCRIPT_TO_TEST="$(pwd)/scripts/skills-migrate-batch.sh"

run_case() {
  local name="$1"
  echo "Running $name"
  rm -f "$XAVIER_LOG"
  touch "$XAVIER_LOG"
  export MOCK_APPLY_FAIL=0
  export MOCK_APPLY_MISSING_DEST=0
  export MOCK_ROLLBACK_FAIL=0
  export MOCK_PLAN_ROOT="/tmp/tools/skills"
  export MOCK_PLAN_NAME="demo"
  rm -rf /tmp/tools/skills/demo 2>/dev/null || true
}

test_dry_run() {
  run_case "test_dry_run"
  local STATE_DIR="$TEMP_DIR/state_dry_run"
  mkdir -p "$STATE_DIR"

  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 1 --state "$STATE_DIR"

  if grep -q "skills apply" "$XAVIER_LOG"; then
    echo "test_dry_run failed: apply was called"
    exit 1
  fi

  if [[ -f "$STATE_DIR/batch-1/done" ]]; then
    echo "test_dry_run failed: done file created"
    exit 1
  fi
  echo "test_dry_run passed"
}

test_execute_success() {
  run_case "test_execute_success"
  local STATE_DIR="$TEMP_DIR/state_success"
  mkdir -p "$STATE_DIR"

  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 2 --state "$STATE_DIR" --execute

  if ! grep -q "skills apply" "$XAVIER_LOG"; then
    echo "test_execute_success failed: apply was not called"
    exit 1
  fi

  if [[ ! -f "$STATE_DIR/batch-2/done" ]]; then
    echo "test_execute_success failed: done file not created"
    exit 1
  fi

  if [[ ! -f "$STATE_DIR/batch-2/backup.tar" ]]; then
    echo "test_execute_success failed: backup.tar not created"
    exit 1
  fi
  echo "test_execute_success passed"
}

test_idempotent() {
  run_case "test_idempotent"
  local STATE_DIR="$TEMP_DIR/state_idempotent"
  mkdir -p "$STATE_DIR/batch-3"
  touch "$STATE_DIR/batch-3/done"

  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 3 --state "$STATE_DIR" --execute

  if grep -q "skills plan" "$XAVIER_LOG"; then
    echo "test_idempotent failed: plan was called on already applied batch"
    exit 1
  fi
  echo "test_idempotent passed"
}

test_failure_triggers_rollback() {
  run_case "test_failure_triggers_rollback"
  local STATE_DIR="$TEMP_DIR/state_fail"
  mkdir -p "$STATE_DIR"

  export MOCK_APPLY_FAIL=1

  set +e
  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 4 --state "$STATE_DIR" --execute
  local STATUS=$?
  set -e

  if [[ $STATUS -eq 0 ]]; then
    echo "test_failure_triggers_rollback failed: script exited with 0"
    exit 1
  fi

  if ! grep -q "skills rollback" "$XAVIER_LOG"; then
    echo "test_failure_triggers_rollback failed: rollback was not called"
    exit 1
  fi

  if [[ -f "$STATE_DIR/batch-4/done" ]]; then
    echo "test_failure_triggers_rollback failed: done file created on failure"
    exit 1
  fi
  echo "test_failure_triggers_rollback passed"
}

test_verification_failure() {
  run_case "test_verification_failure"
  local STATE_DIR="$TEMP_DIR/state_verify_fail"
  mkdir -p "$STATE_DIR"

  export MOCK_APPLY_MISSING_DEST=1

  set +e
  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 5 --state "$STATE_DIR" --execute
  local STATUS=$?
  set -e

  if [[ $STATUS -eq 0 ]]; then
    echo "test_verification_failure failed: script exited with 0"
    exit 1
  fi

  if ! grep -q "skills rollback" "$XAVIER_LOG"; then
    echo "test_verification_failure failed: rollback was not called"
    exit 1
  fi

  echo "test_verification_failure passed"
}

test_backup_symlink_does_not_dereference() {
  run_case "test_backup_symlink_does_not_dereference"
  local STATE_DIR="$TEMP_DIR/state_symlink"
  mkdir -p "$STATE_DIR"
  mkdir -p /tmp/tools/skills

  local SENTINEL="SENTINEL_SECRET_DATA_12345"
  local REAL_FILE="$TEMP_DIR/real_sentinel.txt"
  echo "$SENTINEL" > "$REAL_FILE"

  # Destination is /tmp/tools/skills/demo; make it a symlink to REAL_FILE
  rm -rf /tmp/tools/skills/demo
  ln -s "$REAL_FILE" /tmp/tools/skills/demo

  TAR_OPTIONS="-h" bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 6 --state "$STATE_DIR" --execute

  local BACKUP_TAR="$STATE_DIR/batch-6/backup.tar"
  if [[ ! -f "$BACKUP_TAR" ]]; then
    echo "test_backup_symlink_does_not_dereference failed: backup.tar missing"
    exit 1
  fi

  # Check that tar listing starts with l for the symlink entry
  local TAR_LISTING
  TAR_LISTING=$(tar -tvf "$BACKUP_TAR")
  echo "Backup tar listing:"
  echo "$TAR_LISTING"

  if ! echo "$TAR_LISTING" | grep -q '^l'; then
    echo "test_backup_symlink_does_not_dereference failed: entry is not a symlink (does not start with l)"
    exit 1
  fi

  if grep -q "$SENTINEL" "$BACKUP_TAR"; then
    echo "test_backup_symlink_does_not_dereference failed: backup.tar contains sentinel string"
    exit 1
  fi

  echo "test_backup_symlink_does_not_dereference passed"
}

test_destination_with_spaces_and_glob() {
  run_case "test_destination_with_spaces_and_glob"
  local STATE_DIR="$TEMP_DIR/state_space_glob"
  mkdir -p "$STATE_DIR"

  local SPECIAL_ROOT="$TEMP_DIR/dest space *"
  local SPECIAL_NAME="demo"
  export MOCK_PLAN_ROOT="$SPECIAL_ROOT"
  export MOCK_PLAN_NAME="$SPECIAL_NAME"

  mkdir -p "$SPECIAL_ROOT"
  local INITIAL_FILE="$SPECIAL_ROOT/$SPECIAL_NAME"
  echo "pre-existing" > "$INITIAL_FILE"

  bash "$SCRIPT_TO_TEST" --manifest "$MOCK_MANIFEST" --batch 7 --state "$STATE_DIR" --execute

  local BACKUP_TAR="$STATE_DIR/batch-7/backup.tar"
  if [[ ! -f "$BACKUP_TAR" ]]; then
    echo "test_destination_with_spaces_and_glob failed: backup.tar missing"
    exit 1
  fi

  local TAR_LISTING
  TAR_LISTING=$(tar -tf "$BACKUP_TAR")
  echo "Backup tar listing for space/glob test:"
  echo "$TAR_LISTING"

  local EXPECTED_SUBPATH="${SPECIAL_ROOT#/}/$SPECIAL_NAME"
  if ! echo "$TAR_LISTING" | grep -Fq "$EXPECTED_SUBPATH"; then
    echo "test_destination_with_spaces_and_glob failed: backup.tar does not contain $EXPECTED_SUBPATH"
    exit 1
  fi

  local ENTRY_COUNT
  ENTRY_COUNT=$(echo "$TAR_LISTING" | grep -v '^$' | wc -l)
  if [[ "$ENTRY_COUNT" -ne 1 ]]; then
    echo "test_destination_with_spaces_and_glob failed: backup.tar contains $ENTRY_COUNT entries instead of 1: $TAR_LISTING"
    exit 1
  fi

  if [[ ! -f "$STATE_DIR/batch-7/done" ]]; then
    echo "test_destination_with_spaces_and_glob failed: done file not created"
    exit 1
  fi

  echo "test_destination_with_spaces_and_glob passed"
}

test_dry_run
test_execute_success
test_idempotent
test_failure_triggers_rollback
test_verification_failure
test_backup_symlink_does_not_dereference
test_destination_with_spaces_and_glob

echo "All tests passed."

