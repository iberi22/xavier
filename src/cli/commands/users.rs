//! CLI user account administration commands
//!
//! Handles the `xavier users` subcommand: listing local accounts, changing an
//! account role by email, resetting an account password by email, creating
//! the first (or a new) local account interactively, and enrolling/disabling
//! TOTP 2FA — all from the terminal, without the HTTP server running.
//!
//! These commands open `<estado>/.xavier/auth.db` directly — the same database
//! the HTTP server uses (`XAVIER_STATE_DIR`, else `HOME`, else `.`) — so they
//! work without the server running. They are meant for the node operator.
//!
//! `create`, `totp-enroll` and `totp-disable` deliberately REUSE the auth2
//! logic (`xavier::auth2::new_account`, `generate_totp_enrollment`,
//! `generate_backup_codes_with_hashes`, `verify_totp_code`,
//! `consume_backup_code`) instead of duplicating validation, hashing or TOTP
//! verification: the HTTP `/auth/*` handlers and this CLI produce byte-for-byte
//! identical accounts and 2FA secrets.

use crate::cli::commands::enums::UsersCommand;
use anyhow::{anyhow, bail, Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use xavier::auth2::db::{AuditLog, AuthDb, User, UserSummary, VALID_ROLES};
use xavier::auth2::{self, NewAccount, TotpEnrollment};

/// Resolve the auth database path exactly like the HTTP server does
/// (`src/cli/server.rs`): `XAVIER_STATE_DIR`, else `HOME`/`USERPROFILE`,
/// else the current directory, plus `.xavier/auth.db`.
fn auth_db_path() -> PathBuf {
    let state_dir_str = std::env::var("XAVIER_STATE_DIR")
        .or_else(|_| std::env::var("HOME"))
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(&state_dir_str)
        .join(".xavier")
        .join("auth.db")
}

fn open_auth_db() -> Result<AuthDb> {
    let path = auth_db_path();
    AuthDb::new(&path)
        .with_context(|| format!("no se pudo abrir la base de cuentas {}", path.display()))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Refuses to print secrets (recovery phrase, TOTP secret/QR, backup codes) to a
/// stdout that is not an interactive terminal (piped, redirected, captured by
/// another process), unless the operator explicitly opts in.
fn ensure_secret_output_allowed(explicit_override: bool) -> Result<()> {
    if std::io::stdout().is_terminal() || explicit_override {
        return Ok(());
    }
    bail!(
        "stdout no es una terminal interactiva: por seguridad no se imprime aqui la frase de \
         recuperacion ni los codigos de respaldo/TOTP. Ejecuta el comando en una terminal real, \
         o pasa --i-understand-output-is-not-a-tty si sabes lo que haces y aceptas el riesgo."
    );
}

/// Dispatch a [`UsersCommand`] to the appropriate handler.
pub async fn handle_users_command(cmd: UsersCommand) -> Result<()> {
    match cmd {
        UsersCommand::List { json } => list_users(json).await,
        UsersCommand::SetRole { email, role, json } => set_role(&email, &role, json).await,
        UsersCommand::ResetPassword { email, json } => reset_password(&email, json).await,
        UsersCommand::Create {
            email,
            role,
            name,
            i_understand_output_is_not_a_tty,
        } => create_user_cmd(email, role, name, i_understand_output_is_not_a_tty).await,
        UsersCommand::TotpEnroll {
            email,
            i_understand_output_is_not_a_tty,
        } => totp_enroll_cmd(email, i_understand_output_is_not_a_tty).await,
        UsersCommand::TotpDisable { email, code } => totp_disable_cmd(email, code).await,
    }
}

async fn list_users(json: bool) -> Result<()> {
    let db = open_auth_db()?;
    let users: Vec<UserSummary> = db.list_user_summaries()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&users)?);
        return Ok(());
    }
    if users.is_empty() {
        println!("No hay cuentas registradas.");
        return Ok(());
    }
    println!("{:<38} {:<40} {:<10}", "ID", "EMAIL", "ROL");
    for u in &users {
        println!("{:<38} {:<40} {:<10}", u.id, u.email, u.role);
    }
    println!("\n{} cuenta(s).", users.len());
    Ok(())
}

async fn set_role(email: &str, role: &str, json: bool) -> Result<()> {
    let db = open_auth_db()?;
    let updated = db.set_user_role(email, role)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&updated)?);
        return Ok(());
    }
    println!(
        "Rol actualizado: {} ahora es '{}'.",
        updated.email, updated.role
    );
    Ok(())
}

async fn reset_password(email: &str, json: bool) -> Result<()> {
    let db = open_auth_db()?;
    // La contrasena en claro existe solo en esta variable: se imprime UNA vez
    // y jamas se registra en logs.
    let password = db.reset_user_password(email)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "email": email.trim().to_ascii_lowercase(),
                "password": password,
            }))?
        );
    } else {
        println!(
            "Nueva contrasena para {}:",
            email.trim().to_ascii_lowercase()
        );
        println!("{}", password);
    }
    eprintln!(
        "AVISO: esta contrasena no se vuelve a mostrar. Entregala a su dueno y pierde este texto."
    );
    Ok(())
}

// ── users create ────────────────────────────────────────────────────────────

/// Persists a brand-new account: reuses [`auth2::new_account`] (same validation,
/// Argon2id hashing and 24-word Spanish BIP39 recovery phrase as
/// `POST /auth/register`), then writes it to `auth.db` and logs the audit
/// entry. Pure DB logic, no interactive I/O — kept separate so it is directly
/// unit-testable.
fn create_user_in_db(
    db: &AuthDb,
    email: &str,
    password: &str,
    name: &str,
    role: &str,
) -> Result<NewAccount> {
    let account =
        auth2::new_account(email, password, name, role).map_err(|(_, msg)| anyhow!(msg))?;
    db.create_user(&account.user)
        .context("no se pudo crear la cuenta en auth.db")?;
    db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(account.user.id.clone()),
        action: "cli_register".to_string(),
        ip_address: None,
        details: None,
        created_at: now_secs(),
    })
    .ok();
    Ok(account)
}

async fn create_user_cmd(
    email: String,
    role: String,
    name: Option<String>,
    tty_override: bool,
) -> Result<()> {
    // Checked BEFORE any prompt: no point typing a password just to be told at
    // the end that the recovery phrase cannot be shown.
    ensure_secret_output_allowed(tty_override)?;

    let email_norm = email.trim().to_ascii_lowercase();
    let role_norm = role.trim().to_ascii_lowercase();
    if !VALID_ROLES.contains(&role_norm.as_str()) {
        bail!(
            "rol no valido '{}': valores validos: {}",
            role_norm,
            VALID_ROLES.join(", ")
        );
    }
    let name = name.unwrap_or_else(|| email_norm.clone());

    let db = open_auth_db()?;
    if db.get_user_by_email(&email_norm)?.is_some() {
        bail!(
            "ya existe una cuenta con el correo '{}'; usa 'xavier users reset-password' si perdiste el acceso",
            email_norm
        );
    }

    println!("Creando cuenta para {} (rol: {})", email_norm, role_norm);
    let password = loop {
        let pw: String = dialoguer::Password::new()
            .with_prompt("Contrasena nueva (minimo 12 caracteres)")
            .interact()
            .context("no se pudo leer la contrasena")?;
        let confirm: String = dialoguer::Password::new()
            .with_prompt("Confirma la contrasena")
            .interact()
            .context("no se pudo leer la confirmacion")?;
        if pw != confirm {
            eprintln!("Las contrasenas no coinciden. Intenta de nuevo.");
            continue;
        }
        if let Err((_, msg)) = auth2::validate_registration(&email_norm, &pw, &name) {
            eprintln!("Contrasena o datos invalidos: {msg}");
            continue;
        }
        break pw;
    };

    let account = create_user_in_db(&db, &email_norm, &password, &name, &role_norm)?;

    println!();
    println!(
        "Cuenta creada: {} (id {}, rol {})",
        account.user.email, account.user.id, account.user.role
    );
    println!();
    println!("=== FRASE DE RECUPERACION — ANOTALA AHORA, SE MUESTRA UNA SOLA VEZ ===");
    println!("{}", account.seed_phrase);
    println!("========================================================================");
    eprintln!();
    eprintln!(
        "AVISO: esta frase permite recuperar la cuenta si pierdes la contrasena. Guardala \
         offline (papel, gestor de contrasenas fisico). Xavier NO la vuelve a mostrar ni la \
         guarda en claro."
    );

    loop {
        let ack: String = dialoguer::Input::new()
            .with_prompt("Escribe 'guardada' para confirmar que copiaste la frase de recuperacion")
            .interact_text()
            .context("no se pudo leer la confirmacion")?;
        if ack.trim().eq_ignore_ascii_case("guardada") {
            break;
        }
        eprintln!("Debes escribir exactamente 'guardada' para continuar.");
    }

    Ok(())
}

// ── users totp-enroll / totp-disable ────────────────────────────────────────

/// Generates a TOTP secret for `email` and persists it as "pending" (stored,
/// but `totp_enabled` stays false until [`confirm_totp_enrollment`] validates a
/// code) — mirrors `POST /auth/2fa/setup`. Fails if the account does not exist
/// or already has 2FA enabled.
fn prepare_totp_enrollment(db: &AuthDb, email: &str) -> Result<(User, TotpEnrollment)> {
    let user = db
        .get_user_by_email(email)?
        .ok_or_else(|| anyhow!("no existe ninguna cuenta con el correo '{}'", email))?;
    if user.totp_enabled {
        bail!(
            "la cuenta '{}' ya tiene 2FA activo; usa 'xavier users totp-disable' primero",
            user.email
        );
    }
    let enrollment = auth2::generate_totp_enrollment(&user.email)?;
    db.update_totp_secret(&user.id, &enrollment.secret_base32)
        .context("no se pudo guardar el secreto TOTP pendiente")?;
    db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(user.id.clone()),
        action: "cli_2fa_setup_initiated".to_string(),
        ip_address: None,
        details: None,
        created_at: now_secs(),
    })
    .ok();
    Ok((user, enrollment))
}

/// Verifies `code` against the pending secret (same window/skew as
/// `POST /auth/2fa/verify`: SHA1, 6 digits, 30s step, skew ±1) and, only if it
/// matches, generates + persists 10 backup codes and flips `totp_enabled`.
/// Returns the plaintext backup codes on success.
fn confirm_totp_enrollment(
    db: &AuthDb,
    user: &User,
    secret_base32: &str,
    code: &str,
) -> Result<Vec<String>> {
    let ok = auth2::verify_totp_code(secret_base32, &user.email, code)?;
    if !ok {
        bail!("codigo invalido o expirado");
    }
    let (backup_codes, hashed_codes) = auth2::generate_backup_codes_with_hashes();
    db.update_backup_codes(&user.id, &serde_json::to_string(&hashed_codes)?)
        .context("no se pudieron guardar los codigos de respaldo")?;
    db.enable_totp(&user.id).context("no se pudo activar 2FA")?;
    db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(user.id.clone()),
        action: "cli_2fa_enabled".to_string(),
        ip_address: None,
        details: None,
        created_at: now_secs(),
    })
    .ok();
    Ok(backup_codes)
}

async fn totp_enroll_cmd(email: String, tty_override: bool) -> Result<()> {
    ensure_secret_output_allowed(tty_override)?;

    let email_norm = email.trim().to_ascii_lowercase();
    let db = open_auth_db()?;
    let (user, enrollment) = prepare_totp_enrollment(&db, &email_norm)?;

    println!(
        "Escanea este QR con Google Authenticator o Microsoft Authenticator (cuenta Xavier:{}):",
        user.email
    );
    println!();
    println!("{}", enrollment.qr_unicode);
    println!("Si no puedes escanear, ingresa esta clave manualmente:");
    println!("  {}", enrollment.secret_base32);
    println!();

    const MAX_ATTEMPTS: u32 = 5;
    let mut attempts = 0u32;
    let backup_codes = loop {
        attempts += 1;
        let code: String = dialoguer::Input::new()
            .with_prompt("Codigo de 6 digitos de la app")
            .interact_text()
            .context("no se pudo leer el codigo")?;
        match confirm_totp_enrollment(&db, &user, &enrollment.secret_base32, code.trim()) {
            Ok(codes) => break codes,
            Err(e) => {
                if attempts >= MAX_ATTEMPTS {
                    return Err(e).context(
                        "demasiados intentos fallidos; vuelve a correr 'totp-enroll' para \
                         generar un secreto nuevo",
                    );
                }
                eprintln!("Codigo invalido ({attempts}/{MAX_ATTEMPTS}): {e}. Intenta de nuevo.");
            }
        }
    };

    println!();
    println!("2FA activado para {}.", user.email);
    println!();
    println!("=== CODIGOS DE RESPALDO — ANOTALOS AHORA, SE MUESTRAN UNA SOLA VEZ ===");
    for code in &backup_codes {
        println!("  {code}");
    }
    println!("=========================================================================");
    eprintln!();
    eprintln!(
        "AVISO: cada codigo de respaldo sirve una sola vez, para cuando pierdas el dispositivo \
         con la app. Guardalos offline; Xavier no los vuelve a mostrar."
    );

    Ok(())
}

/// Disables 2FA for `email` if `supplied` is either a valid current TOTP code
/// or an unused backup code (which is consumed on use). Returns which method
/// succeeded, or an error if neither matches.
fn disable_totp_with_code(db: &AuthDb, email: &str, supplied: &str) -> Result<&'static str> {
    let user = db
        .get_user_by_email(email)?
        .ok_or_else(|| anyhow!("no existe ninguna cuenta con el correo '{}'", email))?;
    if !user.totp_enabled {
        bail!("la cuenta '{}' no tiene 2FA activo", user.email);
    }
    let secret = user.totp_secret.as_deref().ok_or_else(|| {
        anyhow!(
            "la cuenta '{}' no tiene un secreto TOTP guardado",
            user.email
        )
    })?;

    if auth2::verify_totp_code(secret, &user.email, supplied).unwrap_or(false) {
        db.disable_totp(&user.id)
            .context("no se pudo desactivar 2FA")?;
        db.log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id.clone()),
            action: "cli_2fa_disabled_totp".to_string(),
            ip_address: None,
            details: None,
            created_at: now_secs(),
        })
        .ok();
        return Ok("totp");
    }

    if let Some(codes_json) = user.backup_codes.as_deref() {
        if let Some(remaining) = auth2::consume_backup_code(codes_json, supplied) {
            db.update_backup_codes(&user.id, &remaining)
                .context("no se pudo actualizar los codigos de respaldo")?;
            db.disable_totp(&user.id)
                .context("no se pudo desactivar 2FA")?;
            db.log_audit(&AuditLog {
                id: ulid::Ulid::new().to_string(),
                user_id: Some(user.id.clone()),
                action: "cli_2fa_disabled_backup_code".to_string(),
                ip_address: None,
                details: None,
                created_at: now_secs(),
            })
            .ok();
            return Ok("backup_code");
        }
    }

    bail!(
        "codigo invalido: no coincide con el TOTP actual ni con ningun codigo de respaldo sin usar"
    )
}

async fn totp_disable_cmd(email: String, code: Option<String>) -> Result<()> {
    let email_norm = email.trim().to_ascii_lowercase();
    let db = open_auth_db()?;

    let supplied = match code {
        Some(c) => c,
        None => dialoguer::Input::new()
            .with_prompt("Codigo TOTP de 6 digitos o codigo de respaldo")
            .interact_text()
            .context("no se pudo leer el codigo")?,
    };

    match disable_totp_with_code(&db, &email_norm, supplied.trim())? {
        "totp" => println!("2FA desactivado para {} (codigo TOTP valido).", email_norm),
        "backup_code" => {
            println!("2FA desactivado para {} (codigo de respaldo).", email_norm);
            eprintln!("AVISO: ese codigo de respaldo ya fue consumido y no sirve de nuevo.");
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use totp_rs::{Algorithm, Builder, Secret};

    const OK_PASS: &str = "ContrasenaLarga2026!";

    fn open_test_db(dir: &tempfile::TempDir) -> AuthDb {
        AuthDb::new(&dir.path().join("auth.db")).expect("should create auth.db")
    }

    /// Builds the exact same TOTP validator as `auth2::build_totp`, to compute
    /// codes deterministically for a fixed clock in tests.
    fn test_totp(secret_base32: &str, email: &str) -> totp_rs::Totp {
        let secret = Secret::try_from_base32(secret_base32).expect("valid base32 secret");
        Builder::new()
            .with_algorithm(Algorithm::SHA1)
            .with_digits(6)
            .with_skew(1)
            .with_step_duration(30)
            .with_secret(secret)
            .with_account_name(email.to_string())
            .with_issuer(Some("Xavier".to_string()))
            .build()
            .expect("should build totp")
    }

    // ── users create ────────────────────────────────────────────────────────

    #[test]
    fn create_user_hashes_password_and_sets_role() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);

        let account = create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "admin")
            .expect("should create user");

        assert_eq!(account.user.role, "admin");
        assert_eq!(account.user.email, "ana@ejemplo.com");
        // The recovery phrase returned in-memory is 24 Spanish words, never stored in clear.
        assert_eq!(account.seed_phrase.split_whitespace().count(), 24);

        let reloaded = db
            .get_user_by_email("ana@ejemplo.com")
            .expect("should read user")
            .expect("user must exist");
        assert_eq!(reloaded.role, "admin");
        // The hash must verify the original password (same Argon2id as /auth/register).
        assert!(
            xavier::auth2::password::verify_password(OK_PASS, &reloaded.password_hash)
                .expect("should verify")
        );
        assert!(!xavier::auth2::password::verify_password(
            "wrong-password",
            &reloaded.password_hash
        )
        .expect("should verify"));
        // The recovery phrase hash must verify against the plaintext phrase returned once.
        let seed_hash = reloaded.recovery_seed_hash.expect("seed hash must be set");
        assert!(
            xavier::security::recovery::RecoverySystem::verify_seed_phrase(
                &account.seed_phrase,
                &seed_hash
            )
        );
    }

    #[test]
    fn create_user_rejects_weak_password_via_shared_validation() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let err = create_user_in_db(&db, "ana@ejemplo.com", "corta1", "Ana", "user")
            .expect_err("should reject weak password");
        assert!(err.to_string().contains("contrasena"));
        assert!(db.get_user_by_email("ana@ejemplo.com").unwrap().is_none());
    }

    // ── users totp-enroll ───────────────────────────────────────────────────

    #[test]
    fn totp_enroll_roundtrip_activates_2fa_and_returns_backup_codes() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "user").unwrap();

        let (user, enrollment) = prepare_totp_enrollment(&db, "ana@ejemplo.com").unwrap();
        assert!(!user.totp_enabled);

        let totp = test_totp(&enrollment.secret_base32, &user.email);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = totp.generate(now).to_string();

        let backup_codes = confirm_totp_enrollment(&db, &user, &enrollment.secret_base32, &code)
            .expect("valid code must confirm enrollment");
        assert_eq!(backup_codes.len(), 10);
        // Backup codes are unique 8-digit strings.
        let unique: std::collections::HashSet<_> = backup_codes.iter().collect();
        assert_eq!(unique.len(), 10);

        let reloaded = db.get_user_by_email("ana@ejemplo.com").unwrap().unwrap();
        assert!(reloaded.totp_enabled);
        assert_eq!(
            reloaded.totp_secret.as_deref(),
            Some(enrollment.secret_base32.as_str())
        );
    }

    #[test]
    fn totp_enroll_accepts_previous_and_next_window_skew() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "user").unwrap();
        let (user, enrollment) = prepare_totp_enrollment(&db, "ana@ejemplo.com").unwrap();
        let totp = test_totp(&enrollment.secret_base32, &user.email);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // One step (30s) in the past must still verify (skew = ±1).
        let past_code = totp.generate(now.saturating_sub(30)).to_string();
        assert!(xavier::auth2::verify_totp_code(
            &enrollment.secret_base32,
            &user.email,
            &past_code
        )
        .unwrap());

        // One step (30s) in the future must still verify.
        let future_code = totp.generate(now + 30).to_string();
        assert!(xavier::auth2::verify_totp_code(
            &enrollment.secret_base32,
            &user.email,
            &future_code
        )
        .unwrap());

        // Three steps away (90s) must NOT verify: outside the ±1 window.
        let far_code = totp.generate(now + 90).to_string();
        assert!(!xavier::auth2::verify_totp_code(
            &enrollment.secret_base32,
            &user.email,
            &far_code
        )
        .unwrap());
    }

    #[test]
    fn totp_enroll_rejects_invalid_code_without_activating() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "user").unwrap();
        let (user, enrollment) = prepare_totp_enrollment(&db, "ana@ejemplo.com").unwrap();

        let err = confirm_totp_enrollment(&db, &user, &enrollment.secret_base32, "000000")
            .expect_err("bogus code must be rejected");
        assert!(err.to_string().contains("invalido"));

        let reloaded = db.get_user_by_email("ana@ejemplo.com").unwrap().unwrap();
        assert!(!reloaded.totp_enabled);
    }

    #[test]
    fn totp_enroll_refuses_when_already_enabled() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "user").unwrap();
        let (user, enrollment) = prepare_totp_enrollment(&db, "ana@ejemplo.com").unwrap();
        let totp = test_totp(&enrollment.secret_base32, &user.email);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = totp.generate(now).to_string();
        confirm_totp_enrollment(&db, &user, &enrollment.secret_base32, &code).unwrap();

        let err = prepare_totp_enrollment(&db, "ana@ejemplo.com")
            .expect_err("should refuse to re-enroll while 2FA is active");
        assert!(err.to_string().contains("ya tiene 2FA activo"));
    }

    // ── users totp-disable ──────────────────────────────────────────────────

    fn enroll_and_activate(db: &AuthDb, email: &str) -> (User, TotpEnrollment, Vec<String>) {
        create_user_in_db(db, email, OK_PASS, "Ana", "user").unwrap();
        let (user, enrollment) = prepare_totp_enrollment(db, email).unwrap();
        let totp = test_totp(&enrollment.secret_base32, &user.email);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = totp.generate(now).to_string();
        let backup_codes =
            confirm_totp_enrollment(db, &user, &enrollment.secret_base32, &code).unwrap();
        let reloaded = db.get_user_by_email(email).unwrap().unwrap();
        (reloaded, enrollment, backup_codes)
    }

    #[test]
    fn totp_disable_with_valid_totp_code() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let (user, enrollment, _codes) = enroll_and_activate(&db, "ana@ejemplo.com");

        let totp = test_totp(&enrollment.secret_base32, &user.email);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = totp.generate(now).to_string();

        let via = disable_totp_with_code(&db, "ana@ejemplo.com", &code).unwrap();
        assert_eq!(via, "totp");

        let reloaded = db.get_user_by_email("ana@ejemplo.com").unwrap().unwrap();
        assert!(!reloaded.totp_enabled);
        assert!(reloaded.totp_secret.is_none());
    }

    #[test]
    fn totp_disable_with_backup_code_is_single_use() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        let (_user, _enrollment, codes) = enroll_and_activate(&db, "ana@ejemplo.com");
        let backup_code = codes.first().cloned().unwrap();

        let via = disable_totp_with_code(&db, "ana@ejemplo.com", &backup_code).unwrap();
        assert_eq!(via, "backup_code");

        let reloaded = db.get_user_by_email("ana@ejemplo.com").unwrap().unwrap();
        assert!(!reloaded.totp_enabled);

        // Re-enroll and confirm the SAME backup code no longer works (single-use).
        let (user2, enrollment2) = prepare_totp_enrollment(&db, "ana@ejemplo.com").unwrap();
        let totp2 = test_totp(&enrollment2.secret_base32, &user2.email);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code2 = totp2.generate(now).to_string();
        confirm_totp_enrollment(&db, &user2, &enrollment2.secret_base32, &code2).unwrap();

        let err = disable_totp_with_code(&db, "ana@ejemplo.com", &backup_code)
            .expect_err("a consumed backup code must never work again");
        assert!(err.to_string().contains("invalido"));
    }

    #[test]
    fn totp_disable_rejects_invalid_code() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        enroll_and_activate(&db, "ana@ejemplo.com");

        let err = disable_totp_with_code(&db, "ana@ejemplo.com", "000000")
            .expect_err("bogus code must be rejected");
        assert!(err.to_string().contains("invalido"));

        let reloaded = db.get_user_by_email("ana@ejemplo.com").unwrap().unwrap();
        assert!(
            reloaded.totp_enabled,
            "2FA must remain enabled on a rejected attempt"
        );
    }

    #[test]
    fn totp_disable_refuses_when_not_enabled() {
        let dir = tempdir().unwrap();
        let db = open_test_db(&dir);
        create_user_in_db(&db, "ana@ejemplo.com", OK_PASS, "Ana", "user").unwrap();

        let err = disable_totp_with_code(&db, "ana@ejemplo.com", "123456")
            .expect_err("should refuse: 2FA not enabled");
        assert!(err.to_string().contains("no tiene 2FA activo"));
    }
}
