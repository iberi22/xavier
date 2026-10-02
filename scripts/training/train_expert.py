#!/usr/bin/env python3
"""Single Xavier mini-expert trainer (MX-05).

  local:    PEFT LoRA on this machine (CUDA/ROCm if torch sees a GPU, else CPU),
            merge, GGUF f16, llama-quantize Q4_K_M, train_report.json.
  notebook: generate a self-contained Colab notebook + bundle.zip for the human.

No synthetic data, no placeholder artifacts: any failure exits non-zero.
"""
import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import expert_core as core  # noqa: E402

TEMPLATE = HERE / "templates" / "expert_notebook_template.ipynb"
PIP_PACKAGES = "transformers peft trl datasets accelerate sentencepiece gguf safetensors"


def _pinned_packages():
    """Use the versions from requirements.txt in the notebook as well."""
    req = HERE / "requirements.txt"
    pins = []
    if req.is_file():
        for line in req.read_text().splitlines():
            line = line.split("#")[0].strip()
            if line and not line.lower().startswith("torch"):
                pins.append(line)
    return " ".join(pins) if pins else PIP_PACKAGES


def _replace_in_cells(nb, mapping):
    for cell in nb["cells"]:
        src = "".join(cell["source"])
        for k, v in mapping.items():
            src = src.replace(k, v)
        cell["source"] = src.splitlines(True)


def generate_notebook(bundle_dir, summary, base_model, hp, out_dir):
    nb = json.loads(TEMPLATE.read_text(encoding="utf-8"))
    config = {
        "base_model": base_model,
        "hyperparams": hp,
        "expected_bundle_hash": summary["bundle_hash"],
    }
    # CONFIG_JSON goes into a raw ''' string; hashes/model ids contain no quotes.
    _replace_in_cells(
        nb,
        {
            "@@BASE_MODEL@@": base_model,
            "@@BUNDLE_HASH@@": summary["bundle_hash"],
            "@@PIP_PACKAGES@@": _pinned_packages(),
            "@@CONFIG_JSON@@": json.dumps(config),
            "@@EXPERT_CORE@@": (HERE / "expert_core.py").read_text(encoding="utf-8"),
        },
    )
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    nb_path = out / "train_expert_colab.ipynb"
    nb_path.write_text(json.dumps(nb, indent=1), encoding="utf-8")
    zip_path = core.zip_bundle(bundle_dir, out / "bundle.zip")
    return nb_path, zip_path


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bundle", required=True, help="bundle dir (bundle_manifest.json, train.jsonl, eval.jsonl)")
    ap.add_argument("--base-model", default=core.DEFAULT_BASE_MODEL)
    ap.add_argument("--out", required=True, help="output dir")
    ap.add_argument("--backend", choices=("local", "notebook"), default="local")
    ap.add_argument("--validate-only", action="store_true", help="only gate/validate the bundle (no torch needed)")
    ap.add_argument("--keep-intermediate", action="store_true", help="keep merged HF weights")
    d = core.DEFAULT_HYPERPARAMS
    ap.add_argument("--epochs", type=int, default=d["epochs"])
    ap.add_argument("--learning-rate", type=float, default=d["learning_rate"])
    ap.add_argument("--batch-size", type=int, default=d["batch_size"])
    ap.add_argument("--grad-accum", type=int, default=d["grad_accum"])
    ap.add_argument("--max-length", type=int, default=d["max_length"])
    ap.add_argument("--lora-r", type=int, default=d["lora_r"])
    ap.add_argument("--lora-alpha", type=int, default=d["lora_alpha"])
    ap.add_argument("--lora-dropout", type=float, default=d["lora_dropout"])
    ap.add_argument("--seed", type=int, default=d["seed"])
    a = ap.parse_args(argv)
    hp = {
        "epochs": a.epochs, "learning_rate": a.learning_rate, "batch_size": a.batch_size,
        "grad_accum": a.grad_accum, "max_length": a.max_length, "lora_r": a.lora_r,
        "lora_alpha": a.lora_alpha, "lora_dropout": a.lora_dropout, "seed": a.seed,
    }
    try:
        summary = core.validate_bundle(a.bundle, a.backend)
        if a.validate_only:
            print(json.dumps({"ok": True, **summary}, indent=2))
            return 0
        if a.backend == "notebook":
            nb, zp = generate_notebook(a.bundle, summary, a.base_model, hp, a.out)
            print("Notebook : %s\nBundle   : %s\n" % (nb, zp))
            print("HUMAN STEPS (free Colab, no Drive mount):")
            print("  1. Open https://colab.research.google.com > File > Upload notebook > %s" % nb)
            print("  2. Runtime > Change runtime type > T4 GPU")
            print("  3. Files panel (left) > upload %s to exactly /content/bundle.zip" % zp)
            print("  4. Runtime > Run all (several minutes: installs deps, builds llama-quantize)")
            print("  5. Files panel > /content/out/ > download model-q4_k_m.gguf and train_report.json")
            print("  6. Put both files back on the node (e.g. POST /v1/training/jobs/{id}/artifacts),")
            print("     then: ollama create + scripts/eval/expert_eval.py before promoting.")
            return 0
        report = core.run_pipeline(a.bundle, a.base_model, a.out, hp, "local", a.keep_intermediate)
        print(json.dumps(report, indent=2))
        print("OK: %s" % report["artifact"]["path"])
        return 0
    except core.ExpertError as e:
        print("ERROR: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
