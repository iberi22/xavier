"""Training core for Xavier mini-experts (MX-05).

Single code path used by BOTH backends:
  * local:    imported by scripts/training/train_expert.py
  * notebook: this file's source is inlined verbatim into the generated .ipynb

Rules: standard library only at import time (heavy deps are imported lazily inside
the functions that need them), no relative imports, no synthetic data, no fake
artifacts. Any failure raises ExpertError -> the caller exits non-zero.
"""
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import zipfile
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_BASE_MODEL = "Qwen/Qwen2.5-0.5B-Instruct"
BUNDLE_FILES = ("bundle_manifest.json", "train.jsonl", "eval.jsonl")
AUDIT_FILE = "anonymization_audit.json"
# Privacy levels that may leave the node (only with the rules in validate_bundle).
REMOTE_OK_LEVELS = ("P0", "P1", "P2", "P3")
AUDIT_PASS_STATUSES = ("passed", "pass", "approved", "ok")

DEFAULT_HYPERPARAMS = {
    "epochs": 3,
    "learning_rate": 2e-4,
    "batch_size": 2,
    "grad_accum": 4,
    "max_length": 1024,
    "lora_r": 16,
    "lora_alpha": 32,
    "lora_dropout": 0.05,
    "seed": 42,
}


class ExpertError(RuntimeError):
    """Any pipeline failure. Never swallowed: callers exit non-zero."""


# --------------------------------------------------------------------------- bundle


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def bundle_hash(bundle_dir):
    """sha256 over manifest + train + eval bytes (fixed order, name-prefixed)."""
    h = hashlib.sha256()
    for name in BUNDLE_FILES:
        p = Path(bundle_dir) / name
        if not p.is_file():
            raise ExpertError("bundle file missing: %s" % p)
        h.update(name.encode() + b"\0")
        h.update(bytes.fromhex(sha256_file(p)))
    return h.hexdigest()


def _read_json(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise ExpertError("cannot read JSON %s: %s" % (path, e))


def _count_lines(path):
    n = 0
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            if line.strip():
                n += 1
    return n


def extract_pair(rec):
    """Return (prompt, answer) from a bundle record, or None if unusable.

    Accepted shapes: {prompt|instruction|question|input, response|output|answer|completion},
    {messages:[{role,content}...]} (last user -> last assistant).
    """
    if not isinstance(rec, dict):
        return None
    msgs = rec.get("messages")
    if isinstance(msgs, list):
        user = [m.get("content") for m in msgs if isinstance(m, dict) and m.get("role") == "user"]
        asst = [m.get("content") for m in msgs if isinstance(m, dict) and m.get("role") == "assistant"]
        if user and asst and isinstance(user[-1], str) and isinstance(asst[-1], str):
            return user[-1], asst[-1]
        return None
    prompt = None
    for k in ("prompt", "instruction", "question", "input"):
        if isinstance(rec.get(k), str) and rec[k].strip():
            prompt = rec[k]
            break
    answer = None
    for k in ("response", "output", "answer", "completion"):
        if isinstance(rec.get(k), str) and rec[k].strip():
            answer = rec[k]
            break
    if prompt is None or answer is None:
        return None
    return prompt, answer


def read_pairs(jsonl_path):
    pairs = []
    with open(jsonl_path, "r", encoding="utf-8") as f:
        for i, line in enumerate(f, 1):
            if not line.strip():
                continue
            try:
                rec = json.loads(line)
            except ValueError as e:
                raise ExpertError("%s line %d: invalid JSON (%s)" % (jsonl_path, i, e))
            pair = extract_pair(rec)
            if pair is None:
                raise ExpertError(
                    "%s line %d: record has no usable prompt/response pair" % (jsonl_path, i)
                )
            pairs.append(pair)
    return pairs


def validate_bundle(bundle_dir, backend):
    """Gate a bundle for a backend. Returns a summary dict or raises ExpertError.

    Rules (design v2 sec 3.4 / 6):
      * P4 or `local_only` bundles never go to a remote backend (notebook).
      * P3 bundles may go to the notebook only with a passing anonymization_audit.json
        (`"passed": true` or `"status"` in passed/pass/approved/ok).
      * Unknown privacy levels are refused for notebook.
    """
    d = Path(bundle_dir)
    if not d.is_dir():
        raise ExpertError("bundle dir not found: %s" % d)
    for name in BUNDLE_FILES:
        if not (d / name).is_file():
            raise ExpertError("bundle is missing %s" % name)
    manifest = _read_json(d / "bundle_manifest.json")
    if not isinstance(manifest, dict):
        raise ExpertError("bundle_manifest.json must be a JSON object")
    level = str(manifest.get("privacy_level", "P2")).upper()
    local_only = bool(manifest.get("local_only", False))
    audit_ok = None
    if backend == "notebook":
        if level == "P4" or local_only:
            raise ExpertError(
                "REFUSED: bundle is P4/local_only; it must never leave the node (backend=notebook)"
            )
        if level not in REMOTE_OK_LEVELS:
            raise ExpertError("REFUSED: unknown privacy_level %r for backend=notebook" % level)
        if level == "P3":
            audit_path = d / AUDIT_FILE
            if not audit_path.is_file():
                raise ExpertError("REFUSED: P3 bundle without %s (backend=notebook)" % AUDIT_FILE)
            audit = _read_json(audit_path)
            status = str(audit.get("status", "")).lower() if isinstance(audit, dict) else ""
            passed = isinstance(audit, dict) and (
                audit.get("passed") is True or status in AUDIT_PASS_STATUSES
            )
            if not passed:
                raise ExpertError(
                    "REFUSED: P3 bundle audit did not pass (need passed=true or status in %s)"
                    % (AUDIT_PASS_STATUSES,)
                )
            audit_ok = True
    elif backend != "local":
        raise ExpertError("unknown backend %r" % backend)
    n_train = _count_lines(d / "train.jsonl")
    n_eval = _count_lines(d / "eval.jsonl")
    if n_train == 0:
        raise ExpertError("train.jsonl is empty: refusing to train (no synthetic fallback)")
    counts = manifest.get("split_counts")
    if isinstance(counts, dict):
        if counts.get("train") not in (None, n_train) or counts.get("eval") not in (None, n_eval):
            raise ExpertError(
                "manifest split_counts %s disagree with files (train=%d eval=%d)"
                % (counts, n_train, n_eval)
            )
    read_pairs(d / "train.jsonl")  # every train record must be usable
    return {
        "privacy_level": level,
        "local_only": local_only,
        "audit_passed": audit_ok,
        "train_examples": n_train,
        "eval_examples": n_eval,
        "bundle_hash": bundle_hash(d),
        "backend": backend,
    }


def zip_bundle(bundle_dir, zip_path):
    """Zip the bundle files (flat) for upload to the Colab file panel."""
    d = Path(bundle_dir)
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for name in BUNDLE_FILES + (AUDIT_FILE,):
            if (d / name).is_file():
                z.write(d / name, name)
    return str(zip_path)


def unzip_bundle(zip_path, dest):
    if not Path(zip_path).is_file():
        raise ExpertError("bundle zip not found at %s" % zip_path)
    dest = Path(dest)
    dest.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(zip_path) as z:
        for info in z.infolist():
            if "/" in info.filename or "\\" in info.filename or info.filename.startswith("."):
                raise ExpertError("unexpected path in bundle zip: %s" % info.filename)
        z.extractall(dest)
    return str(dest)


# --------------------------------------------------------------------------- training


def detect_device():
    """'cuda' (also ROCm builds of torch) if torch sees a GPU, else 'cpu'."""
    try:
        import torch
    except ImportError:
        raise ExpertError("torch is not installed (see scripts/training/README.md)")
    return "cuda" if torch.cuda.is_available() else "cpu"


def train_lora(bundle_dir, base_model, out_dir, hp):
    """LoRA SFT with current PEFT + TRL. Returns (merged_dir, info)."""
    try:
        import torch
        from datasets import Dataset
        from peft import LoraConfig
        from transformers import AutoModelForCausalLM, AutoTokenizer
        from trl import SFTConfig, SFTTrainer
    except ImportError as e:
        raise ExpertError("missing training dependency: %s (pip install -r requirements.txt)" % e)

    device = detect_device()
    pairs = read_pairs(Path(bundle_dir) / "train.jsonl")
    rows = [
        {
            "prompt": [{"role": "user", "content": p}],
            "completion": [{"role": "assistant", "content": a}],
        }
        for p, a in pairs
    ]
    dataset = Dataset.from_list(rows)

    tokenizer = AutoTokenizer.from_pretrained(base_model)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
    # fp32 master weights + fp16 autocast on GPU: stable with LoRA on T4 and gfx1032.
    model = AutoModelForCausalLM.from_pretrained(base_model, dtype=torch.float32)

    lora = LoraConfig(
        r=hp["lora_r"],
        lora_alpha=hp["lora_alpha"],
        lora_dropout=hp["lora_dropout"],
        target_modules="all-linear",
        task_type="CAUSAL_LM",
    )
    work = Path(out_dir) / "trainer_work"
    cfg = SFTConfig(
        output_dir=str(work),
        num_train_epochs=hp["epochs"],
        per_device_train_batch_size=hp["batch_size"],
        gradient_accumulation_steps=hp["grad_accum"],
        learning_rate=hp["learning_rate"],
        max_length=hp["max_length"],
        seed=hp["seed"],
        fp16=(device == "cuda"),
        use_cpu=(device == "cpu"),
        logging_steps=5,
        save_strategy="no",
        report_to="none",
    )
    trainer = SFTTrainer(
        model=model,
        args=cfg,
        train_dataset=dataset,
        processing_class=tokenizer,
        peft_config=lora,
    )
    trainer.train()
    losses = [h["loss"] for h in trainer.state.log_history if "loss" in h]
    merged = trainer.model.merge_and_unload()
    merged = merged.to(torch.float16)
    merged_dir = Path(out_dir) / "merged_hf"
    merged.save_pretrained(str(merged_dir), safe_serialization=True)
    tokenizer.save_pretrained(str(merged_dir))
    shutil.rmtree(work, ignore_errors=True)
    return str(merged_dir), {
        "device": device,
        "train_examples": len(rows),
        "final_loss": losses[-1] if losses else None,
        "first_loss": losses[0] if losses else None,
        "torch": torch.__version__,
    }


def _llama_paths():
    base = os.environ.get("LLAMA_CPP_DIR", "").strip()
    if not base or not Path(base).is_dir():
        raise ExpertError(
            "LLAMA_CPP_DIR is unset or not a directory (need a llama.cpp checkout with "
            "convert_hf_to_gguf.py and a built llama-quantize)"
        )
    convert = Path(base) / "convert_hf_to_gguf.py"
    if not convert.is_file():
        raise ExpertError("convert_hf_to_gguf.py not found in %s" % base)
    quant = os.environ.get("LLAMA_QUANTIZE", "").strip()
    candidates = [quant] if quant else []
    candidates += [str(Path(base) / "build" / "bin" / "llama-quantize"), str(Path(base) / "llama-quantize")]
    found = next((c for c in candidates if c and Path(c).is_file()), None) or shutil.which("llama-quantize")
    if not found:
        raise ExpertError(
            "llama-quantize not found (build it in $LLAMA_CPP_DIR/build/bin, set LLAMA_QUANTIZE, or put it on PATH)"
        )
    return str(convert), found


def check_gguf(path):
    p = Path(path)
    if not p.is_file() or p.stat().st_size < 1024 * 1024:
        raise ExpertError("GGUF %s missing or implausibly small" % p)
    with open(p, "rb") as f:
        if f.read(4) != b"GGUF":
            raise ExpertError("%s is not a GGUF file (bad magic)" % p)


def _run(cmd):
    print("+ " + " ".join(cmd), flush=True)
    r = subprocess.run(cmd)
    if r.returncode != 0:
        raise ExpertError("command failed (exit %d): %s" % (r.returncode, cmd[0]))


def convert_to_gguf(merged_dir, out_dir):
    """HF -> GGUF f16 (convert_hf_to_gguf.py) -> Q4_K_M (llama-quantize)."""
    convert, quantize = _llama_paths()
    f16 = Path(out_dir) / "model-f16.gguf"
    q4 = Path(out_dir) / "model-q4_k_m.gguf"
    _run([sys.executable, convert, str(merged_dir), "--outfile", str(f16), "--outtype", "f16"])
    check_gguf(f16)
    _run([quantize, str(f16), str(q4), "Q4_K_M"])
    check_gguf(q4)
    f16.unlink()
    return str(q4)


def run_pipeline(bundle_dir, base_model, out_dir, hp, backend, keep_intermediate=False):
    """validate -> train -> merge -> GGUF f16 -> Q4_K_M -> train_report.json."""
    hp = dict(DEFAULT_HYPERPARAMS, **(hp or {}))
    summary = validate_bundle(bundle_dir, backend)
    _llama_paths()  # fail early, before spending hours training
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    t0 = time.time()
    started = datetime.now(timezone.utc).isoformat()
    merged_dir, info = train_lora(bundle_dir, base_model, out, hp)
    gguf = convert_to_gguf(merged_dir, out)
    if not keep_intermediate:
        shutil.rmtree(merged_dir, ignore_errors=True)
    report = {
        "base_model": base_model,
        "backend": backend,
        "bundle_hash": summary["bundle_hash"],
        "privacy_level": summary["privacy_level"],
        "eval_examples": summary["eval_examples"],
        "hyperparams": hp,
        "started_at": started,
        "duration_s": round(time.time() - t0, 1),
        "artifact": {
            "path": gguf,
            "file": Path(gguf).name,
            "quantization": "Q4_K_M",
            "size_bytes": Path(gguf).stat().st_size,
            "sha256": sha256_file(gguf),
        },
        "python": platform.python_version(),
    }
    report.update(info)
    (out / "train_report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report
