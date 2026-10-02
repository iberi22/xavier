# ADR-037: Rate limiting — fixed window, keyed by provider, not by IP

- **Estado:** aceptada (T5 de la mitigación de seguridad)
- **Fecha:** 2026-10-01
- **Contexto:** había cinco implementaciones de limitador de tasa en el crate, ninguna de ellas
  alineada con su nombre, y dos montadas en caminos distintos del mismo servidor.

## Contexto

Inventario medido (no deducido de los nombres):

| Implementación | Semántica real | ¿Montada en el daemon? |
|---|---|---|
| `adapters/inbound/http/middleware/rate_limit.rs` — `IpRateLimiter` | token bucket | **No.** `create_router()` no tiene consumidor de producción, solo `tests/` |
| `security/sliding_limiter.rs` — `LockFreeSlidingLimiter` | ventana deslizante por buckets | No. Cero call-sites |
| `security/rate_limiter/sliding_window.rs` — `SlidingWindowLimiter` | **ventana fija** (se ancla al primer request y rueda) | No. Cero call-sites |
| `auth2/middleware.rs` — `RateLimiter` | ventana deslizante real (`Vec<Instant>` retenido) | Solo dentro de `auth2::auth_routes` |
| `cli/http_setup.rs` — `auth_window_allow` | **ventana fija** de 60 s | **Sí.** `server.rs:1684`, sobre el nest `/auth` |

Dos consecuencias:

1. El limitador montado en `/auth` **es** de ventana fija, y el que el nombre `IpRateLimiter`
   sugiere está en el router que solo usan los tests.
2. `security/sliding_limiter.rs` exigía en su test de integración exactamente 50/50 con 100
   hilos. Medido: en 400 rondas, 1 no fue exactamente 50 y **se pasó del límite** (51 admitidos
   con tope 50). El test era una propiedad que el código no garantiza. Además era código muerto.

## Decisión

1. **Borrar** `security/sliding_limiter.rs` y `tests/test_sliding_limiter.rs`, con sus dos
   `pub use` en `security/mod.rs`. No hay consumidor, y un test inestable esperando su día es
   deuda, no cobertura.
2. **Arreglar la carrera de A3** en `WindowCounter` (`rate_limiter/sliding_window.rs`): decidir
   y contar pasan a ser una sola sección crítica bajo un mutex por clave. Medido antes del
   arreglo: 11 de 10 llamadores concurrentes admitidos sobre una clave nueva (1 de 50 rondas).
   Un mutex por clave sin contención cuesta decenas de nanosegundos, y el mapa que los contiene
   ya está detrás de un `RwLock`: la versión «lock-free» no compraba nada.
3. **Mantener la semántica de `count`** como «requests vistos en la ventana», Contando antes de
   decidir, para no debilitar ningún test existente.
4. **No** cambiar a ventana deslizante aquí. El requisito de seguridad verificado en producción
   es «no más de N intentos por minuto», y una ventana fija de 60 s lo cumple salvo en la
   frontera, donde admite 2N dentro de ~1 ms. Para un tope de 20/min esa frontera es un pico de
   40 intentos y no una vía de fuerza bruta sustainable. Cambiar a deslizante exigiría un
   `Vec<Instant>` por clave con poda, es decir memoria no acotada por el número de claves, y no
   se ha medido ninguna propiedad que lo justifique. La diferencia fija-vs-deslizante queda
   fijada por test (`t5_ventana_fija_admite_doble_en_la_frontera`) para que nadie lea
   «sliding» y crea una garantía que el código no tiene.
5. **Renombrar la realidad, no el código.** El módulo ahora dice en su doc que es de ventana
   fija. Renombrar el tipo público `SlidingWindowLimiter` queda para otra PR: tiene re-export en
   `security/mod.rs` y nombre en la spec.

## Un limitador por IP no protege a este nodo

Medido en la máquina del nodo:

- El daemon escucha en `127.0.0.1:8006` y `127.0.0.1:8100`.
- No hay ningún proceso `cloudflared`, `ngrok` ni `frp`.

Los endpoints SaaS los sirve `cores/xavier-cloud` (Cloudflare Worker), no un túnel a esta
máquina. Por tanto un limitador por IP **solo ve IPs de loopback**: una única clave para todo el
tráfico, de modo que un atacante externo no lo puede distinguishes de un cliente legítimo y el
límite por IP no constituye una defensa. Agotarlo solo castiga al operador.

La clave correcta es **identidad, no dirección**: `agent:{lease.agent_id}` cuando hay lease, que
es justo lo que ya hace `auth_rate_limit_middleware` (`http_setup.rs:344`). Es la única de las
cinco implementaciones que keyea por identidad, y es la que está montada.

## Consecuencias

- Se cierra A3 (admisión concurrente por encima del tope) y T5 (código muerto + test inestable).
- Persiste el punto ciego real, que **no es de este crate**: `cores/xavier-cloud` resuelve el
  tenant con `tenantFor()` mediante un `sha256Hex` + lookup en KV por petición, **sin límite de
  intentos fallidos**. Quien esté delante del Worker sí puede martillear el login desde internet.
  Corregirlo es otro repo y otra PR; aquí queda documentado para que no se pierda.
- `create_router()` sigue siendo el único sitio con token bucket y lo usan solo los tests. No se
  toca: cambiarlo alteraría el contrato de esos tests sin cambiar nada de lo que llega por HTTP.