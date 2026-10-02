#!/usr/bin/env bash
# scripts/mini-expert-train.sh - thin wrapper for the Xavier mini-expert pipeline (MX-05).
#
#   export bundle (daemon :8006) -> train_expert.py -> ollama create -> expert_eval.py
#   -> register in Xavier ONLY if the eval verdict is PROMOTE.
#
# No mock manifests, no placeholder GGUFs: any failing step aborts with non-zero exit.
# Auth token is read from $XAVIER_TOKEN and never printed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat << 'EOF2'
Usage: mini-expert-train.sh --name NAME [options]

Required:
  --name NAME              Expert name (also the Ollama model name after promotion)

Options:
  --segment SEG            Domain segment for the registry (default: general)
  --language LANG          ISO language (default: es)
  --clearance N            Clearance 0-5 for the registry (default: 1)
  --source-dataset NAME    Source dataset label (default: xavier-telemetry)
  --xavier-url URL         Daemon base URL (default: $XAVIER_URL or http://127.0.0.1:8006)
  --bundle DIR             Use an existing bundle dir instead of exporting
  --backend local|notebook Training backend (default: local). notebook only generates the
                           Colab notebook + bundle.zip and stops (human step, see output)
  --resume-gguf FILE       Continue after the notebook: use this returned model-q4_k_m.gguf
  --base-model HF_ID       HF base model (default: Qwen/Qwen2.5-0.5B-Instruct)
  --base-ollama NAME       Ollama model used as eval baseline (default: qwen2.5:0.5b)
  --margin F               Required eval score gain over base (default: 0.05)
  --output-dir DIR         Artifacts dir (default: ./build/mini-experts)
  --xavier-bin PATH        xavier CLI binary (default: xavier)
  -h, --help               Show this help

Env: XAVIER_TOKEN (daemon auth), LLAMA_CPP_DIR (llama.cpp checkout, for local training),
     OLLAMA_URL (default http://127.0.0.1:11434).
EOF2
}

NAME=""; SEGMENT="general"; LANGUAGE="es"; CLEARANCE="1"; SOURCE_DATASET="xavier-telemetry"
XAVIER_URL="${XAVIER_URL:-http://127.0.0.1:8006}"; BUNDLE=""; BACKEND="local"; RESUME_GGUF=""
BASE_MODEL="Qwen/Qwen2.5-0.5B-Instruct"; BASE_OLLAMA="qwen2.5:0.5b"; MARGIN="0.05"
OUTPUT_DIR="./build/mini-experts"; XAVIER_BIN="xavier"
OLLAMA_URL="${OLLAMA_URL:-http://127.0.0.1:11434}"

need_val() { [[ $# -ge 2 ]] || { echo "Missing value for $1" >&2; exit 1; }; }
if [[ $# -eq 0 ]]; then usage; exit 1; fi
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        --name) need_val "$@"; NAME="$2"; shift 2 ;;
        --segment) need_val "$@"; SEGMENT="$2"; shift 2 ;;
        --language) need_val "$@"; LANGUAGE="$2"; shift 2 ;;
        --clearance) need_val "$@"; CLEARANCE="$2"; shift 2 ;;
        --source-dataset) need_val "$@"; SOURCE_DATASET="$2"; shift 2 ;;
        --xavier-url) need_val "$@"; XAVIER_URL="$2"; shift 2 ;;
        --bundle) need_val "$@"; BUNDLE="$2"; shift 2 ;;
        --backend) need_val "$@"; BACKEND="$2"; shift 2 ;;
        --resume-gguf) need_val "$@"; RESUME_GGUF="$2"; shift 2 ;;
        --base-model) need_val "$@"; BASE_MODEL="$2"; shift 2 ;;
        --base-ollama) need_val "$@"; BASE_OLLAMA="$2"; shift 2 ;;
        --margin) need_val "$@"; MARGIN="$2"; shift 2 ;;
        --output-dir) need_val "$@"; OUTPUT_DIR="$2"; shift 2 ;;
        --xavier-bin) need_val "$@"; XAVIER_BIN="$2"; shift 2 ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

[[ -n "$NAME" ]] || { echo "Error: --name is required." >&2; exit 1; }
[[ "$NAME" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "Error: --name must match [A-Za-z0-9._-]+" >&2; exit 1; }
[[ "$BACKEND" == "local" || "$BACKEND" == "notebook" ]] || { echo "Error: --backend must be local|notebook" >&2; exit 1; }

WORK="$OUTPUT_DIR/$NAME"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
[[ -n "$BUNDLE" ]] || BUNDLE="$WORK/bundle"

# 1. Export bundle from the daemon (skipped with --bundle or --resume-gguf).
if [[ -z "$RESUME_GGUF" && ! -f "$BUNDLE/bundle_manifest.json" ]]; then
    echo "[1/5] Exporting bundle from $XAVIER_URL/v1/training/bundles ..."
    mkdir -p "$BUNDLE"
    # Token goes through a curl config on a pipe: never on the command line, never echoed.
    auth_cfg() { if [[ -n "${XAVIER_TOKEN:-}" ]]; then printf 'header = "X-Xavier-Token: %s"\n' "$XAVIER_TOKEN"; fi; }
    BODY="$(python3 -c 'import json,sys; print(json.dumps({"seed":42,"eval_ratio":0.1,"clearance":sys.argv[1],"language":sys.argv[2],"segment":sys.argv[3]}))' "INTERNAL" "$LANGUAGE" "$SEGMENT")"
    curl -fsS -X POST "$XAVIER_URL/v1/training/bundles" -H "Content-Type: application/json" \
        --config <(auth_cfg) -d "$BODY" -o "$WORK/export_response.json"
    DATASET_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["dataset_id"])' "$WORK/export_response.json")"
    [[ "$DATASET_ID" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "Invalid dataset_id from daemon" >&2; exit 1; }
    curl -fsS --config <(auth_cfg) "$XAVIER_URL/v1/training/datasets/$DATASET_ID" -o "$BUNDLE/bundle_manifest.json"
    curl -fsS --config <(auth_cfg) "$XAVIER_URL/v1/training/datasets/$DATASET_ID/train" -o "$BUNDLE/train.jsonl"
    curl -fsS --config <(auth_cfg) "$XAVIER_URL/v1/training/datasets/$DATASET_ID/eval" -o "$BUNDLE/eval.jsonl"
    python3 -c 'import json,sys; json.dump(json.load(open(sys.argv[1]))["audit_summary"], open(sys.argv[2],"w"), indent=2)' \
        "$WORK/export_response.json" "$BUNDLE/anonymization_audit.json"
else
    echo "[1/5] Using existing bundle: $BUNDLE"
fi

# 2. Train.
if [[ -n "$RESUME_GGUF" ]]; then
    [[ -f "$RESUME_GGUF" ]] || { echo "GGUF not found: $RESUME_GGUF" >&2; exit 1; }
    GGUF="$(cd "$(dirname "$RESUME_GGUF")" && pwd)/$(basename "$RESUME_GGUF")"
    [[ "$(head -c4 "$GGUF")" == "GGUF" ]] || { echo "Not a GGUF file (bad magic): $GGUF" >&2; exit 1; }
    echo "[2/5] Resuming with returned artifact: $GGUF"
else
    echo "[2/5] Training (backend=$BACKEND) ..."
    python3 "$HERE/training/train_expert.py" --bundle "$BUNDLE" --base-model "$BASE_MODEL" \
        --out "$WORK/train" --backend "$BACKEND"
    if [[ "$BACKEND" == "notebook" ]]; then
        echo "Notebook generated. After the human run, re-invoke with --resume-gguf <model-q4_k_m.gguf>."
        exit 0
    fi
    GGUF="$WORK/train/model-q4_k_m.gguf"
fi

# 3. Ollama model from the real GGUF.
CANDIDATE="$NAME-candidate"
MODELFILE="$WORK/Modelfile"
cat > "$MODELFILE" << EOF2
FROM $GGUF
TEMPLATE """{{ if .System }}<|im_start|>system
{{ .System }}<|im_end|>
{{ end }}<|im_start|>user
{{ .Prompt }}<|im_end|>
<|im_start|>assistant
"""
PARAMETER stop "<|im_end|>"
PARAMETER temperature 0.2
SYSTEM You are a personal mini-expert specialized in $SEGMENT ($LANGUAGE).
EOF2
echo "[3/5] ollama create $CANDIDATE ..."
ollama create "$CANDIDATE" -f "$MODELFILE"

# 4. Eval base vs candidate on the fixed eval split.
echo "[4/5] Evaluating $BASE_OLLAMA vs $CANDIDATE ..."
set +e
python3 "$HERE/eval/expert_eval.py" --eval "$BUNDLE/eval.jsonl" --base "$BASE_OLLAMA" \
    --candidate "$CANDIDATE" --ollama-url "$OLLAMA_URL" --margin "$MARGIN" --report "$WORK/eval_report.json"
EVAL_RC=$?
set -e
if [[ $EVAL_RC -eq 2 ]]; then
    echo "REJECT: candidate not registered (see $WORK/eval_report.json)."
    exit 2
elif [[ $EVAL_RC -ne 0 ]]; then
    echo "Eval failed (exit $EVAL_RC); not registering." >&2
    exit "$EVAL_RC"
fi

# 5. PROMOTE: publish under the final name and register.
echo "[5/5] PROMOTE: publishing '$NAME' and registering ..."
ollama cp "$CANDIDATE" "$NAME"
"$XAVIER_BIN" mini-expert add --name "$NAME" --segment "$SEGMENT" --language "$LANGUAGE" \
    --clearance "$CLEARANCE" --source-dataset "$SOURCE_DATASET" --model-gguf-path "$GGUF" \
    --provider local --endpoint "$OLLAMA_URL/v1"
echo "=== Mini-expert '$NAME' promoted and registered. ==="
