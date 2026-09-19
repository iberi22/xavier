#!/usr/bin/env python3
"""Run offline document acceptance probes against the original Rust modules.

Uses an isolated, small Cargo package, not a reimplementation of the extractors.
Does not start Xavier, access its databases, or download dependencies. The report
and synthetic fixtures stay in the requested output directory outside the repo.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    out = (args.output or Path(tempfile.mkdtemp(prefix="xavier-legal-rag-"))).resolve()
    out.mkdir(parents=True, exist_ok=True)
    package = out / "probe"
    package.mkdir(exist_ok=True)
    deps = tomllib.loads((root / "Cargo.toml").read_text())["dependencies"]
    manifest = '[package]\nname = "xavier-legal-readiness-probe"\nversion = "0.0.0"\nedition = "2021"\n[workspace]\n[[bin]]\nname = "probe"\npath = "main.rs"\n[dependencies]\n'
    for name in ("anyhow", "serde", "serde_json", "sha2", "flate2", "regex", "image"):
        value = deps[name]
        if name == "image":
            # Only the two exercised codecs; no AVIF/video toolchain is needed.
            version = value if isinstance(value, str) else value["version"]
            manifest += f'image = {{ version = {json.dumps(version)}, default-features = false, features = ["png", "jpeg"] }}\n'
            continue
        if isinstance(value, str):
            manifest += f"{name} = {json.dumps(value)}\n"
        else:
            manifest += f'{name} = {{ version = {json.dumps(value["version"])}, features = {json.dumps(value.get("features", []))} }}\n'
    (package / "Cargo.toml").write_text(manifest)
    shutil.copyfile(root / "Cargo.lock", package / "Cargo.lock")
    # Include the repository's exact hex helpers without the unrelated crypto stack.
    crypto = (root / "src/crypto/mod.rs").read_text()
    start = crypto.index("const HEX_CHARS:")
    end = crypto.index("// ---------------------------------------------------------------------------", start)
    source = '#![allow(dead_code, unused_imports)]\n'
    source += f'mod crypto {{\n{crypto[start:end]}\n}}\n'
    source += f'#[path = {json.dumps(str(root / "src/documents/mod.rs"))}]\nmod documents;\n'
    source += (root / "scripts/eval/legal_rag_readiness.rs").read_text()
    (package / "main.rs").write_text(source)
    env = dict(os.environ, CARGO_NET_OFFLINE="true", CARGO_BUILD_JOBS="1")
    env["RUSTC_WRAPPER"] = ""
    env["CARGO_TARGET_DIR"] = str(out / "target")
    command = f"cargo run --quiet --manifest-path {shlex.quote(str(package / 'Cargo.toml'))} -- {shlex.quote(str(out))}"
    result = subprocess.run(["xavier", "exec", command], env=env, cwd=out)
    if result.returncode:
        return result.returncode
    # Reopen disk output and independently hash fixtures and evaluated source.
    report = json.loads((out / "report.json").read_text())
    report["source_sha256"] = {
        str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted((root / "src/documents").glob("*.rs"))
    }
    report["fixtures_sha256"] = {
        p.name: hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted((out / "fixtures").iterdir())
    }
    (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report["checks"], ensure_ascii=False, indent=2))
    print(f"Report: {out / 'report.json'}")
    # This is a readiness gate: failed acceptance criteria must not look green.
    return 0 if all(check["passed"] for check in report["checks"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
