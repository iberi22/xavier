---
title: "API Key Proxy Vault (Ephemeral Secret Lease System)"
description: "AES-256-GCM encryption for stored memory data with Argon2 key derivation"
---

**Status:** `in_progress` | **Score:** 65% | **Última actualización:** 2026-06-29

## Visión General

Xavier actúa como **proxy de confianza** para API keys de terceros (LLM providers, servicios externos). Las claves se almacenan cifradas en el sistema, y cuando un agente necesita una, Xavier le presta una **API key efímera con TTL** que se revoca automáticamente al finalizar la tarea o expirar su tiempo de vida. Esto previene:

- **Robo/exfiltración:** Si un agente es comprometido, su lease expira automáticamente
- **Uso no autorizado:** Las keys nunca se exponen directamente; van inyectadas en el proxy HTTP
- **Auditoría total:** Cada lend, revoke y uso queda registrado

## Arquitectura

```
                    ┌─────────────────────┐
                    │   HardwareVault      │ (keyring/DPAPI)
                    │   (cifrado por SO)   │
                    └──────┬──────────────┘
                           │
                    ┌──────▼──────────────┐
                    │  KeyLendingEngine   │ ←── KeyLeaseManager (nuevo)
                    │  leases + TTL       │
                    └──┬────────────┬─────┘
                       │            │
          ┌────────────▼──┐  ┌──────▼───────────┐
          │ Agent         │  │ ProxyUseCase      │
          │ (Jules, etc)  │  │ (secret injection) │
          └───────────────┘  └───────────────────┘
```

## Estado Actual (65% implementado)

### ✅ COMPLETO (65 pts)

| Componente | % | Notas |
|---|---|---|
| HardwareVault (keyring) | 100% | DPAPI/Keychain funcional |
| LocalSecretsVault (AES-256-GCM) | 100% | Cifrado en disco con MasterKeyManager |
| SecretStore trait | 100% | Abstracción backend lista |
| EphemeralLease struct | 100% | UUID + TTL + agente |
| KeyLendingEngine (coordination) | 100% | lend/revoke/list/cleanup_expired |
| SecretInjectionStrategy | 100% | Bearer, X-API-Key, GitHubToken |
| GenericProxyRequest.lease_token | 100% | Inyección en proxy |
| AuditLogger (QmdAuditLogger) | 100% | Logging SQLite de operaciones |
| CLI `xavier secrets` | 100% | lend, list-leases, revoke, status |
| CLI `xavier vault` | 100% | set, get, delete |
| ModelProviderConfig (env/fallback vault) | 100% | Lee de vault si env var no existe |
| Secret injection en proxy_use_case | 100% | Lease token resuelve e inyecta key |
| Anomaly scanner | 30% | Existe scanner pero no conectado a leases |
| UI panel | 30% | SecurityConfigPanel para API Tokens (parcial) |

### 🟡 PARCIAL (15 pts)

| Componente | % | Gap |
|---|---|---|
| ModelProviderClient → lease | 30% | Lee de env/vault directamente, NO usa KeyLendingEngine |
| Auto-revoke on task complete | 10% | No hay hook `on_task_complete` que revoque leases |
| Rate limit tracking → lease | 40% | RateLimitManager existe pero no integrado con leases |
| OpenBao/SecretStore | 20% | Existe stub de OpenBao pero no implementado |

### ❌ FALTANTE (20 pts)

| Componente | % | Descripción |
|---|---|:---|
| KeyLeaseManager (wrapper agente) | 0% | Capa que intercepta `ModelProviderClient.generate_text()` → solicita lease |
| Agent lifecycle hooks | 0% | `on_task_start()` → lend, `on_task_complete()` → revoke |
| Anti-exfiltración detector | 0% | Detecta si una API key se usa fuera del proxy |
| Dashboard UI leases | 0% | Panel admin con leases activos, revoke button, histórico |
| MCP tool secret resolution | 0% | MCP tools no usan el sistema de leases |

## Sub-features Requeridas (Para Jules)

Ver issues correspondientes:
- #F1: KeyLeaseManager — wrapper que intercepta ModelProviderClient
- #F2: Agent Lifecycle Hooks — on_task_start/complete
- #F3: Leak Detector — anti-exfiltración
- #F4: Dashboard UI — Leases activos + histórico
- #F5: MCP Secret Resolution — leases para MCP tools
- #F6: OpenBao backend implementación real
- #F7: z.ai / opencode provider (GLM + OpenCode CLI)

## Providers Soportados (Target)

| Provider | API Base | Status |
|----------|----------|--------|
| OpenAI | api.openai.com/v1 | ✅ |
| Anthropic | api.anthropic.com/v1 | ✅ |
| DeepSeek | api.deepseek.com/v1 | ✅ |
| Groq | api.groq.com/openai/v1 | ✅ |
| MiniMax | api.minimax.chat/v1 | ✅ |
| Gemini | generativelanguage.googleapis.com | ✅ |
| **z.ai (GLM)** | **api.z.ai/v1** | ❌ **NUEVO** |
| **OpenCode CLI** | **N/A (CLI call)** | ❌ **NUEVO** |

## Target Score Post-Implementación: ~100%

### Functional Encryption Example
Enable AES-256-GCM encryption in your `.env` configuration:

```sh
# Enable secure storage encryption at rest
XAVIER_ENCRYPTION_KEY="argon2-derived-base64-or-raw-32byte-hex-string"
XAVIER_JWT_SECRET="secure-jwt-signing-secret"
```

When active, database entries are transparently encrypted prior to persistence, ensuring data safety even in untrusted runtime environments.
