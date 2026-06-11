# Implementation Plan — Xavier v1.0

**Version:** 0.6.1-beta → 1.0.0  
**Repo:** `E:\scripts-python\xavier` | **HEAD:** `8ed300c`  
**Actualizado:** Junio 11, 2026  
**Código actual:** 422 archivos .rs, ~88,500 líneas de Rust  

---

## Resumen

Xavier está en **~62% del camino a v1.0**. Hay 12 features mapeados:

| Estado | Features |
|--------|----------|
| ✅ Stable (95-100%) | Session Management |
| 🟡 Beta (70-90%) | Unified Storage, Hybrid Search, Belief Graph, MCP Server, Mesh Network |
| 🟠 Draft (25-60%) | Encryption, Code Graph, Telegram Bot, Notification System |
| 🔴 Planned (<30%) | Documentation Site, SRC Reference |

---

## Fase 1: Quick Wins (Semana 1) — Features gap < 40%

> **Objetivo:** Cerrar issues abiertos pequeños + documentación faltante

### 1.1 🟢 Issue #7 — Decidir web/ vs panel-ui (1 hora)
- **Qué:** Evaluar `web/` (dashboard React 18) y `panel-ui/` (Tauri + React)
- **Acción:** Recomendar integrar web → panel-ui como page adicional
- **Output:** Issue comment con decisión + update en features.json

### 1.2 🟢 SRC Reference Docs — Rellenar .gitcore/SRC.md (4 horas)
- **Qué:** Los 422 archivos necesitan documentación en SRC.md
- **Acción:** Script que escanea todos los módulos y genera estructura
- **Output:** `.gitcore/SRC.md` y `SRC_CONFIG.md` con contenido real

### 1.3 🟢 Fix OpenAI API Key (30 min)
- **Qué:** Key `sk-0VU4a...PKKI` es inválida (401)
- **Acción:** Generar nueva key o compilar gllm con módulo local
- **Output:** Vector search funcionando

---

## Fase 2: Issues Abiertos (Semana 1-2) — #3, #4, #5

### 2.1 🔴 Issue #5 — Token Management API (3-4 días)
**Estado actual:** 10% (solo CLI)
**Target:** 90%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. DB schema `api_tokens` table | 2h | SQLite con token_hash, scopes, expiry, created_at, revoked_at |
| 2. POST /security/tokens | 4h | Create token, hash con bcrypt, devolver one-time plaintext |
| 3. GET /security/tokens | 2h | Listar — solo partial hash, nunca full value |
| 4. DELETE /security/tokens/:id | 2h | Revoke — soft delete |
| 5. POST /security/tokens/:id/rotate | 2h | Revoke + create new |
| 6. Scope validation middleware | 4h | Validar scopes en cada request |
| 7. Tauri commands | 2h | create_api_token, revoke, list |
| **Total** | **18h** | |

### 2.2 🔴 Issue #4 — Notification System (4-5 días)
**Estado actual:** 25%
**Target:** 85%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. DB schema `notifications` table | 2h | event, source, level, timestamp, metadata_json |
| 2. Event bus interno | 4h | Enum NotificationEvent, event emitter |
| 3. GET /notifications | 3h | Paginado, filtros por categoría |
| 4. PATCH /notifications/:id/read | 1h | Mark as read |
| 5. DELETE /notifications/all | 1h | Clear all |
| 6. Integrar eventos reales | 4h | memory_indexed, agent_task_complete, system_start, error, warning |
| 7. Tauri emit_all | 4h | Real-time badge count en TopStatusBar |
| 8. 4 category islands | 2h | System, Memory, Agents, Errors |
| **Total** | **21h** | |

### 2.3 🟡 Issue #3 — Discord Webhook (2 días)
**Estado actual:** 0%
**Target:** 80%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. src/messaging/discord.rs | 4h | Webhook sender con embeds |
| 2. src/messaging/mod.rs | 1h | Module registration |
| 3. Encrypted token storage | 2h | AES-256 + keyring |
| 4. Rate limiting (30/min) | 2h | governor-based |
| 5. Integration test | 3h | Mock webhook server |
| **Total** | **12h** | |

---

## Fase 3: Features Core (Semana 3-4)

### 3.1 🟡 Hybrid Search — Fix embeddings (2 días)
**Estado actual:** 70%
**Target:** 95%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. Validar gllm compilation | 4h | local-gllm feature con candle-core 0.9.1 |
| 2. Embedding cache con moka | 4h | TTL + LRU cache para vectores |
| 3. RRF weights runtime config | 2h | Config endpoint POST /search/config |
| 4. Benchmark suite | 6h | Latencia objetivo <500ms |
| **Total** | **16h** | |

### 3.2 🟡 Code Graph — Multi-language (3 días)
**Estado actual:** 40%
**Target:** 80%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. tree-sitter grammars para 5 lenguajes | 8h | Rust, Python, JS, TS, Go |
| 2. ./code/find endpoint mejorado | 4h | Full-text + AST combined search |
| 3. Symbol cross-reference | 4h | Referencias entre archivos |
| 4. Integration tests | 4h | |
| **Total** | **20h** | |

### 3.3 🟡 Encryption — Wire end-to-end (2 días)
**Estado actual:** 60%
**Target:** 90%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. Integrar crypto en storage layer | 6h | Transparent encrypt/decrypt en write/read |
| 2. Key management UI | 4h | Key rotation, backup |
| 3. Performance benchmark | 2h | Medir overhead |
| **Total** | **12h** | |

---

## Fase 4: Enterprice & Polish (Semana 5-6)

### 4.1 🟡 Mesh Network Phase 2 (4 días)
**Estado actual:** 80%
**Target:** 95%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. Conflict resolution | 8h | CRDT-based sync |
| 2. Multi-hop routing | 6h | DHT-based node discovery |
| 3. Auto-update mechanism | 6h | Git-based OTA |
| **Total** | **20h** | |

### 4.2 🟢 Documentation Site (3 días)
**Estado actual:** 30%
**Target:** 90%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. Completar contenido docs/site/ | 8h | Quickstart, API ref, architecture |
| 2. CI/CD deployment pipeline | 4h | GitHub Actions → GitHub Pages |
| 3. Custom domain setup | 2h | docs.xavier.ai |
| **Total** | **14h** | |

### 4.3 🟢 Telegram Bot — Full Implementation (2 días)
**Estado actual:** 35%
**Target:** 85%

| Paso | Tiempo | Detalle |
|------|--------|---------|
| 1. Full command handling | 6h | /memory search, /help, /stats |
| 2. Webhook mode | 3h | ngrok/cloudflare tunnel |
| 3. Bot token encrypted storage | 3h | keyring + AES |
| **Total** | **12h** | |

---

## Timeline Resumen

| Fase | Semana | Features | Horas | Issues cerrados |
|------|--------|----------|-------|-----------------|
| 1️⃣ Quick Wins | 1 | SRC Docs, web/ decision, API key | 5.5h | #7 |
| 2️⃣ Issues Abiertos | 1-2 | Token API, Notifications, Discord | 51h | #3, #4, #5 |
| 3️⃣ Features Core | 3-4 | Hybrid Search, Code Graph, Encryption | 48h | — |
| 4️⃣ Enterprise+Polish | 5-6 | Mesh P2P v2, Docs Site, Telegram | 46h | #10, #11 |

**Total estimado:** ~150 horas de trabajo efectivo  
**ETA a v1.0:** 6 semanas  

---

## Dependencias y Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| gllm + candle-core compat | Alta | 🔴 Bloquea embeddings | Verificar versión pinned (0.9.1) |
| Tauri emit_all complejo | Media | 🟡 Notifications delay | Prototype primero |
| tree-sitter grammars | Baja | 🟢 Code Graph delay | Empezar con Rust + Python |
| Mesh CRDT complexity | Media | 🟡 Phase 2 delay | Usar lib existing (automerge?) |
| API key renewal | Alta | 🟡 Vector search | Múltiples providers (OpenAI, local) |

---

## Feature Progress Dashboard

```
Features                      ████████████████░░░░ 62%
├── Session Management        ████████████████████ 95% ✅
├── MCP Server                ██████████████████░░ 90% ✅
├── Unified Storage           █████████████████░░░ 85%
├── Mesh Network              ████████████████░░░░ 80%
├── Belief Graph              ███████████████░░░░░ 75%
├── Hybrid Search             ██████████████░░░░░░ 70%
├── Encryption at Rest        ████████████░░░░░░░░ 60%
├── Code Graph Index          ████████░░░░░░░░░░░░ 40%
├── Telegram Bot              ███████░░░░░░░░░░░░░ 35%
├── Documentation Site        ██████░░░░░░░░░░░░░░ 30%
├── Notification System       █████░░░░░░░░░░░░░░░ 25%
└── SRC Reference             ████░░░░░░░░░░░░░░░░ 20%
```
