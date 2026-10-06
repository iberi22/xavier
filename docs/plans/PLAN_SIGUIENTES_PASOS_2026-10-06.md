# Xavier — Plan de siguientes pasos (2026-10-06, post-puesta-a-punto)

Estado real medido, no recordado. Es lo que hay que hacer a partir de aquí.

## 0. DÓNDE ESTAMOS

| | |
|---|---|
| `xavier` main | `7961771f` · suite lib **3412/0** · clippy `--all-targets -D warnings` exit 0 |
| `xavier-cloud` main | `c33619c` · deploy `7b944b0e` verificado en producción · **0 vulnerabilidades** · 31 tests |
| Artefactos construidos | `.deb` 38.7 MB + AppImage 109.4 MB, verificados leyendo el paquete |
| Disco | 89% (bajó de 90% liberando ~18G) |
| `XAVIER_CODE_EXTRA_ROOTS` | implementado y **conectado** (`668007b5`) |
| Plan jules-2026 | recuperado de `proyectosSWAL/docs/private/` + addendum de estado |

## 1. HALLAZGOS NUEVOS DE ESTE CICLO (no eran conocidos)

### BUG-1 — el crate NO compila en Windows. El `.msi` nunca se pudo construir.
Run `37478138566` falla con 4 errores, todos preexistentes:
- `src/training/backend.rs` (3×): `libc::SIGKILL` no existe fuera de Unix. El módulo
  entero es Unix y NO tiene `#[cfg(unix)]` en las ramas que lo usan.
- `src/memory/access.rs:668`: `tokio::signal::ctrl_c()` devuelve `()`, no un receptor;
  el código hace `rx.recv()`. Solo compila porque esa rama es `cfg(not(unix))`, que
  nunca se ha compilado en esta máquina (esta es NixOS) ni en CI.

**Impacto**: `nsis` y `msi` son targets declarados en `tauri.conf.json` y nunca se han
producido. `FEATURE_STATUS.md` los da por "Stable". Es el mismo patrón que el resto de
esta sesión: target anunciado, nunca construido.

### BUG-2 — el APK falla por toolchain, no por código
Run `37478810952`: `cc-rs: failed to find tool "aarch64-linux-android-clang"`. Falta el
NDK. Además `gradlew` está commiteado sin bit de ejecución (`-rw-r--r--`) y
`compileSdk = 36` requiere instalar la plataforma.

## 2. PASOS, EN ORDEN

### F1 — Arreglar la portabilidad (desbloquea Windows)
Archivos: `src/training/backend.rs`, `src/memory/access.rs`.
- Gatear el entrenamiento por `#![cfg(unix)]` a nivel de módulo, o hacer que
  `signal_group` tenga un stub no-Unix. Recomiendo lo primero: el entrenamiento de
  procesos con señales no tiene sentido en Windows.
- `ctrl_c()` no devuelve receptor: reescribir esa rama para llamar a `drain_for_shutdown`
  directamente, o usar `tokio::signal::windows::ctrl_c()`.
Criterio de éxito: un job de Actions en `windows-latest` que corra
`cargo check --lib --features ci-safe` y salga verde. Añadir ese job **permanentemente**
al CI, no solo al workflow de instaladores: si no, esto vuelve a romperse sin que nadie
se entere.

### F2 — Terminar el `.msi` y el `.exe`
Tras F1, relanzar `windows-installers.yml`. Verificar los artefactos leyendo el `.msi`
(no por el nombre). Descargar `.exe` de Inno Setup y comprobar que arranca.

### F3 — Terminar el APK
- Instalar NDK: `sdkmanager "ndk;26.1.10909125"` + `chmod +x gradlew` + la plataforma 36.
- Decidir si `abiFilters` necesita armv7 (hoy solo se construye aarch64).
Criterio de éxito: APK subido como artefacto. **El firma sigue siendo decisión de Belal**:
sin keystore el APK solo instala con `adb install`, y perder la clave impide actualizar
la app para siempre.

### F4 — Cerrar los targets sinannunciar
Auditar `tauri.conf.json` contra lo que los workflows realmente producen, y hacer que
la tabla de `FEATURE_STATUS.md` diga la verdad. Ahora mismo declara `rpm`, `nsis` y
`msi` como disponibles y ninguno se construía. O se construyen, o se dejan de
anunciar. Un target anunciado que no existe es deuda, no capacidad.

### F5 — Verificación de los artefactos de forma repetible
Hoy verifiqué el `.deb` a mano (leyendo el `ar`). Eso debería ser un script que corra
tras cada build: comprobar que el paquete tiene `debian-binary`/`control.tar.*`/
`data.tar.*`, que el binario existe dentro, y que las `Depends` no están duplicadas
(ahí encontré que Tauri duplica tres).

## 3. LO QUE SIGUE SIENDO DE BELAL

1. **Firma del APK**: crear el keystore y guardarlo en GitHub Secrets. Es una
   credencial; no la genero.
2. **Publicar un release**: `release.yml` con `workflow_dispatch` crearía un release
   PÚBLICO (su condición incluye `workflow_dispatch`). Por eso los workflows nuevos
   suben artefactos y no publican. Decidir cuándo etiquetar.
3. **Vulnerabilidades de xavier** en dependabot (4 moderate, 1 low). Ninguna crítica.

## 4. REGLAS QUE SALEN DE ESTA SESIÓN

- **Un target declarado no es una capacidad.** Si no hay workflow que lo produzca, no
  está soportado. `FEATURE_STATUS.md` debe derivarse de los artefactos, no al revés.
- **La compilación es verificación.** Un `.deb` que compila en `ubuntu-22.04` prueba el
  baseline de glibc, no que instale en Debian. Y "compila" en la máquina del
  desarrollador no es un gate: aquí es NixOS y nunca compiló para Windows.
- **CI es el único verificador de portabilidad.** `#[cfg(not(unix))]` es código que no
  se ejecuta hasta que alguien compila para ese target.
- **Un test verde sobre una función inaccesible no prueba nada.** La lesson de
  cg.02: 8 tests en verde sobre código sin un solo consumidor.