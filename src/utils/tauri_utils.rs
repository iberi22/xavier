#[cfg(feature = "tauri")]
use std::sync::OnceLock;

#[cfg(feature = "tauri")]
use tauri::AppHandle;

#[cfg(feature = "tauri")]
static TAURI_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[cfg(feature = "tauri")]
pub fn set_tauri_app_handle(handle: AppHandle) {
    let _ = TAURI_APP_HANDLE.set(handle);
}

#[cfg(feature = "tauri")]
pub fn get_tauri_app_handle() -> Option<&'static AppHandle> {
    let handle = TAURI_APP_HANDLE.get();
    if handle.is_none() {
        tracing::debug!("Tauri app handle is not yet initialized");
    }
    handle
}
