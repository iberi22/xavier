#!/usr/bin/env python3
"""
run_ab_experiment.py — Experimento subagent vivo A/B: Xavier como cerebro.

Escenario SWAL multi-sesion (requiere memoria):
  - Sesión A (escritura): el subagent documenta un hecho NUEVO y lo guarda en Xavier.
  - Sesión B (lectura, contexto fresco): se le pregunta por ese hecho. Solo puede
    saberlo si consulta Xavier (su contexto interno se descarta entre sesiones).

Grupos:
  - EXPERIMENTAL (con Xavier): subagent tiene wiring MCP + brain prompt.
  - CONTROL (sin Xavier): subagent sin wiring. Deberia decir "no lo se" o alucinar.

El experimento usa un HECHO SINTETICO unico por corrida (ej: un nombre de color
inventado "zafiro-quantico-7") para evitar que el modelo lo sepa de antemano.

Mide:
  - recall_correct: ¿la Sesión B respondio el hecho correcto?
  - tool_used: ¿la Sesión B llamo a Xavier antes de responder? (se infiere del output)
  - persisted: ¿la Sesión A guardo el hecho en Xavier? (se verifica via get_memory)
  - alucination: ¿la Sesión B invento una respuesta sin证据?

Uso:
  python run_ab_experiment.py --cli opencode
  python run_ab_experiment.py --cli gemini --runs 3
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REPORTS_DIR = REPO_ROOT / "scripts" / "subagents" / "reports"
REPORTS_DIR.mkdir(parents=True, exist_ok=True)
BRAIN_PROMPT = Path(__file__).resolve().parent / "xavier_brain_prompt.md"

BASE = os.environ.get("XAVIER_BASE_URL", "http://localhost:8006")


def read_token() -> str:
    for line in (REPO_ROOT / ".env").read_text(encoding="utf-8").splitlines():
        if line.startswith("XAVIER_TOKEN=") and not line.startswith("#"):
            return line.split("=", 1)[1].strip()
    sys.exit("no token")


def http(path: str, token: str, body: dict | None = None):
    req = urllib.request.Request(f"{BASE}{path}", method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("X-Xavier-Token", token)
    data = json.dumps(body).encode() if body else None
    with urllib.request.urlopen(req, data=data, timeout=30) as r:
        return json.loads(r.read().decode())


# ---------------------------------------------------------------------------
# Hecho sintetico (no pre-existe en el modelo)
# ---------------------------------------------------------------------------
SYNTH_FACTS = [
    ("config-magma-9", "La configuracion del modulo magma usa prioridad 9 y modo estricto"),
    ("token-coral-42", "El token coral de integracion tiene caducidad 42 dias y scope read-only"),
    ("flag-zafiro-7", "El flag experimental zafiro esta activado en el build 7 con latencia 120ms"),
    ("clave-berilo-3", "La clave berilo de rotacion se ejecuta cada 3 horas en zona eu-west"),
    ("proto-nepal-11", "El protocolo nepal version 11 requiere handshake bidireccional obligatorio"),
]


def synth_fact(run_idx: int) -> tuple[str, str]:
    return SYNTH_FACTS[run_idx % len(SYNTH_FACTS)]


# ---------------------------------------------------------------------------
# Prompts por sesion
# ---------------------------------------------------------------------------
def session_a_prompt(fact_key: str, fact_value: str, with_xavier: bool) -> str:
    base = BRAIN_PROMPT.read_text(encoding="utf-8") if with_xavier else ""
    instruction = (
        f"Tarea de DOCUMENTACION: acabo de decidir un hecho tecnico nuevo que debes registrar.\n\n"
        f"HECHO: {fact_value}\n\n"
    )
    if with_xavier:
        instruction += (
            f"Instrucciones: usa la herramienta create_memory para guardar este hecho en Xavier.\n"
            f"  - path: \"ab-experiment/{fact_key}\"\n"
            f"  - content: \"{fact_value}\"\n"
            f"  - kind: \"decision\"\n"
            f"Despues de guardarlo, confirma con el ID devuelto. Responde SOLO con el ID o 'ERROR'.\n"
        )
    else:
        instruction += (
            f" Instrucciones: simplemente confirma que registraste el hecho respondiendo 'REGISTRADO: {fact_key}'. "
            f"No tienes acceso a memoria externa.\n"
        )
    return (base + "\n\n" + instruction) if base else instruction


def session_b_prompt(fact_key: str, with_xavier: bool) -> str:
    base = BRAIN_PROMPT.read_text(encoding="utf-8") if with_xavier else ""
    question = (
        f"Pregunta: Cual es el valor/contenido asociado a la clave tecnica '{fact_key}'?\n\n"
    )
    if with_xavier:
        question += (
            f"Instrucciones: PRIMERO usa la herramienta mem_search (o search_memory) con "
            f"query=\"{fact_key}\" y filters={{\"path_prefix\":\"ab-experiment/\"}}.\n"
            f"Despues responde citando el contenido encontrado, o 'NO_LO_SE' si no hay resultado.\n"
        )
    else:
        question += (
            f" Responde con lo que sepas o 'NO_LO_SE' si no tienes informacion al respecto.\n"
        )
    return (base + "\n\n" + question) if base else question


# ---------------------------------------------------------------------------
# Lanzar CLI
# ---------------------------------------------------------------------------
def run_opencode(prompt: str, mcp_config: Path | None, timeout: int = 180) -> tuple[str, int]:
    workdir = Path(tempfile.mkdtemp(prefix="xavier-ab-"))  # type: ignore # noqa
    args = ["opencode", "run", "--auto", "--format", "json", "--dir", str(workdir), prompt]
    # opencode lee opencode.json o .mcp.json en el workdir
    if mcp_config and mcp_config.exists():
        target = workdir / "opencode.json"
        target.write_text(mcp_config.read_text(encoding="utf-8"), encoding="utf-8")
    return _exec(args, timeout)


def run_gemini(prompt: str, mcp_config: Path | None, timeout: int = 180) -> tuple[str, int]:
    workdir = Path(tempfile.mkdtemp(prefix="xavier-ab-"))  # type: ignore # noqa
    if mcp_config and mcp_config.exists():
        gemini_dir = workdir / ".gemini"
        gemini_dir.mkdir(exist_ok=True)
        (gemini_dir / "settings.json").write_text(mcp_config.read_text(encoding="utf-8"), encoding="utf-8")
    args = ["gemini", "-p", prompt, "-y"]
    return _exec(args, timeout, cwd=workdir)


def run_codex(prompt: str, mcp_config: Path | None, timeout: int = 180) -> tuple[str, int]:
    # codex no soporta MCP; el mcp_config aqui es el system-prompt fallback
    sp_flag = []
    if mcp_config and mcp_config.exists():
        # usar el system-prompt de codex (curl fallback) + el prompt de tarea
        fallback = mcp_config.read_text(encoding="utf-8")
        prompt = fallback + "\n\n" + prompt
    args = ["codex", "exec", prompt, "-m", "gpt-5.3-codex", "--dangerously-bypass-approvals-and-sandbox"]
    return _exec(args, timeout)


CLI_RUNNERS = {"opencode": run_opencode, "gemini": run_gemini, "codex": run_codex}


def _resolve_cli(name: str) -> str:
    """Resuelve el binario real del CLI (Windows npm usa .cmd shims)."""
    resolved = shutil.which(name)
    if resolved:
        return resolved
    # fallback: probar .cmd en el npm global bin
    if os.name == "nt":
        npm_bin = os.path.expanduser("~/AppData/Roaming/npm")
        for ext in (".cmd", ".ps1", ""):
            cand = os.path.join(npm_bin, name + ext)
            if os.path.exists(cand):
                return cand
    return name


def _exec(args: list[str], timeout: int, cwd: Path | None = None) -> tuple[str, int]:
    # resolver el ejecutable (primer arg) para .cmd shims en Windows
    if args:
        args = [_resolve_cli(args[0])] + args[1:]
    try:
        r = subprocess.run(args, capture_output=True, text=True, timeout=timeout, cwd=cwd, shell=False)
        return (r.stdout or "") + (r.stderr or ""), r.returncode
    except subprocess.TimeoutExpired:
        return "TIMEOUT", -1
    except FileNotFoundError:
        return f"CLI_NOT_FOUND: {args[0]}", -2


# ---------------------------------------------------------------------------
# Verificacion via Xavier HTTP
# ---------------------------------------------------------------------------
def verify_persisted(token: str, fact_key: str, fact_value: str) -> dict:
    """Comprueba si la Sesión A guardo el hecho en Xavier."""
    try:
        r = http("/memory/search", token, {
            "query": fact_key,
            "limit": 5,
            "filters": {"path_prefix": "ab-experiment/"},
        })
        results = r.get("results", [])
        found = any(fact_value[:30] in (res.get("content", "") or "") for res in results)
        return {"persisted": found, "n_results": len(results), "sample": [res.get("content", "")[:80] for res in results[:3]]}
    except Exception as e:
        return {"persisted": False, "error": str(e)}


# ---------------------------------------------------------------------------
# Orquestador
# ---------------------------------------------------------------------------
def run_one(cli: str, run_idx: int, with_xavier: bool, token: str, mcp_config: Path | None) -> dict:
    fact_key, fact_value = synth_fact(run_idx)
    runner = CLI_RUNNERS[cli]
    tag = "WITH_XAVIER" if with_xavier else "CONTROL"
    result = {
        "cli": cli, "run": run_idx, "group": tag,
        "fact_key": fact_key, "fact_value": fact_value,
    }

    # --- Sesión A: escritura ---
    a_prompt = session_a_prompt(fact_key, fact_value, with_xavier)
    t0 = time.time()
    a_out, a_rc = runner(a_prompt, mcp_config)
    result["session_a"] = {"rc": a_rc, "duration_s": round(time.time() - t0, 1), "output_tail": a_out[-400:]}
    result["session_a"]["persisted_in_xavier"] = verify_persisted(token, fact_key, fact_value) if with_xavier else None

    # limpieza del contexto: SIEMPRE lanzamos sesion B como proceso fresco (workdir nuevo)

    # --- Sesión B: lectura (contexto fresco) ---
    time.sleep(1)
    b_prompt = session_b_prompt(fact_key, with_xavier)
    t1 = time.time()
    b_out, b_rc = runner(b_prompt, mcp_config)
    result["session_b"] = {"rc": b_rc, "duration_s": round(time.time() - t1, 1), "output_tail": b_out[-600:]}

    # --- Scoring ---
    out_l = b_out.lower()
    # recall_correct: el valor del hecho aparece en la respuesta de sesion B
    recall_correct = fact_value[:20].lower() in out_l or any(w.lower() in out_l for w in fact_value.split()[:3] if len(w) > 4)
    # alucination: responde algo distinto a NO_LO_SE sin tener el valor
    said_dont_know = "no_lo_se" in out_l or "no lo sé" in out_l or "no lo se" in out_l or "i don't know" in out_l or "no tengo" in out_l
    # tool_used: indicios de que llamo a Xavier (mencion de la herramienta o del resultado)
    tool_used = ("mem_search" in out_l or "create_memory" in out_l or "xavier" in out_l
                 or "path_prefix" in out_l or "ab-experiment" in out_l)

    result["metrics"] = {
        "recall_correct": recall_correct,
        "said_dont_know": said_dont_know,
        "tool_used_evidence": tool_used,
        "alucination": (not recall_correct and not said_dont_know),
    }
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", choices=list(CLI_RUNNERS), default="opencode")
    ap.add_argument("--runs", type=int, default=1, help="Numero de hechos sinteticos a probar")
    ap.add_argument("--groups", default="both", choices=["both", "with", "control"])
    args = ap.parse_args()

    token = read_token()
    # health
    try:
        urllib.request.urlopen(f"{BASE}/health", timeout=5)
    except Exception:
        sys.exit(f"Xavier no responde en {BASE}")

    mcp_dir = REPO_ROOT / "scripts" / "subagents" / "mcp" / "GENERATED"
    mcp_map = {"opencode": mcp_dir / "opencode.mcp.json", "gemini": mcp_dir / "gemini.settings.json", "codex": mcp_dir / "codex.system-prompt.md"}
    mcp_config = mcp_map[args.cli]
    if not mcp_config.exists():
        print(f"[warn] {mcp_config} no existe. Genera primero: python scripts/subagents/generate_mcp_configs.py")

    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    results = []
    groups = ["with", "control"] if args.groups == "both" else [args.groups]
    for g in groups:
        for i in range(args.runs):
            with_x = (g == "with")
            print(f"\n=== {args.cli} | group={g.upper()} | run={i} ===")
            r = run_one(args.cli, i, with_x, token, mcp_config)
            results.append(r)
            m = r["metrics"]
            print(f"  recall_correct={m['recall_correct']} tool_used={m['tool_used_evidence']} dont_know={m['said_dont_know']} aluc={m['alucination']}")

    # reporte
    report_path = REPORTS_DIR / f"ab_experiment_{args.cli}_{ts}.json"
    report_path.write_text(json.dumps({"cli": args.cli, "ts": ts, "results": results}, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n[report] {report_path}")

    # resumen
    def agg(grp):
        items = [r for r in results if r["group"].lower().startswith(grp)]
        if not items:
            return None
        return {
            "n": len(items),
            "recall": sum(1 for r in items if r["metrics"]["recall_correct"]) / len(items),
            "tool_used": sum(1 for r in items if r["metrics"]["tool_used_evidence"]) / len(items),
            "alucination": sum(1 for r in items if r["metrics"]["alucination"]) / len(items),
        }
    print("\n=== RESUMEN ===")
    for g in groups:
        a = agg(g)
        if a:
            print(f"  {g:8s}: n={a['n']} recall={a['recall']:.2f} tool_used={a['tool_used']:.2f} aluc={a['alucination']:.2f}")


if __name__ == "__main__":
    main()
