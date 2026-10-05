//! Native Google Drive connection for backup, without hand-editing `rclone.conf`.
//!
//! # Why loopback and not device-code
//!
//! RFC 8628 device flow exists for **input-limited** clients (TVs, CLIs with no
//! browser). Google supports it only for that case ("OAuth 2.0 for TV and
//! Limited-Input Device Applications"), and the sibling OOB flow it was meant to
//! replace was fully blocked in January 2023. A backup daemon on a workstation
//! has a browser and a loopback interface, so it should use the flow Google
//! actually recommends for installed/native clients:
//!
//! - **loopback redirect** on `http://127.0.0.1:<ephemeral>` + PKCE S256.
//!   One browser consent, no code pasted, and the loopback binding means another
//!   machine on the LAN cannot intercept the callback. Port is ephemeral, which
//!   is why it must be registered as a *client type* redirect, not a fixed URI.
//!
//! Trade-off, stated honestly: loopback needs the consent to happen while the
//! daemon is waiting. Device-code would survive a daemon restart mid-consent, but
//! at the cost of a lower-privilege grant type and a user-code typing ritual.
//! For a nightly backup timer, one interactive consent at setup is the right
//! trade.
//!
//! # Why `drive.file` and not `drive`
//!
//! [`DriveScopes::BACKUP`] requests
//! `https://www.googleapis.com/auth/drive.file`, which grants access **only to
//! files this app created**. `drive` would grant the entire account — every
//! document, every photo — for a job whose entire job is uploading a vault.
//!
//! This is Google's explicit policy ("choose the most narrowly focused scope
//! possible"), it makes the consent screen non-sensitive (users grant it without
//! a warning), and it is the difference between a compromised refresh token
//! exposing a memory vault versus exposing a life.
//!
//! **The trade-off you must know about**: with `drive.file`, the app can only see
//! files *it* created. It therefore **cannot read, rotate, or delete the four
//! historical plaintext `master.key` files** sitting in `gdrive:SWAL/backups/`
//! (created by rclone under the broader `drive` grant), and it cannot delete the
//! legacy snapshots. Cleanup of those must be done once with the existing rclone
//! remote, by hand. That is the price of the narrower scope, and it is worth it.
//!
//! # Token storage
//!
//! The `refresh_token` is a long-lived credential, so it is sealed with
//! AES-256-GCM under a key derived from the Xavier master key via HKDF
//! ([`drive_credentials`]), never written in the clear and never logged.
//!
//! Note the deliberate asymmetry with [`crate::recovery`]: the refresh token is
//! wrapped by `master.key`, which is **re-derivable** from host + machine-id
//! (`src/keystore/mod.rs:183`). So a lost `master.key` costs a re-consent, not
//! data — which is exactly the right severity for a credential, and the exact
//! wrong trade for a memory vault. That asymmetry is the whole argument for
//! treating the record key and the master key as different assets.
//!
//! # Reusing an existing rclone remote
//!
//! [`detect_rclone_remote`] reports whether a usable remote already exists. The
//! native path is only taken when it does not, so the two working timers
//! (`xavier-backup.timer`, `xavier-backup-verify.timer`) and the already-valid
//! `gdrive-xavier-crypt:` remote are untouched by this module.

pub mod credentials;
pub mod device_flow;
pub mod rclone;

pub use credentials::{DriveCredentialStore, DriveCredentials, DriveScopes, TokenKind};
pub use device_flow::{DeviceCode, DriveAuthClient, OAuthTransport, TokenResponse};
pub use rclone::{detect_rclone_remote, RcloneRemote, TransportKind};

/// Default rclone config path.
pub const DEFAULT_RCLONE_CONFIG: &str = "~/.config/rclone/rclone.conf";
