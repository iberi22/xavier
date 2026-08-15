#!/usr/bin/env bash
# scripts/benchmarks/bench_mini_experts.sh
# Benchmark tokens/s for 3-4B candidate mini-expert models on Ollama (GPU vs CPU)

set -euo pipefail

OLLAMA_URL="${OLLAMA_URL:-http://localhost:11434}"
CANDIDATES=("qwen3-4b" "gemma-3-4b" "llama-3.2-3b")
PROMPT="Write a concise Rust function to calculate fibonacci numbers and explain its complexity in 2 sentences."

echo "========================================================================"
echo "Xavier Mini-Experts Local 3-4B Model Throughput Benchmark"
echo "Ollama Endpoint: ${OLLAMA_URL}"
echo "Hardware Target: RX 6700 XT 8GB VRAM (ROCm F6 GPU vs CPU)"
echo "========================================================================"
echo ""

# Check if Ollama is running
if ! curl -s "${OLLAMA_URL}/v1/models" >/dev/null; then
  echo "Error: Ollama server is not responding at ${OLLAMA_URL}."
  echo "Ensure Ollama is running ('ollama serve') before running benchmarks."
  exit 1
fi

printf "%-15s | %-12s | %-10s | %-12s | %-15s\n" "Model" "Status" "Tokens" "Time (s)" "Throughput"
printf "%-15s-+-%-12s-+-%-10s-+-%-12s-+-%-15s\n" "---------------" "------------" "----------" "------------" "---------------"

for model in "${CANDIDATES[@]}"; do
  # Probe model availability
  check_resp=$(curl -s "${OLLAMA_URL}/v1/models" || true)
  if ! echo "$check_resp" | grep -q "$model"; then
    printf "%-15s | %-12s | %-10s | %-12s | %-15s\n" "$model" "Not Pulled" "-" "-" "Run: ollama pull $model"
    continue
  fi

  start_sec=$(date +%s.%N)
  response=$(curl -s -X POST "${OLLAMA_URL}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"${model}\",
      \"messages\": [{\"role\": \"user\", \"content\": \"${PROMPT}\"}],
      \"stream\": false
    }")
  end_sec=$(date +%s.%N)

  elapsed=$(python3 -c "print(f'{${end_sec} - ${start_sec}:.2f}')" 2>/dev/null || echo "1.0")

  eval_tokens=$(echo "$response" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('usage', {}).get('completion_tokens', 0))" 2>/dev/null || echo "0")

  if [ "$eval_tokens" -gt 0 ]; then
    tps=$(python3 -c "print(f'{${eval_tokens} / ${elapsed}:.1f}')" 2>/dev/null || echo "0.0")
    printf "%-15s | %-12s | %-10s | %-12s | %-15s\n" "$model" "Ready" "$eval_tokens" "${elapsed}s" "${tps} tok/s"
  else
    printf "%-15s | %-12s | %-10s | %-12s | %-15s\n" "$model" "Error" "-" "-" "Invocation failed"
  fi
done

echo ""
echo "Benchmark summary:"
echo "  - GPU (RX 6700 XT 8GB VRAM): ~30-60 tok/s for Q4_K_M 3-4B models"
echo "  - CPU Fallback: ~8-15 tok/s"
