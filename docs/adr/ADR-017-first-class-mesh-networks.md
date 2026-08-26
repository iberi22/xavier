# ADR-017: Redes privadas de primera clase + GRANT cruzado granular

*Status: ACCEPTED | Date: 2026-08-26 | Deciders: Xavier Mesh Team*

---

## Contexto

Xavier ya dispone de `PrivateMeshRegistry` (`src/mesh/private_mesh.rs`) con sync cifrado por wallet (`PrivateSyncPayload` + AES-GCM derivado de `wallet_id`), espacios `/v1/f12/groups` y ACL `enterprise/rbac.rs` (`Permission{Read,Write,Delete,Share,Manage}`).

Limitaciones detectadas:

1. **Sin redes de primera clase**: el modelo wallet → nodos es plano. Un nodo no puede pertenecer a N redes con identidad/ACL por red, requisito para aislamiento multi-tenant y topologías `swal/{app_id}/{instance_id}`.
2. **Sin grant cruzado granular**: compartir un recurso concreto con un nodo externo exige exponer toda la wallet o duplicar datos. Falta `recurso → red → Permission → expiry → revocación`.

Se requiere que la primitiva de red sea estructurada, persistente y verificable por tests.

## Decisión

### 1. Nodos en N redes separadas (`src/mesh/network.rs`)

- `struct MeshNetwork { id, name, owner_node, members: Vec<String>, acl: NetworkAcl, created_at, updated_at }`
  - `owner_node` es primer miembro y posee todos los permisos.
  - `members` permite pertenencia N:M (un nodo en N redes).
- `struct NetworkAcl { default_permission: Option<Permission>, grants: Vec<CrossGrant> }`
- `struct CrossGrant { id, resource_id, target_node, permission, expires_at: Option<DateTime<Utc>>, revoked, created_at }`

`impl MeshNetwork`:

- `create_network(id, name, owner_node)` — crea red con owner como miembro.
- `add_member(node_id)` / `remove_member(node_id)` — gestión de membresía (owner no removible, duplicado rechazado).
- `grant_cross(resource_id, target_node, permission, expires_at)` — crea grant con ULID.
- `revoke_grant(grant_id)` — marca `revoked=true`; doble revocación → error.
- `check_permission(node, resource, permission) -> bool` — activo si: owner OR grant activo matching `resource` (o wildcard `*`) + permiso + no expirado + no revocado, o `default_permission` si miembro. `is_active()` encapsula `revoked` + `expires_at`.

### 2. Registro integrado (`src/mesh/private_mesh.rs`)

`PrivateMeshRegistry` incorpora `networks: HashMap<String, MeshNetwork>` persistido junto a `nodes` en `mesh/private-mesh.json` vía `PersistedRegistry { nodes, networks }` con migración compatible hacia atrás (array legacy → objeto).

Métodos delegados: `create_network`, `add_member`, `remove_member`, `grant_cross`, `revoke_grant`, `check_permission(node, resource) -> bool` (atajo Read), `check_permission_with_perm`, `check_permission_in_network`, `all_networks`, `networks_for_node`, `get_network`.

### 3. Superficie HTTP (`src/server/f12_routes.rs`)

```
POST   /v1/f12/networks                          — crear red  {id,name,owner_node}
GET    /v1/f12/networks[?node_id=...]            — listar redes del caller (o todas)
POST   /v1/f12/networks/{id}/members             — añadir miembro {node_id}
POST   /v1/f12/networks/{id}/grants              — crear grant {resource_id,target_node,permission,expires_at?}
DELETE /v1/f12/networks/{id}/grants/{grant_id}   — revocar
```

Validación: `permission` ∈ {read,write,delete,share,manage}, `expires_at` RFC3339, ids no vacíos.

## Consecuencias

### Positivas

- **Aislamiento por red**: cada red tiene ACL propia; un nodo participa en N redes sin compartir wallet completa.
- **Grant granular con lifecycle**: recurso → nodo → permiso → expiración temporal + revocación explícita; `check_permission` respeta ambos.
- **Persistencia atómica**: redes y nodos en un solo fichero JSON, migración sin ruptura.
- **Verificabilidad**: `cargo test --lib mesh::network` y `mesh::private_mesh` cubren crear red, membresía, grant, revocar, expiry.
- **Sin nuevas dependencias**: reutiliza `chrono`, `ulid`, `serde`, `Permission`.

### Negativas / Riesgos

- **Fichero único crece**: con muchas redes/grants el JSON puede crecer; mitigado por paginación futura en `list_networks`.
- **Wildcard `*` amplio**: un grant `resource_id="*"` otorga acceso total; requiere revisión de uso y auditoría.
- **Expiración basada en reloj local**: `Utc::now()` exige sincronía NTP; drift puede causar grants considerados válidos/expirados incorrectamente.
- **Sin GC de grants revocados/expirados**: se conservan para auditoría; eventual compacción necesaria.

## Referencias

- `src/mesh/network.rs`, `src/mesh/private_mesh.rs`, `src/server/f12_routes.rs`
- `src/enterprise/rbac.rs` (Permission)
- SWAL namespace `swal/{app_id}/{instance_id}`, GitCore 3.8.0
