---
title: "Universal LLM Provider + CLI Agent Dashboard + Onboarding v2"
description: "Chat del panel responde usando un LLM local (Ollama) con fallback a cloud y degradación elegante a memoria cuando ningún provider responde."
---

## Context
Xavier debe detectar, monitorear y permitir cambiar entre todos los proveedores de LLM disponibles: locales + CLI agents + cloud. El sistema debe escanear el entorno al inicio, verificar login/credenciales, medir calidad de uso, y mostrar un dashboard en la UI (React/Tauri).

## Estado actual (2026-06-08)
- ✅ Provider system: Anthropic, OpenAI, Groq, DeepSeek, Gemini, MiniMax, Local (Ollama)
- ✅ Rate limiting: token bucket, per-provider quotas, cache, cooldown
- ✅ Usage tracking: dashboard con métricas diarias/semanales/mensuales
- ✅ Proxy de API keys con PQC (KEK/DEK + HardwareVault)
- ✅ Tier system: Free/Cloud/Pro/Enterprise
- ✅ Security layers: prompt guard, threat detection, homoglyph, URL scanner, canary
- ✅ Onboarding básico: detecta OS, Docker, WSL, rustc, node, python, git, ollama
- ✅ React Tauri UI con Radix UI (`panel-ui/`)
- ⚠️ CLI agents no detectados (opencode, codex, claude, copilot)
- ❌ Login status de CLI agents no verificado
- ❌ Límites de uso reales por API tier no trackeados
- ❌ Calidad comparativa entre proveedores
- ❌ Notificaciones para cambio de proveedor

## Hardware detectado
- GPU: AMD Radeon RX 6600 (8GB VRAM, reportado como 8147MB por dxdiag)
- RAM: 32GB
- Models Ollama ya descargados: gemma-4-E2B-it-uncensored Q4_K_M (3.2GB, 2B params), nomic-embed-text (255MB), + otros ~5GB en blobs
- CLI agents instalados:
  - opencode@1.3.6 (npm)
  - codex@0.130.0 (npm, de OpenAI)
  - claude@2.1.91 (standalone exe, Claude Code)
  - copilot@0.0.365 (npm, GitHub Copilot CLI)
  - agy → no encontrado (posiblemente no instalado)

## CLI Detection Matrix (investigación)
| CLI | Comando login status | Comando límites | Config path |
|-----|---------------------|-----------------|-------------|
| codex | `codex login status` → "Logged in using ChatGPT" | N/A (por token) | `~/.codex/` |
| opencode | `opencode whoami` / ver token env | N/A | package.json |
| claude | check `~/.claude/.credentials.json` | N/A | `~/.claude/settings.json` |
| copilot | check `~/.copilot/config.json` | N/A | `~/.copilot/` |

## Env vars detectadas
```sh
OPENAI_API_KEY, OPENAI_BASE_URL
GROQ_API_KEY, GEMINI_API_KEY (x4), MINIMAX_API_KEY
FIRECRAWL_API_KEY, OPENROUTER_API_KEY
CLOUDCODE_MODEL, CLOUDCODE_PROVIDER (bedrock)
OPENCLAW_SERVICE_MANAGED_ENV_KEYS
xavier_TOKEN, XAVIER2_TOKEN
```

---

# FASE 1: System Detection Engine (Backend)

## ISSUE 1.1: SystemScanner — CLI Agent, Model & Service Discovery
**Type**: feature
**Labels**: backend, detection, system
**Priority**: P0
**Estimate**: 3-5 points

### Description
Crear un módulo `src/system_scanner/` que escanee el sistema al inicio de Xavier y detecte:
1. **CLI agents instalados** (`open`, `codex`, `claude`, `copilot`, `agy`)
2. **Ollama** y modelos descargados (via API REST: `GET /api/tags`)
3. **GPU** (modelo, VRAM, driver)
4. **Env vars** de API keys (OPENAI, ANTHROPIC, GEMINI, GROQ, MINIMAX, etc.)
5. **Config files** de cada CLI agent (`~/.claude/`, `~/.copilot/`, `~/.codex/`)
6. **Login status** de cada CLI agent (token válido vs no)
7. **Xavier API** propia (HTTP up/down)

### Implementation
```rust
// src/system_scanner/mod.rs
pub mod cli_agents;   // detecta CLI agents
pub mod ollama;       // detecta Ollama
pub mod gpu;          // detecta GPU
pub mod env_keys;     // detecta env vars
pub mod config_files; // detecta config files

// src/system_scanner/cli_agents.rs
pub struct CliAgentInfo {
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub logged_in: bool,
    pub login_method: Option<String>,
    pub config_path: Option<String>,
}

pub fn detect_all_cli_agents() -> Vec<CliAgentInfo>
```

### Detection rules
```
opencode: check npm global → opencode --version → 1.3.6
          login: check env OPENAI_API_KEY or OPENAI_BASE_URL

codex:    check npm global → codex --version → 0.130.0
          login: codex login status → parse "Logged in using X"

claude:   check ~/.local/bin/claude.exe → --version → 2.1.91
          login: parse ~/.claude/.credentials.json / settings.json
          detect OAuth vs API key

copilot:  check npm global → copilot --version → 0.0.365
          login: parse ~/.copilot/config.json
```

### Acceptance criteria
- [ ] `scanner scan` CLI command que imprime todo lo detectado
- [ ] `XavierSystem` struct se construye al inicio y persiste
- [ ] Ollama detecta modelos vía API REST en puerto 11434
- [ ] GPU detection reporta nombre, VRAM total/libre, driver
- [ ] Login status reporta "✅ Logged in" o "❌ Needs auth"

---

## ISSUE 1.2: TokenQuotaTracker — Límites de API por proveedor
**Type**: feature
**Labels**: backend, proxy, tokens
**Priority**: P0
**Estimate**: 5 points

### Description
Extender el sistema de rate limiting existente para trackear límites de API reales por proveedor. Cada proveedor cloud tiene tiers (free, paid) con límites distintos. Xavier debe:
1. **Consultar límites** vía headers de respuesta (X-RateLimit-Remaining, etc.)
2. **Tier detection** automático (free vs pro vs enterprise) basado en límites observados
3. **Notificar** cuándo se acerca al límite del tier actual
4. **Sugerir upgrade** si se superan consistentemente los límites

### Data model
```rust
pub struct ProviderQuota {
    pub provider: ProviderKind,
    pub tier: ApiTier,              // Free | Pro | Enterprise | Unknown
    pub requests_remaining: Option<u32>,
    pub tokens_remaining: Option<u32>,
    pub requests_limit: Option<u32>,
    pub tokens_limit: Option<u64>,
    pub resets_at: Option<DateTime>,
    pub current_billing_period: Option<BillingPeriod>,
}

pub enum ApiTier {
    Free,
    Pro,
    Enterprise,
    Unknown,
}
```

### Implementation
- Modificar el proxy de API keys (`src/domain/proxy/`) para interceptar rate limit headers
- Almacenar en `src/memory/` las métricas de límite por proveedor
- Crear comando `xavier quota` que imprime límites actuales

### Acceptance criteria
- [ ] Intercepta headers de rate limit de OpenAI, Anthropic, Groq, Gemini
- [ ] Detecta tier automáticamente basado en límites
- [ ] Almacena límites en memoria persistente
- [ ] Comando `xavier quota` muestra tier + tokens restantes + reset time

---

## ISSUE 1.3: ProviderSwitch — Sistema de cambio de proveedor en caliente
**Type**: feature
**Labels**: backend, routing
**Priority**: P1
**Estimate**: 3 points

### Description
Crear un sistema que permita cambiar el proveedor activo de LLM en **tiempo real** sin reiniciar Xavier. Los cambios se propagan a todos los handlers que dependen del LLM.

```rust
pub enum ActiveProvider {
    Auto(OllamaModel),         // Inteligente: elige el mejor basado en contexto
    Manual(ProviderKind),      // Usuario elige explícitamente
    Fallback(Vec<ProviderKind>), // Si falla el primario, usa el siguiente
}

pub struct ProviderRouter {
    active: ActiveProvider,
    history: Vec<ProviderSwitchEvent>,
}

pub fn switch_provider(new: ProviderKind) -> Result<()>
pub fn set_auto_strategy(strategy: AutoStrategy)
```

### Acceptance criteria
- [ ] Comando `xavier provider set <name>` cambia en caliente
- [ ] Comando `xavier provider status` muestra proveedor activo
- [ ] Historial de cambios se guarda en memoria
- [ ] Proving automático si un provider falla (fallback chain)

---

# FASE 2: Onboarding v2 (Backend + CLI)

## ISSUE 2.1: OnboardingInteractivov2 — Detección avanzada + recomendaciones
**Type**: feature
**Labels**: backend, onboarding, UX
**Priority**: P0
**Estimate**: 5 points

### Description
Reemplazar el onboarding actual por uno v2 que:
1. Escanea el sistema (usando SystemScanner de Issue 1.1)
2. Presenta un resumen interactivo:
   - "Encontré 4 CLI agents: opencode, codex, claude, copilot"
   - "Encontré Ollama con 3 modelos: gemma-4-e2b, nomic-embed-text, ..."
   - "Encontré 6 API keys configuradas"
3. Pregunta: "¿Quieres habilitar el modo headless para que CLI agents externos puedan consultar Xavier?"
4. Configura auto-detección de mejor proveedor según contexto
5. Guarda la configuración en `~/.xavier/provider-config.yaml`

### Integration with existing onboarding
```rust
// Extender src/cli/onboarding.rs
pub enum AgentRole {
    System(std::path::PathBuf),
    CliChild(CliAgentInfo),
    Headless,
}

pub fn generate_provider_recommendations(scanner: &SystemScan) -> Vec<String>
```

### Acceptance criteria
- [ ] `xavier setup` (o `xavier config init`) ejecuta el nuevo onboarding
- [ ] Detecta y recomienda CLI agents disponibles
- [ ] Pregunta por modo headless
- [ ] Guarda configuración de proveedores
- [ ] No molesta en ejecuciones subsecuentes (solo si `--force` o cambios detectados)

---

# FASE 3: LLM Quality Dashboard (Backend + API)

## ISSUE 3.1: QualityTracker — Medición de calidad por proveedor
**Type**: feature
**Labels**: backend, metrics
**Priority**: P1
**Estimate**: 5 points

### Description
Sistema de medición de calidad que trackea por cada proveedor:
- Tokens por segundo (latencia)
- Tasa de éxito vs error
- Tiempo hasta primer token (TTFT)
- Uso de tokens totales por día/semana/mes
- Costo estimado (basado en tier)
- Prompt satisfaction score (feedback implícito: ¿el usuario aceptó la respuesta?)

```rust
pub struct ProviderMetrics {
    pub provider: ProviderKind,
    pub period: TimePeriod,
    pub total_requests: u64,
    pub total_tokens: TokenCount,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_latency_ms: f64,
    pub avg_ttft_ms: f64,
    pub estimated_cost_usd: f64,
    pub user_satisfaction: Option<f64>,  // 0.0-1.0
}

pub fn track_llm_call(start: Instant, provider: ProviderKind, tokens: TokenCount, result: &Result)
pub fn get_metrics(provider: Option<ProviderKind>, period: TimePeriod) -> ProviderMetrics
```

### Acceptance criteria
- [ ] Trackea cada llamada LLM con su proveedor
- [ ] Expone endpoint API REST `/v1/metrics/providers`
- [ ] Almacena históricos con TTL (30 días rolling window)
- [ ] Calcula costo estimado basado en tier del proveedor

---

# FASE 4: Frontend — Panel de Configuración + Dashboard (React/Tauri)

## ISSUE 4.1: ProviderConfigPage — UI de configuración de proveedores
**Type**: feature
**Labels**: frontend, react, panel-ui
**Priority**: P0
**Estimate**: 8 points

### Description
Crear página de configuración de proveedores en `panel-ui/src-tauri/` con Radix UI.

**Mock layout:**
```
┌──────────────────────────────────────────────┐
│  ⚙️ Provider Settings                         │
├──────────────────────────────────────────────┤
│  ┌─ Active Provider ───────────────────────┐ │
│  │  [Auto ▼] Ollama (local) ⋮ Status: 🟢  │ │
│  │  Fallback: [Anthropic ▼] if local fails │ │
│  └──────────────────────────────────────────┘ │
│                                              │
│  ┌─ CLI Agents Detected ──────────────────┐ │
│  │  ☑️ opencode 1.3.6  🟢 Logged in     │ │
│  │  ☑️ codex 0.130.0   🟢 Logged in     │ │
│  │  ☑️ claude 2.1.91   🟢 Logged in     │ │
│  │  ☐ copilot 0.0.365 🟡 Not logged in │ │
│  │  [Enable Headless: ☑️]                │ │
│  └──────────────────────────────────────────┘ │
│                                              │
│  ┌─ Cloud Providers ──────────────────────┐ │
│  │  Provider      │ Tier     │ Quota      │ │
│  │  ────────────────────────────────────  │ │
│  │  OpenAI        │ Free 🟡  │ 3.5K/5K req│ │
│  │  Anthropic     │ Pro 🟢   │ 42K/∞ tok │ │
│  │  Gemini        │ Free 🟡  │ 58/60 rpm  │ │
│  │  Groq          │ Free 🟡  │ 28/30 rpm  │ │
│  │  OpenRouter    │ Unknown  │ —          │ │
│  └──────────────────────────────────────────┘ │
│                                              │
│  ┌─ API Keys ─────────────────────────────┐ │
│  │  OPENAI:        ●●●●●●●●●●     [Edit]  │ │
│  │  GEMINI:        ●●●●●●●●●●     [Edit]  │ │
│  │  [Add API Key...]                       │ │
│  └──────────────────────────────────────────┘ │
├──────────────────────────────────────────────┤
│  [Save]  [Reset to Defaults]                  │
└──────────────────────────────────────────────┘
```

**Components needed:**
- `ProviderSelector` — dropdown con proveedores + status indicator
- `CliAgentList` — lista de CLI agents con toggle
- `ProviderQuotaTable` — tabla de límites de API
- `ApiKeyInput` — input seguro con máscara
- `HeadlessToggle` — switch para modo headless

### Integration points
- Backend API endpoints: `GET/PUT /v1/config/providers`
- Backend API endpoints: `GET /v1/system/scan`
- Backend API endpoints: `GET /v1/metrics/providers`

### Acceptance criteria
- [ ] Página de settings funcional en Tauri app
- [ ] Muestra proveedor activo + status
- [ ] Lista CLI agents detectados con login status
- [ ] Muestra quotas de API cloud
- [ ] Permite agregar/editar API keys
- [ ] Toggle headless mode
- [ ] Auto-refresca cada 30 segundos

---

## ISSUE 4.2: DashboardPage — Vista de calidad de uso + comparativa
**Type**: feature
**Labels**: frontend, react, panel-ui
**Priority**: P1
**Estimate**: 5 points

### Description
Tablero de calidad comparativa entre proveedores. Muestra gráficos de:
- Latencia promedio por proveedor (última hora/día/semana)
- Tasa de éxito/error
- Tokens/día por proveedor
- Costo estimado
- Tiempo hasta primer token

**Vista:**
```
┌──────────────────────────────────────────────┐
│  📊 LLM Performance Dashboard                │
│  [Hour ▼] [All Providers ▼]                  │
├──────────────────────────────────────────────┤
│  ┌─ Latency Comparison ─────────────────┐    │
│  │  ████████████ Ollama (25ms)          │    │
│  │  ██████████████ Anthropic (180ms)    │    │
│  │  ████████████████████ Groq (320ms)   │    │
│  │  ██████████████████████████ GPT-4o   │    │
│  └──────────────────────────────────────┘    │
│  ┌─ Usage This Week ────────────────────┐    │
│  │  📈 Tokens/day chart                  │    │
│  │  💰 $3.42 estimated cost             │    │
│  └──────────────────────────────────────┘    │
│  ┌─ Provider Recommendation ────────────┐    │
│  │  🟢 Ollama (local) for short queries │    │
│  │  🟡 Anthropic for complex analysis   │    │
│  └──────────────────────────────────────┘    │
├──────────────────────────────────────────────┤
│  [Switch to: Ollama ▼]                        │
└──────────────────────────────────────────────┘
```

### Components
- `LatencyChart` — barras comparativas de latencia
- `UsageChart` — gráfico de tokens/día
- `CostSummary` — costo estimado
- `ProviderSwitch` — dropdown para cambiar proveedor desde dashboard

---

## ISSUE 4.3: NotificationSystem — Notificaciones de cambio de proveedor
**Type**: feature
**Labels**: frontend, react, notifications
**Priority**: P1
**Estimate**: 3 points

### Description
Sistema de notificaciones que informa al usuario:
- "Tu tier gratuito de OpenAI está cerca del límite (3.2K/5K requests)"
- "Ollama local tiene latencia 10x menor que Anthropic — ¿cambiar?"
- "Nuevo modelo detectado en Ollama: gemma-4-9b"
- "CLI agent codex ya no está logueado"

**Types:**
```rust
pub enum NotificationKind {
    ProviderLimitNear(ProviderKind, f64),      // % del límite alcanzado
    ProviderLimitReached(ProviderKind),
    BetterProviderAvailable(ProviderKind, f64), // mejora de latencia/costo
    NewModelDetected(String),
    CliAgentLoggedOut(String),
    CliAgentDetected(String),
}
```

### Implementation
- Backend: event system que emite `NotificationEvent`
- Frontend: toast/notification component con Radix UI
- Configurable: "No volver a mostrar" persistente

---

# FASE 5: Headless Mode — CLI Agent Bridge

## ISSUE 5.1: HeadlessServer — API TCP para agentes externos
**Type**: feature
**Labels**: backend, api, integration
**Priority**: P1
**Estimate**: 8 points

### Description
Cuando el headless mode está habilitado, Xavier expone una API TCP/HTTP en un puerto configurable (ej: 8007) donde otros CLI agents pueden consultar:

1. **Contexto de memoria**: `GET /context?query=...` → devuelve QMD search results
2. **Tool list**: `GET /tools` → qué herramientas tiene Xavier
3. **Ejecutar tool**: `POST /tools/:name` → ejecuta tool de Xavier
4. **Memory search**: `POST /memory/search` → busca en QMD
5. **Provider info**: `GET /provider/status` → qué proveedor activo y status

**Endpoints:**
```rust
// TCP API
pub enum HeadlessRequest {
    ContextQuery { query: String, limit: usize },
    ExecuteTool { name: String, args: Vec<String> },
    MemorySearch { text: String, filters: Option<MemoryFilters> },
    ProviderStatus,
}

pub enum HeadlessResponse {
    Context(Vec<MemoryItem>),
    ToolResult(String),
    MemorySearch(Vec<MemoryItem>),
    Provider(ProviderStatus),
}
```

### Security
- Auth via `xavier_TOKEN` env var or config file
- Solo procesos locales (bind to 127.0.0.1)
- Rate limiting específico para modo headless
- Logging de todas las llamadas headless

### Acceptance criteria
- [ ] Servidor TCP en puerto configurable (default 8007)
- [ ] Autenticación con token
- [ ] Endpoint de contexto de memoria funcional
- [ ] Endpoint de provider status funcional
- [ ] CLI agents pueden consultar: `curl http://localhost:8007/context?query=... -H "Authorization: Bearer $XAVIER_TOKEN"`
- [ ] Documentación de endpoints en README

---

# FASE 6: Fine-tuning Pipeline (Google Colab + Local)

## ISSUE 6.1: ColabFT — Google Colab fine-tuning script
**Type**: feature
**Labels**: ml, colab, fine-tuning
**Priority**: P2
**Estimate**: 5 points

### Description
Crear notebook de Google Colab que:
1. Carga Gemma 4 E2B (base, desde HuggingFace)
2. Aplica QLoRA (Unsloth)
3. Usa datos de Xavier (conversaciones + consultas QMD)
4. Exporta a GGUF y comprime
5. Ofrece descarga directa o upload a HuggingFace

**Script location**: `scripts/colab/xavier-fine-tune.ipynb`

---

# ISSUE IMPLEMENTATION ORDER

```
1.1  SystemScanner (Backend)        ─ P0 ─→  2.1  Onboarding v2
 ↓                                            ↓
1.2  TokenQuotaTracker              1.3  ProviderSwitch
 ↓                                            ↓
3.1  QualityTracker (Backend)        Phases 1-3 = Backend Core
 ↓
4.1  ProviderConfigPage (UI)
4.2  DashboardPage (UI)              4.3  NotificationSystem
 ↓
5.1  HeadlessServer
 ↓
6.1  Colab Fine-tuning
```

## Dependencies
- Issues 1.1, 1.2, 2.1 deben completarse antes que 4.1
- Issue 3.1 debe completarse antes que 4.2
- Issue 5.1 es independiente (puede ir en paralelo con 4.x)
- Issue 6.1 es el último paso

### Functional Local Configuration Example
Configure Xavier to function in a fully localized mode using Ollama or other offline engines:

**Environment Setup (`.env`):**
```sh
XAVIER_LOCAL_LLM_URL="http://127.0.0.1:11434"
XAVIER_EMBEDDING_MODEL="nomic-embed-text"
XAVIER_CHAT_MODEL="gemma-4-E2B-it-uncensored"
```

Verify that the local engine status has been detected as "running":
```bash
curl http://localhost:8006/v1/offline-models/status
```
