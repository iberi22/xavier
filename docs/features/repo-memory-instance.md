# Per-repo memory instance (XAV-REPO)

Cada repo git —y cada producto dentro de un monorepo— tiene su propia
instancia de memoria: su codegraph, su configuración, su nivel de
privacidad, y un paquete cifrado que se puede commitear al repo para
sincronizarlo.

El repositorio de memoria por repo vive en `<repo>/.xavier/`.

## Estado: parcialmente implementado

| Pieza | Estado | Dónde |
|---|---|---|
| Identidad por repo | ✅ existe | `src/codebase/repo_identity.rs` |
| `code_graph.db` por repo | ✅ existe | `src/codebase/codegraph_paths.rs` |
| Codegraph anclado a un commit | ✅ existe | `.xavier/codegraph-sync-commit` |
| Config por repo | ✅ nueva | `src/codebase/repo_config.rs` |
| CLI de config | ✅ nuevo | `src/cli/commands/repo.rs` |
| Cifrado en reposo del paquete | ✅ nuevo | `src/codebase/repo_package_crypto.rs` |
| **Paquete exportable por CLI** | ❌ pendiente | falta el flujo `pack`/`unpack` |
| **BD de memoria por repo** | ❌ pendiente | las memorias viven en el store global |

## Configuración por repo

Se declara en `<repo>/.xavier/config.toml`, y se gestiona con:

```bash
xavier repo config init  -C <dir>      # crea el config junto al producto
xavier repo config show  -C <dir>      # muestra la configuración efectiva
xavier repo config set project_id <v> -C <dir>
```

El fichero se commitea para compartir la instancia con el equipo.

```toml
project_id = "duque-mvp"
name = "duque-mvp"

[privacy]
default_clearance = "internal"    # private | internal | team | public

[codegraph]
sync_to_repo = false              # si el paquete cifrado se commitea

# embedding_dimensions = 768       # vacío = hereda el global
# retention_days = 90              # vacío = sin retención
```

**Ausente = comportamiento actual.** Un repo sin `.xavier/config.toml`
sigue funcionando exactamente igual; el fichero nunca es obligatorio. Un
TOML inválido produce un error claro, nunca un panic.

### Por qué `project_id` es configurable

`derive_project_id()` deriva el id del **nombre del directorio del repo**.
En un monorepo eso colapsa todos los productos al mismo id: `duque-mvp` y
`tripro-web` serían la misma instancia. Declarando `project_id` en cada
producto, cada uno tiene la suya.

Por eso `init -C <producto>` escribe el config **junto al producto**, no en
la raíz del git: si se escribiera en la raíz, los dos productos del mismo
repo leerían y escribirían el mismo fichero y no podrían separarse.

Todo `project_id`, venga del directorio o del config, pasa por el mismo
`sanitize_project_id()`. No hay dos normalizadores: un id con guion o con
barras nunca puede acabar siendo una ruta en un caso y no en el otro.

## Paquete cifrado

El codegraph por repo (`code_graph.db`) se cifra en reposo para poder
commitearse a un repo git —privado o público— sin filtrar nada.

- Reusa el motor de cifrado existente del proyecto
  (`crypto::encryption::{aes_encrypt, aes_decrypt}` y
  `memory::sqlite_vec_store::at_rest::{wrap_dek, unwrap_dek}`). No se
  introduce criptografía nueva.
- **KEK por repo**: HKDF-SHA256 sobre la master key con `info`
  domain-separated (`xavier-repo-package-kek-v1:<project_id>`), el mismo
  patrón que `MasterKeyManager::auth_db_key()`. Dos repos nunca comparten
  KEK, así que la clave de un repo no abre el paquete de otro.
- **DEK aleatorio por operación**, envuelto con `wrap_dek`: la KEK nunca
  toca el payload directamente.
- Contenedor `XAVPKG01` + versión, con un header que lleva `project_id`,
  `source_file_name` e `indexed_commit` — el codegraph viaja anclado al
  commit con el que se construyó. `plaintext_len` se deriva del propio
  payload, así que un header nunca puede contradecir a su ciphertext.
- API: `seal`/`open`, `encrypt_file`/`decrypt_file`, y `verify_file` para
  comprobar integridad sin escribir nada.

### Verificado con una BD real

Cifrando un `code_graph.db` que contenía el secreto
`SECRETO_CLIENTE_LEONARDO_DUQUE_9931`:

- el secreto **no** aparece en claro en el `.enc`;
- la cabecera `SQLite format 3` tampoco;
- el roundtrip es byte-idéntico;
- la clave de **otro** repo es rechazada, sin escribir ningún fichero.

## Cómo encaja con el resto

`repo_identity.rs` resuelve el grafo anclado a la raíz del repo que pide la
consulta y **nunca** sustituye datos de otro repo; un repo sin índice se
reporta como `degraded` **para ese repo**, con su identidad propia.
`codegraph-sync-commit` registra el commit indexado, y las consultas lo
reportan como `unknown` cuando no hay checkpoint — nunca inventa un hash.

## Pendiente

1. **Flujo CLI del paquete**: existe la criptografía, falta `pack`/`unpack`
   para cifrar y descifrar el paquete desde la línea de órdenes.
2. **BD de memoria por repo**: hoy solo hay `code_graph.db` por repo; los
   registros de memoria siguen en el store global. Sin esto el aislamiento
   es del codegraph, no del resto de la memoria.
3. **Cablear la config a `derive_repo_identity()`** y a
   `codegraph_sync::write_checkpoint()`, que hoy siguen el camino antiguo.

Nada de esto migra ni modifica las memorias existentes: es trabajo futuro
sobre la misma base.
