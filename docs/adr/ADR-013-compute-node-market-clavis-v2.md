# ADR-013: Rediseño de la Arquitectura de Cómputo Distribuido — Enfoque Sidecar + Fallback HTTP

*Status: PARTIALLY ACCEPTED / REWRITTEN | Date: 2026-07-18*

---

## Contexto

Originalmente, se concibió un diseño de 4 fases para la provisión distribuida de recursos de GPU y cómputo para Xavier, con una duración estimada de 7 semanas de trabajo completo:
1. **Sidecar GPU** (2 semanas) - Lanzamiento de un sidecar de cómputo GPU local.
2. **Clavis v2 / ComputeLease JWT** (1 semana) - Autenticación criptográfica de leases mediante JWT.
3. **Compute Node Market** (2 semanas) - Red P2P Gossipsub, subastas (bids), custodia multifirma (escrow), y reputación distribuida.
4. **MoE Router** (1 semana) - Mixture of Experts por embeddings para enrutamiento inteligente de consultas.

Sin embargo, tras el análisis estratégico detallado en `docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md` y de acuerdo con las directrices de `AGENTS.md` para el ecosistema SWAL, se ha determinado que el diseño original introducía un nivel crítico de sobre-ingeniería que compromete el time-to-market y la mantenibilidad de la aplicación. Las arquitecturas de mercado descentralizado sin confianza, el enrutamiento complejo basado en embeddings y la facturación web2 centralizada (como Stripe) se alejan de los objetivos de diseño pragmáticos y soberanos de SWAL.

Por lo tanto, este documento reescribe por completo el ADR-013 original para aprobar únicamente un enfoque ligero y robusto compuesto por un sidecar local de GPU y un sistema determinista de tolerancia a fallos mediante fallback HTTP. Esto reduce el esfuerzo total de desarrollo a solo **3 semanas**.

---

## Decisiones de Aceptación (Aceptados)

### 1. Sidecar GPU (`xavier-gpud`)
- **Estado**: **ACEPTADO**
- **Decisión**: Implementar el daemon de monitoreo e inferencia de GPU como un binario nativo llamado `xavier-gpud` dentro del Cargo Workspace de Xavier (no como un repositorio independiente).
- **Justificación**: Integrar `xavier-gpud` directamente en el monorepo simplifica enormemente la gestión del ciclo de vida del software, el control de versiones y el empaquetado. Permite compartir tipos y utilidades del workspace sin la necesidad de publicar dependencias externas ni mantener repositorios separados.
- **Mapeo al Código**: Se ubicará en la raíz del workspace como una crate adicional (`bin/xavier-gpud/` o `xavier-gpud/`).

### 2. Fallback HTTP a Peers (`ProviderKind::Local`)
- **Estado**: **ACEPTADO**
- **Decisión**: Extender `ProviderKind::Local` para incluir una lista ordenada y secuencial de URLs de peers como fallback de hardware de inferencia.
- **Justificación**: En lugar de depender de una red compleja de gossipsub para emparejar nodos de cómputo, la tolerancia a fallos se resuelve de manera determinista y de bajo consumo de recursos. Si el nodo local o la GPU local está saturada o no responde, el cliente consulta secuencialmente los endpoints HTTP configurados en la lista ordenada hasta obtener una respuesta exitosa.

---

## Decisiones de Rechazo (Rechazados)

### 1. Compute Node Market (Gossipsub, Bids, Escrow, Reputación)
- **Estado**: **RECHAZADO**
- **Justificación**: Diseñar una red de subastas de cómputo en tiempo real sobre Gossipsub, complementada con mecanismos de custodia multipartita (escrow) y sincronización de reputaciones, representa una complejidad operativa masiva. Esto desvía el foco de Xavier de ser un motor de contexto ágil y local-first.
- **Referencia a Documento de Investigación**: Conforme a `docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md`, la sincronización de estados distribuidos en un mercado abierto sin confianza no se alinea con la prioridad de ofrecer integraciones deterministas y de alto rendimiento en el laboratorio de desarrollo.

### 2. MoE Router (Mixture of Experts por Embeddings)
- **Estado**: **RECHAZADO**
- **Justificación**: El enrutamiento dinámico mediante similitud semántica de embeddings introduce una latencia inaceptable para consultas en caliente y añade impredecibilidad en el flujo RAG. En su lugar, el despacho de tareas a modelos específicos de GPU o CPU se realiza de forma directa y explícita según la configuración declarativa del usuario.
- **Referencia a Documento de Investigación**: En `docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md` se rechazan las rutas dinámicas por embeddings en favor de una correspondencia estricta y predecible de llamadas a APIs para optimizar el rendimiento y la depuración del sistema.

### 3. Clavis "v2" y ComputeLease JWT
- **Estado**: **RECHAZADO**
- **Justificación**: Al no existir un mercado dinámico de nodos de cómputo ni un esquema de arrendamiento temporal multi-inquilino, el desarrollo de "ComputeLease" basados en JWT firmados es redundante. Clavis v1, que proporciona almacenamiento de claves en hardware vault seguro y préstamos efímeros para la integración del bot de Telegram, es totalmente estable y suficiente para proteger los endpoints.
- **Referencia a Documento de Investigación**: Conforme a `docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md`, se prioriza la simplicidad de la seguridad perimetral de red tradicional (mecanismos de token estático o mTLS de la capa mesh) sobre contratos de lease criptográficos autoadministrados.

### 4. Stripe como Método de Pago
- **Estado**: **RECHAZADO (Violación de AGENTS.md)**
- **Justificación**: El uso de pasarelas de pago web2 centralizadas como Stripe viola de forma directa los principios de soberanía del ecosistema SWAL definidos en las directivas de desarrollo del proyecto. La economía de la red y el acceso pro de los nodos se rigen exclusivamente por la posesión, el staking y el aporte de valor medido mediante el token de utilidad `$SWAL`.
- **Referencia a AGENTS.md**: `AGENTS.md` prohíbe explícitamente el uso de Stripe ("active SWAL node only — no Stripe as Pro unlock; ownership + stake yield via $SWAL"). De igual forma, `docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md` ratifica que el ecosistema debe permanecer autónomo, libre de la mediación de entidades financieras tradicionales.

---

## Plan de Fases Actualizado

La reestructura de la arquitectura disminuye el plan de trabajo de 7 semanas a un esfuerzo de **3 semanas** en total:

```
+------------------------------+       +------------------------------+
|   Fase 1: Sidecar GPU        | ----> |   Fase 2: Fallback HTTP      |
|   (xavier-gpud) [2 semanas]  |       |   a Peers [1 semana]         |
+------------------------------+       +------------------------------+
```

### Fase 1: Sidecar GPU (`xavier-gpud`)
- **Duración**: 2 semanas.
- **Tareas principales**:
  - Creación del subproyecto `xavier-gpud` dentro del Cargo Workspace.
  - Implementación de la API HTTP de control local (consulta de estado de VRAM, utilización, modelos activos).
  - Proxy de inferencia local que despacha peticiones de embedding y generación al backend seleccionado (Ollama, Llama.cpp, CUDA local).

### Fase 2: Fallback HTTP determinista
- **Duración**: 1 semana.
- **Tareas principales**:
  - Modificación del backend de inferencia local en Xavier para soportar fallbacks en cadena.
  - Implementación del soporte de múltiples endpoints HTTP ordenados bajo `ProviderKind::Local`.
  - Mecanismo robusto de reintento secuencial inmediato ante fallos de conexión, timeout o falta de recursos (VRAM out of memory) en el peer de mayor prioridad.

---

## Consecuencias

### Positivas (+)
- **Entrega Acelerada**: Reducción del ciclo de desarrollo de 7 a 3 semanas.
- **Confiabilidad e Inspección Simplificadas**: Depurar un fallback secuencial HTTP es infinitamente más simple que diagnosticar transacciones fallidas de mercado en Gossipsub.
- **Cumplimiento Normativo Interno**: Alineación total con el modelo de tokens `$SWAL` sin incurrir en violaciones a `AGENTS.md`.
- **Huella de Memoria Reducida**: Al remover componentes de mercado y enrutamiento por embeddings, el consumo base de RAM del nodo se mantiene sumamente ligero.

### Negativas (-)
- **Coordinación Manual**: Requiere que los operadores configuren de forma declarativa las direcciones de los peers de confianza en su archivo de configuración local, perdiendo el descubrimiento dinámico automático de mercados de cómputo abiertos.

---

## Referencias
- [Plan Definitivo XTSP GPU Lab](docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md) (Estrategia de simplificación)
- [AGENTS.md de Xavier](../../AGENTS.md) (Restricción explícita de pasarelas de pago y lineamientos de tokens)
- [ADR-005: Multi-crate Migration](./005-multi-crate-migration.md) (Gestión de workspace)
