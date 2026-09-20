---
name: xavier-jules-wave
description: "XAVIER Rust harness — Jules wave issues profesionales con template canónico Rust (cargo check/clippy/test/fmt) + PR Delivery guard + islas disjuntas. Hereda de github/gitcore-jules-issues v3.0 y lo adapta a Xavier (Rust, 1M ctx, xhigh). OBLIGATORIO para toda ola SWAL/Xavier."
version: 1.0.0
author: Hermes + BELA
tags: [xavier, rust, jules, wave, harness, gitcore, cargo, clippy]
---

# Xavier Jules Wave — Harness Rust Profesional (v1.0)

> Hereda de `github/gitcore-jules-issues` v3.0. Este archivo es **OBLIGATORIO** para toda ola Xavier (Rust). Si trabajas en `~/proyectosSWAL/apps/xavier`, **LEE ESTE ARCHIVO PRIMERO** antes de crear cualquier issue para Jules.

## 0. Cuándo usar este skill

- Crear cualquier issue para Jules en `iberi22/xavier` (wave-4, wave-5, etc.)
- Preparar olas con 10-15 issues paralelos
- Auditar issues existentes contra el template canónico

**Siempre** carga este skill con `skill_view(name='xavier-jules-wave')` antes de `gh issue create`.

## 1. Título canónico (OBLIGATORIO)

```
# [WAVE-N.XX] feat-kebab-case — Human-Readable Title (max 72 chars)

> Wave N — Hardening/Infra/Core. Labels: `wave-N`, `olaN` (sin `jules` todavía)
> Merge order: X/10 | Risk: LOW/MED/HIGH | Effort: Small/Medium/Large
```

- Ej: `# [WAVE-4.03] feat-mesh-service-network — INTERNAL publish/consume + personal data exclusion`
- El prefijo `feat-` es **OBLIGATORIO** (para que `gitcore` lo mapee a `feat-*` en features.json)
- No usar `[WAVE-4.03]` sin `feat-`, no usar `Ola N.XX` genérico

## 2. Template canónico Rust/Xavier — 11 secciones en orden exacto

Cada issue DEBE tener **TODAS** estas secciones en **ESTE orden**. Sin excepciones. Copia/pega este esqueleto y rellena:

```markdown
# [WAVE-N.XX] feat-kebab — Title

> Wave N — Hardening. Labels: `wave-N`, `olaN` (sin `jules` todavía)
> Merge order: X/10 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

- File: `src/path/to/file.rs` (N lines, structs: Foo, Bar, fn: baz)
- Feature: `feat-xyz` at N% in `.gitcore/features.json` (beta/stable, REQ-XXX)
- Tests: N existing, N passing in `cargo test --lib <filter>`
- Handler: `src/adapters/inbound/http/handlers/...` (si aplica)

## Desired State (DELTA)

- **Section A** (lines XX-YY): Add struct `Foo` + fn `bar`
- **New file**: `src/path/to/new.rs` with `pub struct NewThing`
- **New test**: ` cargo test --lib <filter> -- --nocapture` con N casos
- Risk: MED — descripción del riesgo

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "Rust axum handler 2025"
2. search: "Rust cargo test pattern 2025"
3. search: "Xavier <feature> Rust 2025"
4. search: "Rust clippy fix for <lint> 2025"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand these existing files:
   - `src/path/to/existing.rs` — note the pattern used
   - `src/adapters/inbound/http/routes.rs` — understand axum routing
3. Execute according to the 3 internal micro-phases:
   - **Phase 1: Types & Contracts**: Declare structs, enums, error types (`thiserror`), and traits. Keep initial surface minimal.
   - **Phase 2: Core Logic**: Implement isolated methods, zero-alloc state changes, avoiding excessive complexity.
   - **Phase 3: Tests & Delivery**: Add isolated tests in `#[cfg(test)] mod tests`, verify with `cargo check`, and create the PR immediately.
4. **Blocker Recovery**: If compiler errors or test failures arise, search the web immediately for the exact error signature. Document fixes in the PR body. NEVER pause or stall waiting for feedback."

## Existing Code Patterns (MUST follow)

- `src/adapters/inbound/http/handlers/memory.rs` → axum Handler + State extraction + Result<Json>
- `src/memory/store.rs` → MemoryStore trait + async_trait + anyhow::Result
- `src/security/clearance.rs` → ClearanceLevel + ClearanceEnforcer

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check -p xavier --all-targets 2>&1 | grep ^error | wc -l` == 0
- [ ] `grep -c "MyStruct\|my_fn" src/path/to/file.rs` >= 1
- [ ] `cargo test --lib <filter> -- --nocapture 2>&1 | grep ok` >= 1
- [ ] `cargo clippy -- -D warnings 2>&1 | grep "^error" | wc -l` == 0
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/path/to/file.rs` | N lines, 3 fns | Add `MyStruct` + `my_fn` | MED |
| `src/path/to/new.rs` | NEW | Create `NewThing` | LOW |

## DO NOT touch (Anti-Regression)

- `crates/xavier-core-logic` — unless explicitly listed above
- `.gitcore/features.json` — reconciled at wave end only
- `Cargo.lock` unless adding dep with `cargo add` — then include reason
- `src/memory/store.rs` — unless explicitly listed (isla disjunta)
- No new dependencies without updating `.env.example` if env var added

## Anti-Hallucination Guard ⚠️

1. **READ before write**: Leer archivo COMPLETO (`read_file` con offset/limit) antes de modificar
2. **Match existing patterns**: Usar mismo estilo (thiserror/anyhow, tokio::spawn_blocking, `anyhow::Result`)
3. **No inventar imports**: Verificar que crates existen en `Cargo.toml` (ej. `serde`, `tokio`, `axum`)
4. **Cargo check gate**: `cargo check -p xavier --all-targets` debe ser 0 errores antes de commit
5. **No tocar `.gitcore/features.json`**: Reconciliado al final de wave por el orquestador

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` muestra archivos nuevos/modificados ANTES de abrir PR
- [ ] `git diff --stat HEAD` lista archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar `git ls-files` antes de push
- [ ] SI el trabajo no se pudo completar: NO abrir PR — comentar blocker en issue
- [ ] `git show HEAD --name-only | grep -cE "src/|crates/"` >= 1 (archivos fuente reales, no solo package.json)
- [ ] `wc -l src/path/to/new.rs` >= 20 (archivo NO vacío si crea nuevo)
- [ ] `grep -c "fn test_\|#\[test\]" src/path/to/file.rs` >= 2 (tests reales)

## Verification

```bash
cargo check -p xavier --all-targets 2>&1 | grep ^error | wc -l  # expect 0
cargo clippy --all-targets -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --lib <filter> -- --nocapture 2>&1 | tail -5  # N passed
cargo fmt --check  # 0 diff
```

## Dependencies & Merge Order

- **Depends on:** WAVE-N-1 (commit hash)
- **Parallel with:** WAVE-N other X (disjoint islands verified)
- **Merge order within wave:** X/10
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| `cargo test` fails on sqlite | Try with `CARGO_TARGET_DIR=target`; if still fails, env not code |
| File doesn't exist | `find . -name "filename" 2>/dev/null` |
| Test fails on new code | Fix logic or test |
| PR conflicts with parallel work | Rebase on main (`git pull --rebase origin main`), re-run verification |
| Missing dep in Cargo.toml | `cargo add <dep>` + update `.env.example` if needed |
| Clippy -D warnings fails | Fix lint or add `#[allow(clippy::...)]` with comment |
```

## 3. Diferencias clave vs template genérico Dart

| Genérico (Dart) | Xavier Rust |
|-----------------|-------------|
| `dart analyze lib/...` | `cargo check -p xavier --all-targets` + `cargo clippy -- -D warnings` |
| `flutter test` | `cargo test --lib <filter> -- --nocapture` |
| `lib/vault/service.dart` | `src/adapters/inbound/http/handlers/...` + `src/memory/store.rs` |
| `pubspec.yaml` | `Cargo.toml` + `Cargo.lock` |
| `tests/foo.spec.ts` | `src/path/to/file.rs` con `#[cfg(test)] mod tests` |

## 4. Checklist pre-dispatch (OBLIGATORIO, no saltar)

Antes de `gh issue edit --add-label jules`, ejecuta:

```bash
# 1. Islas disjuntas
python3 << 'PY'
islands = {"4.01": ["src/a.rs"], "4.02": ["src/b.rs"]}
for a in islands:
  for b in islands:
    if a < b and set(islands[a]) & set(islands[b]):
      raise SystemExit(f"CONFLICT {a} vs {b}")
print("✅ islands verified")
PY

# 2. Cada body tiene 11 secciones
for f in .hermes/olaN/body-*.md; do
  count=$(grep -c "^## " "$f")
  if [ "$count" -lt 11 ]; then echo "❌ $f solo $count secciones"; exit 1; fi
  echo "✅ $f $count secciones"
done

# 3. Releer bodies
for n in $(gh issue list --search "wave-N" --json number --jq '.[].number'); do
  gh issue view $n --json body --jq '.body' | head -30
done

# 4. Cargo check en main
CARGO_TARGET_DIR=target cargo check --all-targets 2>&1 | grep "^error" | wc -l  # 0
```

Solo si todo pasa → `for n in 1743 1744 ...; do gh issue edit $n --add-label jules; done`

## 5. Qué NO hacer (errores de WAVE-4.03)

- ❌ Título sin `feat-` (usé `[WAVE-4.03]` en vez de `[WAVE-4.03] feat-mesh-service-network — ...`)
- ❌ Labels solo `wave-4` sin `ola4` (el canónico pide ambos)
- ❌ PR Delivery sin `wc -l` y `grep -c "fn test_"` (permite PRs con archivos vacíos, NIDO #11)
- ❌ Verification sin `cargo clippy` y `cargo fmt` (permite PRs que rompen CI)
- ❌ Crear issues con `gh issue create --label wave-4,jules` (jules antes de verificar) → Jules toma issues incompletos

## 6. Dónde se guarda este harness

- `~/.hermes/skills/xavier-jules-wave/SKILL.md` (global, todas las sesiones)
- `~/proyectosSWAL/apps/xavier/.hermes/skills/xavier-jules-wave/SKILL.md` (proyecto, backup)
- Referenciado en `~/.hermes/SYSTEM_RULES.md` y `/etc/nixos/scripts/rules/SYSTEM_RULES.md` si existe

Futuros Hermes: **cargar este skill** al inicio de cualquier ola Xavier:

```bash
skill_view(name='xavier-jules-wave')
```

## 7. Redacción de waves DELEGADA a opencode CLI (VERIFIED 2026-09-02, WAVE-8/9/10 = 20 issues)

Para waves grandes (5-15 issues) redactar a mano es costoso. **Patrón verificado**: usar `opencode_delegate.py` (warm server) para generar los bodies con el template canónico, parsear la salida, validar secciones, crear con `gh`.

### 7.1 Procedimiento

1. **Define JSON spec** con file islands disjuntas verificadas (`/tmp/opencode-prompts/waveN-islands.json`):
   ```json
   {
     "WAVE-N.01": {"feat": "feat-kebab", "title": "...", "files": ["src/a.rs"], "effort": "M", "risk": "LOW", ...},
     "WAVE-N.02": {...}
   }
   ```

2. **Build prompt** (`/tmp/opencode-prompts/waveN-final.md`) = template canónico + JSON spec + context del repo + lista de paths REALES verificados con `find`/`ls`.

3. **Delegate con warm server** (11x más rápido que `opencode run` cold):
   ```bash
   # Server ya debe estar corriendo: opencode serve --port 4096 &
   python3 ~/.hermes/scripts/opencode_delegate.py \
     --prompt-file /tmp/opencode-prompts/waveN-final.md \
     --model opencode-go/deepseek-v4-flash \
     --title waveN-name --json > /tmp/opencode-out/waveN.json 2>&1
   ```

4. **Parsear salida** (ver §7.2 abajo para el pitfall crítico):
   ```python
   import re, json
   raw = open("/tmp/opencode-out/waveN.json").read()
   m = re.search(r'^\{', raw, re.MULTILINE)
   wrapper = json.loads(raw[m.start():m.start()+raw[m.start():].rfind("}")+1])
   text = wrapper["text"]
   ```

5. **Extraer bodies** con delimitadores `===ISSUE_<N>_<FEAT>===`:
   ```python
   pattern = re.compile(r'===ISSUE_(\d+)_([\w-]+)===', re.MULTILINE)
   # Si solo hay 2/5 delimitadores pero esperabas 5: ver §7.3 (rate limit recovery)
   ```

6. **Validar** que cada body tiene las secciones requeridas (buscar strings como "Current State", "Acceptance Criteria", "Verification", etc.).

7. **Verificar labels existen** ANTES de `gh issue create`:
   ```bash
   gh label list --repo iberi22/xavier --limit 200 --json name --jq '.[].name' > /tmp/labels.txt
   grep -qxF "wave-N" /tmp/labels.txt || gh label create "wave-N" --repo ... --color XXXXXX
   ```
   Si la label no existe, `gh issue create` falla con `could not add label: 'X' not found` y **NO crea la issue**.

8. **Crear con `gh issue create --body-file`** y delay 6s entre cada llamada (gh rate limit conservative).

9. **Apply label `jules`** solo después de verificar (file islands + secciones + bodies correctos):
   ```bash
   for n in 1807 1808 1809 ...; do gh issue edit $n --repo iberi22/xavier --add-label jules; sleep 2; done
   ```

### 7.2 PITFALL CRÍTICO: Wrapper JSON prefix (VERIFIED 2026-09-02)

`opencode_delegate.py --json` prepende una línea de log antes del JSON:
```
[opencode delegate] 211.1s | tokens in/out=6/10621 | session=ses_f9cafe44bffe
{"text": "...", "session": "ses_f9..."}
```

**Si haces `json.loads(open(...).read())` directamente → `json.decoder.JSONDecodeError`**.

**Fix obligatorio**:
```python
import re
raw = open("/tmp/opencode-out/waveN.json").read()
m = re.search(r'^\{', raw, re.MULTILINE)  # Busca el primer {
if not m: raise SystemExit("No JSON found in wrapper output")
json_text = raw[m.start():]
last = json_text.rfind("}")
json_text = json_text[:last+1]
wrapper = json.loads(json_text)
text = wrapper["text"]
```

### 7.3 Rate limit recovery — waves cortadas (VERIFIED 2026-09-02, WAVE-8)

Opencode con qwen3.8-flash puede **cortarse a mitad** (produjo 2/5 bodies en lugar de 5) cuando el modelo alcanza rate limits internos. **Síntoma**: el log contiene solo 2 delimitadores `===ISSUE_...===` cuando esperabas 5.

**Recovery**:
1. Detectar: `len(pattern.findall(text)) < expected_count`
2. Identificar cuáles faltan: `set(expected.keys()) - set(parsed.keys())`
3. Construir un prompt solo para los faltantes (más corto → menos rate pressure):
   ```python
   missing = {k: v for k, v in full_spec.items() if k not in parsed}
   short_prompt = template_core + json.dumps(missing)  # sin todo el spec completo
   ```
4. **Delay 15-30s** antes de relanzar (esperar a que se libere el rate window):
   ```bash
   sleep 30 && python3 ~/.hermes/scripts/opencode_delegate.py \
     --prompt-file /tmp/opencode-prompts/waveN-missing.md \
     --model opencode-go/deepseek-v4-flash ...
   ```
5. La 2da corrida termina más rápido (88s vs 200+ s) porque el rate window ya pasó.

### 7.4 Métricas observadas (Xavier, qwen3.8-flash, WAVE-8/9/10)

| Operación | Latencia |
|-----------|----------|
| `opencode run` cold | ~88s (provider warm-up) |
| `opencode serve` warm 1ra | ~7.6s |
| `opencode serve` warm 2da+ | ~2-5s |
| Wave de 5 issues (warm server) | ~200-330s |
| Wave de 3 issues missing (warm, post-delay) | ~88s |
| `gh issue create` + 6s delay × 5 | ~50s |
| `gh issue edit --add-label jules` × N | ~2s × N |

### 7.5 Output format esperado del modelo

El modelo emite el texto delimitado por `===ISSUE_<N>_<FEAT>===` y dentro tiene el markdown completo del body. **NO** debe emitir headers tipo `## ISSUE 1` ni prefijos tipo `Here are the 5 issues:`. Si lo hace, el parser falla — re-delega con instrucción más explícita ("OUTPUT ONLY THE 5 BODIES. NO PREAMBLE.").
