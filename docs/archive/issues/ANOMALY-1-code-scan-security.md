# Issue: ANOMALY-1 — Path Escaping & Lack of Allowed Workspace Validation in `code_scan_handler`

**Status:** 🔴 PENDING
**Labels:** jules, security, high-priority
**Created:** 2026-05-21

---

## 🎯 Objetivo
Añadir validación estricta de rutas en `code_scan_handler` para evitar escaneos de rutas absolutas o relativas fuera del espacio de trabajo permitido (`XAVIER_WORKSPACE_DIR` o el directorio del proyecto).

## 📋 Descripción del Problema
El endpoint `POST /code/scan` en `src/adapters/inbound/http/handlers/code.rs` permite especificar un parámetro `path`. 

Aunque el código actual tiene un chequeo simple para evitar path traversal relativo (`requested_path.contains("..")`), **no valida rutas absolutas** (ej. `/etc`, `/usr`, o rutas de red). Un atacante que obtenga acceso al servidor o un agente malicioso podría escanear e indexar directorios del sistema operativo entero, comprometiendo la privacidad de los archivos de la máquina host.

```rust
// Código vulnerable actual en src/adapters/inbound/http/handlers/code.rs
let requested_path = payload.path.unwrap_or_else(|| ".".to_string());

if requested_path.contains("..") {
    return Ok(Json(serde_json::json!({
        "status": "error",
        "message": "path traversal not allowed",
        "indexed_files": 0,
    })));
}

// Escanea directamente la ruta recibida, permitiendo rutas absolutas fuera del workspace!
match state.code_indexer.index(Path::new(&requested_path)).await { ... }
```

## 🔧 Archivos Afectados
*   `src/adapters/inbound/http/handlers/code.rs`

## ✅ Criterios de Aceptación
1.  **Resolución de Ruta Absoluta:** Resolver la ruta a escanear a su forma canónica usando `std::fs::canonicalize` antes de procesarla.
2.  **Validación de Workspace:** Obtener el workspace actual o el directorio permitido de Xavier (utilizando `XAVIER_WORKSPACE_DIR` o `std::env::current_dir()`).
3.  **Restricción Estricta:** Comprobar que la ruta canónica del escaneo comienza con el prefijo del directorio de workspace permitido. De lo contrario, retornar un error `400 Bad Request` o un JSON de bloqueo:
    ```json
    {
      "status": "blocked",
      "reason": "path_outside_workspace",
      "message": "Scanned path must reside within the allowed workspace directory."
    }
    ```
4.  **Tests Unitarios:** Implementar o extender una prueba unitaria que intente escanear `/etc` o una ruta absoluta fuera del workspace y verifique que es bloqueada de inmediato.

## 🔧 Comandos de Verificación
1.  Correr `cargo check` para asegurar que compila:
    ```bash
    cargo check --target-dir target_local
    ```
2.  Ejecutar el test de seguridad:
    ```bash
    cargo test --target-dir target_local --lib adapters::inbound::http::handlers::code::tests
    ```
