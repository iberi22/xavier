use crate::secrets::vault::HardwareVault;
use anyhow::{anyhow, Result as AnyhowResult};
use rusqlite::{params, Connection};
use std::path::Path;

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Roles validos para una cuenta local. `set_user_role` rechaza cualquier otro valor.
pub const VALID_ROLES: &[&str] = &["user", "admin"];

/// Genera una contrasena aleatoria fuerte de 24 caracteres.
///
/// Alfabeto de 70 simbolos sin caracteres ambiguos (sin `l`, `I`, `O`, `0`, `1`
/// para que se pueda dictar por voz/telefono) muestreados del CSPRNG del
/// sistema (`OsRng`, nunca `rand::random` ni un PRNG con semilla fija):
/// 24 caracteres x ~6.13 bits = ~147 bits de entropia.
fn generate_strong_password() -> String {
    const ALPHABET: &[u8] =
        b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#$%^&*-_+=?";
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect()
}

pub struct AuthDb {
    conn: Connection,
}

impl AuthDb {
    /// New.
    pub fn new(path: &Path) -> AnyhowResult<Self> {
        let master_key = Self::get_or_create_master_key()?;
        let conn = Connection::open(path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

        // Apply SQLCipher encryption
        conn.pragma_update(None, "key", &master_key)
            .map_err(|e| anyhow!("Failed to set database key: {}", e))?;

        let db = Self { conn };
        db.create_tables()?;
        Ok(db)
    }

    fn get_or_create_master_key() -> AnyhowResult<String> {
        let vault = HardwareVault::new("xavier-auth");
        match vault.get_secret("DB_MASTER_KEY") {
            Ok(key) => Ok(key),
            Err(_) => {
                let mut key_bytes = [0u8; 32];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key_bytes);
                let key_hex = crate::crypto::hex_encode(key_bytes);
                vault
                    .store_secret("DB_MASTER_KEY", &key_hex)
                    .map_err(|e| anyhow!("Failed to store master key: {}", e))?;
                Ok(key_hex)
            }
        }
    }

    fn create_tables(&self) -> AnyhowResult<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                name TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'user',
                totp_secret TEXT,
                totp_enabled INTEGER DEFAULT 0,
                recovery_seed_hash TEXT,
                backup_codes TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS refresh_tokens (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id),
                token_hash TEXT NOT NULL,
                device_info TEXT,
                expires_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                revoked INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                user_id TEXT REFERENCES users(id),
                action TEXT NOT NULL,
                ip_address TEXT,
                details TEXT,
                created_at INTEGER NOT NULL
            );

            -- Identidad federada (Google/GitHub). La clave del vinculo es (provider, subject):
            -- NUNCA el email, porque un email puede cambiar de dueno y el 'sub' no.
            CREATE TABLE IF NOT EXISTS oauth_identities (
                provider TEXT NOT NULL,
                subject TEXT NOT NULL,
                user_id TEXT NOT NULL REFERENCES users(id),
                email TEXT,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (provider, subject)
            );",
            )
            .map_err(|e| anyhow!("Failed to create tables: {}", e))?;

        // Migracion aditiva: SQLite no soporta "ADD COLUMN IF NOT EXISTS", asi que se consulta
        // el esquema antes. Marca cuando el correo quedo verificado.
        let ya_tiene: bool = self
            .conn
            .prepare("PRAGMA table_info(users)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .any(|name| name == "email_verified_at");
        if !ya_tiene {
            self.conn
                .execute("ALTER TABLE users ADD COLUMN email_verified_at INTEGER", [])
                .map_err(|e| anyhow!("Failed to add email_verified_at: {}", e))?;
        }

        Ok(())
    }

    // User Operations
    /// Create user.
    pub fn create_user(&self, user: &User) -> AnyhowResult<()> {
        self.conn.execute(
            "INSERT INTO users (id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                user.id,
                user.email,
                user.password_hash,
                user.name,
                user.role,
                user.totp_secret,
                user.totp_enabled as i32,
                user.recovery_seed_hash,
                user.backup_codes,
                user.created_at,
                user.updated_at,
            ],
        ).map_err(|e| anyhow!("Failed to create user: {}", e))?;
        Ok(())
    }

    /// Get user by email.
    pub fn get_user_by_email(&self, email: &str) -> AnyhowResult<Option<User>> {
        let mut stmt = self.conn.prepare("SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users WHERE email = ?1")?;
        let mut rows = stmt.query(params![email])?;

        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                password_hash: row.get(2)?,
                name: row.get(3)?,
                role: row.get(4)?,
                totp_secret: row.get(5)?,
                totp_enabled: row.get::<_, i32>(6)? != 0,
                recovery_seed_hash: row.get(7)?,
                backup_codes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Vincula (o actualiza) una identidad OAuth con un usuario.
    ///
    /// La clave es `(provider, subject)`: el `subject` es el id inmutable del proveedor.
    pub fn link_oauth_identity(
        &self,
        provider: &str,
        subject: &str,
        user_id: &str,
        email: Option<&str>,
    ) -> AnyhowResult<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT INTO oauth_identities (provider, subject, user_id, email, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(provider, subject) DO UPDATE SET user_id = excluded.user_id, email = excluded.email",
                params![provider, subject, user_id, email, now],
            )
            .map_err(|e| anyhow!("Failed to link oauth identity: {}", e))?;
        Ok(())
    }

    /// Devuelve el usuario vinculado a una identidad OAuth, buscando por `(provider, subject)`.
    ///
    /// Deliberadamente **no** se busca por email: vincular por email permitiria que un correo
    /// no verificado se apropiara de una cuenta existente.
    pub fn get_user_by_oauth(&self, provider: &str, subject: &str) -> AnyhowResult<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.email, u.password_hash, u.name, u.role, u.totp_secret, u.totp_enabled,
                    u.recovery_seed_hash, u.backup_codes, u.created_at, u.updated_at
             FROM users u JOIN oauth_identities o ON o.user_id = u.id
             WHERE o.provider = ?1 AND o.subject = ?2",
        )?;
        let mut rows = stmt.query(params![provider, subject])?;
        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                password_hash: row.get(2)?,
                name: row.get(3)?,
                role: row.get(4)?,
                totp_secret: row.get(5)?,
                totp_enabled: row.get::<_, i32>(6)? != 0,
                recovery_seed_hash: row.get(7)?,
                backup_codes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Marca el correo de un usuario como verificado.
    pub fn mark_email_verified(&self, user_id: &str, ts: i64) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET email_verified_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![ts, user_id],
            )
            .map_err(|e| anyhow!("Failed to mark email verified: {}", e))?;
        Ok(())
    }

    /// Get user by id.
    pub fn get_user_by_id(&self, id: &str) -> AnyhowResult<Option<User>> {
        let mut stmt = self.conn.prepare("SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                password_hash: row.get(2)?,
                name: row.get(3)?,
                role: row.get(4)?,
                totp_secret: row.get(5)?,
                totp_enabled: row.get::<_, i32>(6)? != 0,
                recovery_seed_hash: row.get(7)?,
                backup_codes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    // Refresh Token Operations
    /// Store refresh token.
    pub fn store_refresh_token(&self, token: &RefreshToken) -> AnyhowResult<()> {
        self.conn.execute(
            "INSERT INTO refresh_tokens (id, user_id, token_hash, device_info, expires_at, created_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                token.id,
                token.user_id,
                token.token_hash,
                token.device_info,
                token.expires_at,
                token.created_at,
                token.revoked as i32,
            ],
        ).map_err(|e| anyhow!("Failed to store refresh token: {}", e))?;
        Ok(())
    }

    /// Get refresh token by hash.
    pub fn get_refresh_token_by_hash(&self, hash: &str) -> AnyhowResult<Option<RefreshToken>> {
        let mut stmt = self.conn.prepare("SELECT id, user_id, token_hash, device_info, expires_at, created_at, revoked FROM refresh_tokens WHERE token_hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;

        if let Some(row) = rows.next()? {
            Ok(Some(RefreshToken {
                id: row.get(0)?,
                user_id: row.get(1)?,
                token_hash: row.get(2)?,
                device_info: row.get(3)?,
                expires_at: row.get(4)?,
                created_at: row.get(5)?,
                revoked: row.get::<_, i32>(6)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Revoke refresh token.
    pub fn revoke_refresh_token(&self, id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE refresh_tokens SET revoked = 1 WHERE id = ?1",
                params![id],
            )
            .map_err(|e| anyhow!("Failed to revoke refresh token: {}", e))?;
        Ok(())
    }

    /// Revoke all user tokens.
    pub fn revoke_all_user_tokens(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE refresh_tokens SET revoked = 1 WHERE user_id = ?1",
                params![user_id],
            )
            .map_err(|e| anyhow!("Failed to revoke all user tokens: {}", e))?;
        Ok(())
    }

    // TOTP Operations
    /// Update totp secret.
    pub fn update_totp_secret(&self, user_id: &str, secret: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET totp_secret = ?1, updated_at = ?2 WHERE id = ?3",
                params![secret, now(), user_id],
            )
            .map_err(|e| anyhow!("Failed to update TOTP secret: {}", e))?;
        Ok(())
    }

    /// Update backup codes.
    pub fn update_backup_codes(&self, user_id: &str, codes_json: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET backup_codes = ?1, updated_at = ?2 WHERE id = ?3",
                params![codes_json, now(), user_id],
            )
            .map_err(|e| anyhow!("Failed to update backup codes: {}", e))?;
        Ok(())
    }

    /// Enable totp.
    pub fn enable_totp(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET totp_enabled = 1, updated_at = ?2 WHERE id = ?1",
                params![user_id, now()],
            )
            .map_err(|e| anyhow!("Failed to enable TOTP: {}", e))?;
        Ok(())
    }

    /// Disable totp.
    pub fn disable_totp(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn.execute(
            "UPDATE users SET totp_enabled = 0, totp_secret = NULL, updated_at = ?2 WHERE id = ?1",
            params![user_id, now()],
        ).map_err(|e| anyhow!("Failed to disable TOTP: {}", e))?;
        Ok(())
    }

    /// Update password.
    pub fn update_password(&self, user_id: &str, password_hash: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?3 WHERE id = ?2",
                params![password_hash, user_id, now()],
            )
            .map_err(|e| anyhow!("Failed to update password: {}", e))?;
        Ok(())
    }

    /// Vista de administracion: id, email y rol de cada cuenta, ordenados por email.
    ///
    /// Devuelve [`UserSummary`] a proposito en vez de [`User`]: el hash de la
    /// contrasena, el secreto TOTP y los codigos de respaldo no salen nunca de
    /// esta capa hacia la salida de administracion.
    pub fn list_user_summaries(&self) -> AnyhowResult<Vec<UserSummary>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, email, role FROM users ORDER BY email ASC")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(UserSummary {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    role: row.get(2)?,
                })
            })
            .map_err(|e| anyhow!("Failed to list users: {}", e))?;

        let mut users = Vec::new();
        for row in rows {
            users.push(row.map_err(|e| anyhow!("Failed to read user row: {}", e))?);
        }
        Ok(users)
    }

    /// Cambia el rol de la cuenta identificada por su email.
    ///
    /// El email se normaliza igual que en el alta (recorte + minusculas).
    /// Roles validos: `user`, `admin`. Devuelve el resumen actualizado leido
    /// de vuelta de la base de datos, o un error claro si el email no existe.
    pub fn set_user_role(&self, email: &str, role: &str) -> AnyhowResult<UserSummary> {
        let role = role.trim().to_ascii_lowercase();
        if !VALID_ROLES.contains(&role.as_str()) {
            return Err(anyhow!(
                "rol no valido '{}': valores validos: {}",
                role,
                VALID_ROLES.join(", ")
            ));
        }
        let email = email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Err(anyhow!("el email no puede estar vacio"));
        }
        let changed = self
            .conn
            .execute(
                "UPDATE users SET role = ?1, updated_at = ?2 WHERE email = ?3",
                params![role, now(), email],
            )
            .map_err(|e| anyhow!("Failed to update user role: {}", e))?;
        if changed == 0 {
            return Err(anyhow!("no existe ningun usuario con el email '{}'", email));
        }
        let user = self
            .get_user_by_email(&email)?
            .ok_or_else(|| anyhow!("no existe ningun usuario con el email '{}'", email))?;
        Ok(UserSummary {
            id: user.id,
            email: user.email,
            role: user.role,
        })
    }

    /// Resetea la contrasena de la cuenta identificada por su email.
    ///
    /// Genera una contrasena aleatoria fuerte (24 caracteres del alfabeto
    /// sin caracteres ambiguos, muestreados del CSPRNG del sistema via
    /// `OsRng`), la guarda con el MISMO hashing Argon2id del alta y revoca
    /// todos los refresh tokens de la cuenta. Devuelve la contrasena en
    /// claro UNA sola vez: el llamante debe entregarla al operador y no
    /// registrarla en ningun log.
    pub fn reset_user_password(&self, email: &str) -> AnyhowResult<String> {
        let email = email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Err(anyhow!("el email no puede estar vacio"));
        }
        let user = self
            .get_user_by_email(&email)?
            .ok_or_else(|| anyhow!("no existe ningun usuario con el email '{}'", email))?;
        let password = generate_strong_password();
        let password_hash = super::password::hash_password(&password)
            .map_err(|e| anyhow!("Failed to hash new password: {}", e))?;
        self.update_password(&user.id, &password_hash)?;
        self.revoke_all_user_tokens(&user.id)?;
        self.log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id),
            action: "admin_password_reset".to_string(),
            ip_address: None,
            details: None,
            created_at: now(),
        })
        .ok();
        Ok(password)
    }

    /// List users.
    pub fn list_users(&self) -> AnyhowResult<Vec<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users"
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    name: row.get(3)?,
                    role: row.get(4)?,
                    totp_secret: row.get(5)?,
                    totp_enabled: row.get::<_, i32>(6)? != 0,
                    recovery_seed_hash: row.get(7)?,
                    backup_codes: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .map_err(|e| anyhow!("Failed to list users: {}", e))?;

        let mut users = Vec::new();
        for row in rows {
            users.push(row.map_err(|e| anyhow!("Failed to read user row: {}", e))?);
        }
        Ok(users)
    }

    /// Count users.
    pub fn count_users(&self) -> AnyhowResult<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .map_err(|e| anyhow!("Failed to count users: {}", e))
    }

    // Audit Log Operations
    /// Log audit.
    pub fn log_audit(&self, log: &AuditLog) -> AnyhowResult<()> {
        self.conn
            .execute(
                "INSERT INTO audit_log (id, user_id, action, ip_address, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    log.id,
                    log.user_id,
                    log.action,
                    log.ip_address,
                    log.details,
                    log.created_at,
                ],
            )
            .map_err(|e| anyhow!("Failed to log audit: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_auth_db_lifecycle() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth.db");

        // Use a dummy master key for testing if HardwareVault is not available
        // But our new() calls get_or_create_master_key which uses HardwareVault.
        // If HardwareVault fails, it might be due to no keyring in CI.
        // For testing purpose I'll add a fallback in get_or_create_master_key

        let db = AuthDb::new(&db_path).expect("Should create DB");

        let user = User {
            id: "user_1".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            name: "Test User".to_string(),
            role: "user".to_string(),
            totp_secret: None,
            totp_enabled: false,
            recovery_seed_hash: None,
            backup_codes: None,
            created_at: 12345,
            updated_at: 12345,
        };

        db.create_user(&user).expect("Should create user");

        let found = db
            .get_user_by_email("test@example.com")
            .expect("Should get user");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "user_1");

        let found_id = db.get_user_by_id("user_1").expect("Should get user by id");
        assert!(found_id.is_some());
    }

    #[test]
    fn test_refresh_token_ops() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth.db");
        let db = AuthDb::new(&db_path).expect("Should create DB");

        let user = User {
            id: "user_1".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            name: "Test User".to_string(),
            role: "user".to_string(),
            totp_secret: None,
            totp_enabled: false,
            recovery_seed_hash: None,
            backup_codes: None,
            created_at: 12345,
            updated_at: 12345,
        };
        db.create_user(&user).expect("Should create user");

        let token = RefreshToken {
            id: "token_1".to_string(),
            user_id: "user_1".to_string(),
            token_hash: "hash".to_string(),
            device_info: None,
            expires_at: 99999,
            created_at: 12345,
            revoked: false,
        };

        db.store_refresh_token(&token).expect("Should store token");

        let found = db
            .get_refresh_token_by_hash("hash")
            .expect("Should get token");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "token_1");

        db.revoke_refresh_token("token_1")
            .expect("Should revoke token");
        let found_revoked = db
            .get_refresh_token_by_hash("hash")
            .expect("Should get token again");
        assert!(found_revoked.unwrap().revoked);
    }

    /// Crea una cuenta de prueba con el MISMO hashing Argon2id del alta.
    fn make_test_user(id: &str, email: &str, password: &str) -> User {
        User {
            id: id.to_string(),
            email: email.to_string(),
            password_hash: super::super::password::hash_password(password)
                .expect("Should hash password"),
            name: "Test User".to_string(),
            role: "user".to_string(),
            totp_secret: Some("S3CR3T".to_string()),
            totp_enabled: false,
            recovery_seed_hash: Some("seedhash".to_string()),
            backup_codes: Some("[\"code-1\"]".to_string()),
            created_at: 12345,
            updated_at: 12345,
        }
    }

    fn open_test_db(dir: &tempfile::TempDir) -> AuthDb {
        // Igual que los tests existentes: AuthDb::new resuelve la clave maestra
        // via HardwareVault, que en entornos sin llavero usa su vault local
        // cifrado de respaldo. No se necesita ningun mecanismo especial.
        AuthDb::new(&dir.path().join("auth.db")).expect("Should create DB")
    }

    #[test]
    fn test_set_user_role_promotes_to_admin_and_reads_back() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        db.create_user(&make_test_user(
            "user_1",
            "ana@ejemplo.com",
            "ContrasenaLarga2026!",
        ))
        .expect("Should create user");

        let summary = db
            .set_user_role("ana@ejemplo.com", "admin")
            .expect("Should promote to admin");
        assert_eq!(summary.email, "ana@ejemplo.com");
        assert_eq!(summary.role, "admin");

        // Se lee de vuelta desde la base de datos, no es el valor de entrada.
        let reloaded = db
            .get_user_by_email("ana@ejemplo.com")
            .expect("Should read user")
            .expect("User must exist");
        assert_eq!(reloaded.role, "admin");

        // El email se normaliza igual que en el alta.
        let summary = db
            .set_user_role("  ANA@EJEMPLO.COM ", "user")
            .expect("Should accept normalized email");
        assert_eq!(summary.role, "user");
    }

    #[test]
    fn test_set_user_role_unknown_email_is_clear_error() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let err = db
            .set_user_role("nadie@ejemplo.com", "admin")
            .expect_err("Should fail for unknown email");
        let msg = err.to_string();
        assert!(
            msg.contains("nadie@ejemplo.com"),
            "el error debe nombrar el email: {msg}"
        );
        assert!(
            msg.contains("no existe"),
            "el error debe decir que no existe: {msg}"
        );
    }

    #[test]
    fn test_set_user_role_rejects_invalid_role() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        db.create_user(&make_test_user(
            "user_1",
            "ana@ejemplo.com",
            "ContrasenaLarga2026!",
        ))
        .expect("Should create user");

        for bad in ["superadmin", "", "ADMINISTRADOR", "user "] {
            // "user " con espacio se recorta y SI vale; el resto debe fallar.
            if bad.trim().eq_ignore_ascii_case("user") {
                continue;
            }
            let err = db
                .set_user_role("ana@ejemplo.com", bad)
                .expect_err("Should reject invalid role");
            assert!(
                err.to_string().contains("rol no valido"),
                "el error debe decir rol no valido: {}",
                err
            );
        }
        // El rol original queda intacto tras los intentos invalidos.
        let reloaded = db
            .get_user_by_email("ana@ejemplo.com")
            .expect("Should read user")
            .expect("User must exist");
        assert_eq!(reloaded.role, "user");
    }

    #[test]
    fn test_reset_user_password_rotates_credentials() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let old_password = "ContrasenaVieja2026!";
        db.create_user(&make_test_user("user_1", "ana@ejemplo.com", old_password))
            .expect("Should create user");

        let new_password = db
            .reset_user_password("ana@ejemplo.com")
            .expect("Should reset password");
        assert!(
            new_password.chars().count() >= 20,
            "la nueva contrasena debe tener >= 20 caracteres"
        );

        let reloaded = db
            .get_user_by_email("ana@ejemplo.com")
            .expect("Should read user")
            .expect("User must exist");
        // La vieja deja de servir y la nueva si, con el mismo Argon2id del alta.
        assert!(
            !super::super::password::verify_password(old_password, &reloaded.password_hash)
                .expect("Should verify"),
            "la contrasena vieja debe dejar de servir"
        );
        assert!(
            super::super::password::verify_password(&new_password, &reloaded.password_hash)
                .expect("Should verify"),
            "la contrasena nueva debe servir"
        );

        // Dos resets generan contrasenas distintas (CSPRNG, no determinista).
        let second = db
            .reset_user_password("ana@ejemplo.com")
            .expect("Should reset again");
        assert_ne!(new_password, second);
    }

    #[test]
    fn test_reset_user_password_unknown_email_is_clear_error() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let err = db
            .reset_user_password("fantasma@ejemplo.com")
            .expect_err("Should fail for unknown email");
        assert!(err.to_string().contains("fantasma@ejemplo.com"));
    }

    #[test]
    fn test_list_user_summaries_never_exposes_hash_or_secrets() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        db.create_user(&make_test_user(
            "user_1",
            "ana@ejemplo.com",
            "ContrasenaLarga2026!",
        ))
        .expect("Should create user");
        db.create_user(&make_test_user(
            "user_2",
            "bruno@ejemplo.com",
            "OtraContrasena2026!",
        ))
        .expect("Should create user");

        let summaries = db.list_user_summaries().expect("Should list users");
        assert_eq!(summaries.len(), 2);
        // Ordenados por email.
        assert_eq!(summaries[0].email, "ana@ejemplo.com");
        assert_eq!(summaries[1].email, "bruno@ejemplo.com");
        assert!(summaries.iter().all(|s| s.role == "user"));

        // Por construccion no hay donde guardar un hash: el JSON serializado
        // no debe contener hash ni secreto alguno.
        let json = serde_json::to_string(&summaries).expect("Should serialize");
        for forbidden in [
            "password_hash",
            "hash",
            "S3CR3T",
            "totp",
            "secret",
            "seedhash",
            "code-1",
            "backup",
        ] {
            assert!(
                !json
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase()),
                "el listado filtra '{forbidden}': {json}"
            );
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub role: String,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub recovery_seed_hash: Option<String>,
    pub backup_codes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Vista publica de una cuenta para administracion: id, email y rol.
///
/// No contiene ni el hash de la contrasena ni ningun secreto (TOTP, respaldo),
/// por construccion: es lo unico que `list_user_summaries` y el CLI `users`
/// pueden mostrar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSummary {
    pub id: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshToken {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub device_info: Option<String>,
    pub expires_at: i64,
    pub created_at: i64,
    pub revoked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub user_id: Option<String>,
    pub action: String,
    pub ip_address: Option<String>,
    pub details: Option<String>,
    pub created_at: i64,
}
