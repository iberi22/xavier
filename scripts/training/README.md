# Xavier mini-expert training (MX-05)

One trainer, two backends. Design: `docs/design/MINI-EXPERTOS-SISTEMA-v2.md` sec 3.6/3.7/6.

| File | Role |
|---|---|
| `train_expert.py` | CLI: `--bundle DIR --base-model ID --out DIR --backend local\|notebook [--validate-only]` |
| `expert_core.py` | Training core (stdlib at import; lazy torch). Inlined into the notebook. |
| `templates/expert_notebook_template.ipynb` | Colab template filled by `--backend notebook` |
| `../eval/expert_eval.py` | Base vs candidate on the fixed `eval.jsonl` via Ollama; PROMOTE/REJECT |
| `../mini-expert-train.sh` | Wrapper: export -> train -> `ollama create` -> eval -> register only on PROMOTE |
| `legacy/` | DEPRECATED scripts, not called by anything |

Bundle = dir with `bundle_manifest.json`, `train.jsonl`, `eval.jsonl` (+ `anonymization_audit.json`).
Records need a prompt/response pair (`prompt|instruction|question|input` + `response|output|answer|completion`,
or `messages`). Gating: P4/`local_only` never goes to `notebook`; P3 goes to `notebook` only with an
`anonymization_audit.json` carrying `"passed": true` (or `"status": "passed|approved|ok"`).
Note: the daemon's current audit file is a statistics summary without that flag, so P3 bundles are
refused for the notebook until the anonymization step emits an explicit pass.

## Install

```bash
uv venv .venv && source .venv/bin/activate
uv pip install -r scripts/training/requirements.txt
# torch (pick one; not pinned here):
uv pip install torch                                                        # CPU / CUDA default wheel
uv pip install torch --index-url https://download.pytorch.org/whl/rocm6.x   # AMD (use the real 6.x index)
export HSA_OVERRIDE_GFX_VERSION=10.3.0    # RX 6600-class (gfx1032) has no official ROCm target
```

llama.cpp (GGUF conversion): clone it, build `llama-quantize`, then
`export LLAMA_CPP_DIR=/path/to/llama.cpp` (optionally `LLAMA_QUANTIZE=/path/to/llama-quantize`).
Missing paths fail the run before training starts.

## Use

```bash
python3 scripts/training/train_expert.py --bundle B --out O --validate-only          # gate only, no torch
python3 scripts/training/train_expert.py --bundle B --out O --backend local          # writes O/model-q4_k_m.gguf + O/train_report.json
python3 scripts/training/train_expert.py --bundle B --out O --backend notebook       # writes notebook + bundle.zip, prints human steps
scripts/mini-expert-train.sh --name my-expert --segment codebase/xavier              # whole pipeline
```

Exit codes: any failure is non-zero; `expert_eval.py` returns 0 PROMOTE, 2 REJECT, 1 error.
Notebook runs: upload `bundle.zip` to `/content/bundle.zip` in Colab, Run all, download the GGUF and
`train_report.json`, then `mini-expert-train.sh --name X --bundle B --resume-gguf model-q4_k_m.gguf`.
`fixtures/tiny_bundle` is a hand-made P3 bundle with a stats-only audit (used to prove gating).
