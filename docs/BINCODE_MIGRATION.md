# Plan de Migración: Bincode

> **Fecha:** 2026-06-04
> **Repositorio:** xavier (iberi22/xavier)
> **Estado actual:** Ya no hay dependencia directa, solo transitiva via `gllm` → `burn` → `burn-core` → `bincode v2.0.1`

---

## 1. Situación Actual

### Dependencia directa: ❌ No

Bincode **no está declarado** en `Cargo.toml` del workspace ni de `code-graph`. Fue eliminado como dependencia directa en commits previos.

### Dependencia transitiva: ✅ Sí

`bincode v2.0.1` llega como dependencia transitiva a través de:

```
xavier
 └── gllm v0.10.6 (opcional, feature "local-gllm")
      └── burn v0.20.1
           └── burn-core v0.20.1
                └── bincode v2.0.1
```

### Uso en código fuente: ❌ No

Búsqueda exhaustiva (`Select-String -Pattern "bincode"` en todos los `*.rs`): **cero resultados**. El código fuente de Xavier nunca llama a `bincode` directamente.

### Serialización que usa Xavier actualmente:

| Librería | Uso | Ámbito |
|----------|-----|--------|
| **serde + serde_json** | Serialización/deserialización principal | ~200+ ocurrencias en toda la codebase |
| **zerocopy** | Zero-copy para bytes crudos | Dependencia directa declarada |
| **serde (derive)** | Derivas `Serialize`/`Deserialize` en casi todos los structs | Ubicuo |

**Conclusión:** La serialización binaria del proyecto ya está manejada por `serde_json` y `zerocopy`. Bincode solo existe en el árbol de dependencias como artifact de `gllm`/`burn`.

---

## 2. ¿Qué es bincode y por qué migrar?

### bincode v1 (unmaintained)

- El `bincode` original (v1.x) fue **declarado sin mantenimiento** por su autor en 2020-2021
- Quedó con vulnerabilidades conocidas (RUSTSEC-2021-0129, RUSTSEC-2021-0130, RUSTSEC-2021-0131)
- No soporta `#[non_exhaustive]`, `flatten` ni `adjacent` de serde correctamente
- Ya no recibe parches de seguridad

### bincode v2 (la versión actual en el árbol)

- `bincode v2.0.1` es un fork mantenido por la comunidad de `burn` (burn-core lo requiere)
- Es más seguro y rápido que v1
- Sin embargo, sigue siendo un formato *vendored/implícito* controlado por upstream
- Si burn migra a otro formato en el futuro, bincode se actualizaría automáticamente

### Alternativas modernas

| Alternativa | Ventajas | Desventajas | Ideal para |
|-------------|----------|-------------|------------|
| **bincode v3** | Mantenido activamente, feature `serde` completa, más rápido | API nueva (Encode/Decode + serde) | Todo propósito general |
| **postcard** | Diseñado para embedded/no_std, compacto | No zero-copy, limitado a 2^16 tamaño | IoT, WASM, buffers pequeños |
| **rkyv** | Zero-copy real, archivos mapeados en memoria | No soporta `serde::Serialize` nativo (usa Archive trait propio) | Checkpoints grandes, archivos mmap |
| **messagepack (rmp-serde)** | Estándar binario portable, soporte en otros lenguajes | Más overhead que bincode (~10-20%) | Logs, intercambio entre servicios |
| **ciborium (CBOR)** | Estándar IETF, self-describing | Pesado, más overhead | Interoperabilidad |
| **speedy** | Muy rápido, zero-copy parcial | Poco adoptado, ecosistema pequeño | Proyectos nuevos |
| **flatbuffers / cap'n'proto** | Zero-copy, acceso sin deserializar | Schema externo, tooling complejo | Alto rendimiento, pipelines streaming |

---

## 3. Recomendación para Xavier

Dado que **no hay código propio que use bincode**, la estrategia se divide en dos partes:

### A. Si solo preocupa la dependencia transitiva (bajo riesgo)

**Decisión: NO HACER NADA** ⏸️

Razones:
- `bincode v2` es mantenido por el ecosistema `burn` (activo, con releases regulares)
- `gllm` es una dependencia **opcional** (feature `local-gllm`). Si no se compila con ella, bincode ni siquiera aparece en el build
- Si `burn` migra a otro formato, bincode se irá automáticamente con `cargo update`
- No hay superficie de ataque en código propio

### B. Si se quiere eliminar completamente la dependencia

**Ruta:** Esperar o contribuir a que `gllm`/`burn` actualicen sus dependencias.

`burn-core` en su versión actual (v0.20.1) depende de `bincode v2.0.1`. Para eliminarlo:
1. `cargo update -p bincode` → intenta subir a última versión disponible
2. Monitorear releases de `burn` que remuevan la dependencia
3. Alternativamente: evaluar migración de `gllm` a otro provider de embeddings que no requiera `burn`

---

## 4. Plan de Acción

### Paso 1: Verificar que no hay código directo ✅ YA HECHO

```powershell
# Confirmado: 0 archivos .rs mencionan "bincode"
Select-String -Path src\**\*.rs -Pattern "bincode"  # → vacío
```

### Paso 2: Verificar alcance transitivo ✅ YA HECHO

```powershell
cargo tree -i bincode
# → solo via gllm → burn → burn-core
```

### Paso 3: Intentar actualizar bincode a v3 (2026) 🟡 OPCIONAL

```powershell
cargo update -p bincode
```

Si Cargo.lock se actualiza a v3.x, el problema está resuelto — bincode v3 es un proyecto mantenido activamente.

### Paso 4: Monitorear releases de burn 🟢 RECOMENDADO

Agregar a la checklist de mantenimiento:
- Cada vez que se actualice `gllm`, revisar si `burn` (y por tanto `bincode`) se actualizó
- Si `burn` deja de depender de bincode, hacer `cargo update` para limpiar el lockfile

### Paso 5: Migración completa (solo si hay necesidad futura) 🔴 ALTO ESFUERZO

Si en el futuro se decide usar serialización binaria directamente en Xavier, evaluar en orden:

1. **postcard** — si se necesita serializar structs serde en binario compacto para persistencia o red
2. **rkyv** — si los checkpoints/sessions necesitan zero-copy para gran volumen de datos
3. **speedy** — si se quiere máxima velocidad con API simple

---

## 5. Esfuerzo Estimado

| Acción | Esfuerzo | Prioridad | Dependencia |
|--------|----------|-----------|-------------|
| Verificar código propio | ✅ Completado | — | — |
| `cargo update` a bincode v3 | **Fácil** (1-2 min) | 🟡 Baja | `cargo` CLI |
| Monitorear release de burn | **Fácil** (5 min/mes) | 🟢 Mínima | — |
| Migrar bincode en código propio | N/A | — | No hay código que migrar |
| Reemplazar `burn`/`gllm` por otro provider | **Complejo** (semanas) | 🔴 No recomendado ahora | Breaking change en embeddings |

---

## 6. Resumen

| Aspecto | Estado |
|---------|--------|
| ¿Hay código que use bincode? | ❌ No |
| ¿Hay dependencia directa? | ❌ No |
| ¿Hay dependencia transitiva? | ✅ Sí (gllm → burn → burn-core → bincode v2.0.1) |
| ¿bincode está desactualizado? | Parcialmente — v2.0.1 es un fork mantenido por el ecosistema burn |
| ¿Riesgo de seguridad real? | Muy bajo — código ajeno, superficie no expuesta |
| **Acción recomendada** | **No migrar ahora**. Monitorear updates de `gllm`/`burn`. Si se añade serialización binaria propia, evaluar `postcard` o `rkyv`. |

---

## Appendices

### A. Comandos útiles para monitoreo futuro

```powershell
# Ver versión actual de bincode en lockfile
Select-String -Path Cargo.lock -Pattern 'name = "bincode"' -Context 0,3

# Ver de dónde viene
cargo tree -i bincode

# Actualizar todo el sub-árbol de burn
cargo update -p burn-core
```

### B. Bincode v3 (2026)

A junio 2026, `bincode v3.0.0` está disponible en crates.io y es un proyecto mantenido activamente. Los cambios principales:

- API nativa `Encode`/`Decode` (independiente de serde)
- Soporte continuo para el feature `serde` (compatibilidad con derives existentes)
- `Cu-bincode` (v2 fork mantenido) como puente para ecosistemas legacy
- `bincode_reloaded` es otro fork independiente v3.1.3

Si `burn-core` actualiza a bincode v3, `cargo update` resolverá automáticamente.
