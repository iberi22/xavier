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
| Config por repo | ✅ | `src/codebase/repo_config.rs` |
| CLI de config | ✅ | `src/cli/commands/repo.rs` |
| Cifrado en reposo del paquete | ✅ | `src/codebase/repo_package_crypto.rs` |
| `pack` / `unpack` del paquete | ✅ | `src/cli/commands/repo_pack.rs` |
| La config manda en la identidad | ✅ | `src/codebase/repo_identity.rs` |
| BD de memoria por repo (ruta) | ✅ ruta, no aislamiento | `src/memory/repo_memory_store_path.rs` |
| **Aislamiento físico de la memoria** | ❌ pendiente | ver abajo |

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

## Empaquetar y restaurar

```bash
xavier repo pack   -C <dir>   # cifra code_graph.db -> code_graph.db.enc
xavier repo unpack -C <dir>   # lo restaura y muestra el header
```

`pack` deja `<repo>/.xavier/code_graph.db.enc` listo para commitear: es el
único artefacto que hay que versionar. El `indexed_commit` viaja dentro del
paquete cifrado, así que el codegraph siempre se restaura en la versión del
commit con el que se construyó. Sin checkpoint ni HEAD de git, el valor es
`unknown` — nunca un hash inventado.

`unpack` es **atómico**: descifra a un temporal y renombra solo si todo va
bien, de modo que un paquete corrupto o cifrado con la clave de otro repo no
deja un `code_graph.db` a medias.

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

## Lo que falta: aislamiento físico de la memoria

El aislamiento del **codegraph** está hecho. El de los **registros de
memoria**, no, y conviene decirlo sin adornos.

Hoy `memory_store_path_for()` resuelve la ruta por repo y la selección a
partir de la `RepoIdentity`, pero el daemon sigue abriendo **un único store
global** (`data/vec-store.sqlite3`), con todos los registros multiplexados en
la columna `workspace_id`. Dos repositorios que compartan ese daemon comparten
las mismas filas.

Lo que falta para el aislamiento real es mover ese multiplexado a una base
por repo. Es el paso grande que queda, y no se ha hecho a propósito en esta
ola: cambia dónde viven 22.000+ registros existentes y pidió explícitamente no
tocar las memorias actuales.

Nada de lo hecho hasta aquí migra ni modifica memorias existentes.
