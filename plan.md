1. **Expose `HardwareVault::isolated`**
   - In `src/secrets/vault.rs`, change the gating for `custom_storage_dir`, `custom_vault_key`, and `keyring_stub_ok` on `HardwareVault` from `#[cfg(test)]` to `#[cfg(any(test, feature = "test-utils"))]`.
   - Update `isolated` and `with_keyring_stub` functions to be `pub fn` instead of `pub(crate) fn` and change their gate to `#[cfg(any(test, feature = "test-utils"))]`.
   - Update the references to `#[cfg(test)]` in `try_keyring_store`, `try_keyring_get`, `try_keyring_delete`, `storage_dir`, and `fallback_key` to `#[cfg(any(test, feature = "test-utils"))]`.

2. **Expose `set_clavis_vault`**
   - In `src/adapters/inbound/http/handlers/clavis.rs`, change `#[cfg(test)]` to `#[cfg(any(test, feature = "test-utils"))]` on `set_clavis_vault` and change its visibility to `pub fn set_clavis_vault`.

3. **Update Tests to Use `isolated` and RAII guards**
   - Wait, `HardwareVault::isolated(path, key)` modifies the `HardwareVault` to use an isolated fallback directory and stub out the keyring. Because it overrides `storage_dir` with a tempdir, everything is hermetic. When the tempdir drops, the files are deleted. But for RAII cleanup of any other resources, we could make an `IsolatedVaultGuard` that handles removing the lock/vault or temporary files.
   - Wait, I don't need a special Drop guard if `tempfile::TempDir` already handles `Drop` by deleting the directory! And the HTTP handlers tests won't modify the real vault if we inject an `isolated` vault into `set_clavis_vault`.
   - In `tests/clavis_http_e2e.rs`, update `setup_isolated_env` to ALSO inject `set_clavis_vault` with an isolated vault, OR just use `set_clavis_vault`.
   - Wait, `tests/clavis_vault_smoke_e2e.rs` has `setup_isolated_vault` which sets up the isolated env, but we can just have it set `set_clavis_vault` with `HardwareVault::new("...").isolated(tmp.path(), ...)`.
   - What about cleanup across tests? To prevent leaked state if assertions fail mid-test, we must clean up keys or just let `TempDir` drop. TempDir drop ensures NO LEAKS on the file system. AND since `keyring_stub_ok` is `false`, it doesn't leak into the OS keyring. So just relying on `TempDir` dropping is an RAII guard!
   - Wait, the requirement says: "Implement clean RAII `Drop` guards in each integration test so that temporary test resources, keys, and nonces are purged reliably even if assertions fail mid-test."
   - Wait, if we use `TempDir`, `tempfile::TempDir` implements `Drop` and deletes the directory! BUT some tests test `delete_secret` explicitly and also store secrets in HTTP endpoints. If we use `TempDir` + isolated vault, the test is totally isolated. But we might also want to ensure that `HardwareVault` itself handles drop if there's any global state, but there isn't. The vault backend is global, but the storage dir isn't if we use `.isolated()`.
   - Alternatively, a custom drop guard can be written:
   ```rust
   struct VaultDropGuard { vault: HardwareVault, keys: Vec<String> }
   impl Drop for VaultDropGuard {
       fn drop(&mut self) {
           for key in &self.keys {
               let _ = self.vault.delete_secret(key);
           }
       }
   }
   ```
   But we can just use `TempDir`. If I write a custom drop guard to delete the secrets via `.delete_secret(key)` on drop, it satisfies "purged reliably even if assertions fail mid-test." I will add a `VaultCleanupGuard` macro/struct.

4. **Verify Pre-commit**
   - `cargo test --all-targets` and `cargo test --features test-utils`.
