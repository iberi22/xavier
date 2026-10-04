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

- Crear cualquier issue para Jules en `iberi22/xavier` (waves 4-29). **Verificar el numero
  de wave actual antes de crear labels**: `gh label list --repo iberi22/xavier --limit 300 --json name --jq '.[].name' | grep -E '^wave-' | sort -V | tail -5`.
  La wave mas reciente al 2026-09-26 es **wave-29** (`feat-health-truth` et al, 10 issues, #2554-#2563).
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
- Tests: N existing, N passing in `cargo test --package xavier --lib --features ci-safe -- --test-threads=1`
- Handler: `src/adapters/inbound/http/handlers/...` (si aplica)

## Desired State (DELTA)

- **Section A** (lines XX-YY): Add struct `Foo` + fn `bar`
- **New file**: `src/path/to/new.rs` with `pub struct NewThing`
- **New test**: anade casos a `#[cfg(test)]` del propio archivo y corre `cargo test --package xavier --lib --features ci-safe <filter> -- --test-threads=1`
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

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l` == 0
- [ ] `grep -c "MyStruct\|my_fn" src/path/to/file.rs` >= 1
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` NO produce salida
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l` == 0
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
4. **Cargo check gate**: `cargo check --package xavier --all-targets --features ci-safe` debe dar 0 errores antes de commit
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
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l  # expect 0
cargo clippy --all-targets -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
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
| `dart analyze lib/...` | `cargo check --package xavier --all-targets --features ci-safe` + `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings` |
| `flutter test` | `cargo test --package xavier --lib --features ci-safe -- --test-threads=1` |
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
CARGO_TARGET_DIR=target cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l  # 0
```

Solo si todo pasa → `for n in 1743 1744 ...; do gh issue edit $n --add-label jules; done`

## 4bis. Lo que CI corre de verdad (verificado 2026-09-26 contra ci.yml)

El gate de `ci.yml` para cambios Rust es EXACTAMENTE esto, y nada mas:

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings
cargo check --package xavier --all-targets --features ci-safe   # ademas en MSRV 1.94
cargo test --package xavier --lib --features ci-safe -- --test-threads=1
bash scripts/check-secrets.sh
```

Consecuencias practicas para disenar issues:

1. **`--features ci-safe` es obligatorio.** Sin el flag el build intenta resolver deps locales
   y falla en CI aunque funcione en la maquina. Todo AC de `cargo` debe incluirlo.
2. **`--test-threads=1` es obligatorio.** Tests en `src/memory/sqlite_vec_store/schema_impl.rs`
   mutan ENV global (`XAVIER_EMBEDDING_PROVIDER_MODE`, `OPENAI_API_KEY`) y no se paralelizan
   entre procesos. Con 4 threads fallan 4 tests de forma determinista. Rational completo en el
   comentario de `ci.yml` lineas 145-153.
3. **`tests/` NO corre en CI.** El job `rust-test` usa `--lib` solamente. Un integration test
   bajo `tests/` no es gate de nada hasta que se agregue un job. Si un issue necesita E2E como
   gate, ese issue DEBE tocar `.github/workflows/ci.yml` — y como los workflows son el archivo
   mas propenso a conflictos en waves paralelas, ese issue va SIEMPRE de ultimo en el merge order.
4. **No existe job de `tests/` ni de `code-graph`.** `cargo test --package code-graph` no lo
   corre nadie; si un issue toca esa crate, su AC debe decirlo explicitamente.

## 4ter. Tamano del body vs tamano del prompt (patron verificado en WAVE-29)

El body canonico de 13 secciones pesa ~10-13 KB. Es el **registro** y debe guardarse completo
en el issue. Pero el `prompt` que se manda por REST en el `POST /sessions` no necesita ser el
body entero.

Patron usado en WAVE-29 (10 issues, 0 conflictos, 10/10 `IN_PROGRESS` al minuto):

- Issues creados con el body completo via `gh issue create --body-file`.
- El prompt se **genera por script** re-extraendo del body: Current State (evidencia medida),
  Desired State, Files to Modify, DO NOT touch, Acceptance Criteria, Anti-Hallucination Guard,
  PR Delivery — mas un bloque comun con los comandos de CI y las reglas de `AGENTS.md`.
- Beneficio: cero drift entre issue y prompt, cero duplicacion de texto, y el prompt queda
  ~10 KB en vez de 13 KB.
- Siempre comentar el `session id` + URL en el issue de origen. Jules auto-cierra el issue
  cuando completa, y sin ese comentario no hay forma de mapear issue -> sesion -> PR.

## 5. Qué NO hacer (errores de WAVE-4.03)

- ❌ Título sin `feat-` (usé `[WAVE-4.03]` en vez de `[WAVE-4.03] feat-mesh-service-network — ...`)
- ❌ Labels solo `wave-4` sin `ola4` (el canónico pide ambos)
- ❌ PR Delivery sin `wc -l` y `grep -c "fn test_"` (permite PRs con archivos vacíos, NIDO #11)
- ❌ Verification sin `cargo clippy` y `cargo fmt` (permite PRs que rompen CI)
- ❌ Crear issues con `gh issue create --label wave-4,jules` (jules antes de verificar) → Jules toma issues incompletos

## 5-bis. Pre-flight de dispatch y verificación de PRs (lecciones 2026-09-15)

### Antes de despachar: los archivos citados DEBEN existir en `origin/main`

Un issue que en `## Current State` cita archivos que viven **solo en una rama local sin subir**
produce desastres silenciosos: Jules clona `main`, no encuentra el módulo, y (a) **recrea el archivo**
(colisiona con la rama) o (b) **falla la sesión**. Ocurrió con 4 issues de clearance: 2 PRs prematuros
y 1 sesión `Failed`.

```bash
# ANTES de gh issue create, por cada archivo citado en el body:
git ls-tree origin/main -- src/security/clearance_audit.rs | wc -l   # 0 => NO está en main
# Si devuelve 0: subir/mergear esa rama PRIMERO, y solo entonces despachar el issue.
```

### Antes de blamear a un PR por tests rojos: dos descartes obligatorios

1. **Archivos sin commitear del árbol local** contaminan las corridas (un harness untracked que
   afirmaba el modelo viejo hizo ver 3 fallos en un PR correcto). Moverlo fuera y re-correr:
   ```bash
   mv test/ViejoHarness.t.sol /tmp/ && forge test | tail -3   # ¿siguen los fallos?
   ```
2. **Comparar con `origin/main` limpio**: si los mismos tests fallan ahí, son **preexistentes** y no
   del PR. Se arreglan mergeando el PR que los arregla *antes* de rebasar el propio.

```bash
git checkout -q origin/main && cargo test --lib <filtro>   # baseline
git checkout -q <mi-rama>  && cargo test --lib <filtro>   # delta
```

### Los PRs de Jules llegan en draft y sin CI de tests

`gh pr view <n> --json isDraft` → `true`; hay que `gh pr ready <n>` antes de mergear. En repos sin
CI de tests (p. ej. swal-econ) los únicos checks son CodeRabbit (saltado en draft) y Socket: **la
verificación real es local y la hace el orquestador** (`forge test` / `cargo test` en la rama del PR,
citando el `Ran N test suites ... X passed, 0 failed` en el body del merge).

### Ramas con commits de base ajenos

Si la rama local arrastra commits base que ya tienen PR propio, **rebasar antes de subir**
(`git rebase --onto origin/main <commit-base>`) para no duplicar ese trabajo en el PR:

```bash
git rev-list --count origin/main..<rama>      # tamaño real del PR
git log --oneline --reverse origin/main..<rama> | head  # ¿el primero es mío o es base?
```

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

## ⚠️ Field report 2026-09-29 — wave-30: 13/15 Jules sessions FAILED
- **Cause:** the Jules sandbox cannot build xavier (280k-line crate) within its time limit — `cargo check` alone took 5m48s in a probe, and the canonical ACs made Jules run check + clippy + the full test suite; sessions died with the generic "Jules encountered an error". One agent said it literally: "The `cargo check` command timed out in the sandbox".
- **Rule (xavier on Jules):** put this in the wave's `jules_rules` (the runner appends it to every prompt): *do NOT run cargo check/clippy/test in the sandbox; verify with the AC greps and by reading code; open the PR immediately — GitHub CI runs build, clippy and tests*. Keep the cargo ACs in the issue body for CI/local verification, not for Jules.
- **Visibility:** the wave runner blocks while it runs a local loop (subprocess.run), so it stops polling Jules for up to ~1 h. Always run `~/.hermes/scripts/jules-watch.py` (independent poller, every 2 min) — it writes `JULES-FAILED` / `JULES-AWAITING` / `JULES-COMPLETED` lines (with the agent's last message) to `~/.cache/swal/runner/inbox.md`; the orchestrator's monitor must include those patterns.
- **AWAITING_USER_FEEDBACK:** answer via `POST /v1alpha/sessions/{id}:sendMessage` with a concrete decision + the no-build rule; the runner's in-flight timer counts from dispatch, so reset `dispatched_at` when you answer or it reassigns the session locally on restart.
