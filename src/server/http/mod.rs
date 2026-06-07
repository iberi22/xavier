//! HTTP server implementation for Xavier.
//!
//! This module provides the core HTTP server infrastructure, including configuration,
//! lifecycle management (startup/shutdown), and various API endpoints organized into
//! submodules for health monitoring, memory management, and real-time communication.

pub mod api;
pub mod health;
pub mod types;
pub mod v1;
pub mod websocket;

use tracing::error;

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
}
impl HttpConfig {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
    pub fn with_tls(mut self, cert_path: String, key_path: String) -> Self {
        self.tls_enabled = true;
        self.tls_cert_path = Some(cert_path);
        self.tls_key_path = Some(key_path);
        self
    }
}

pub struct HttpServer {
    _config: HttpConfig,
}
impl HttpServer {
    pub fn new(config: HttpConfig) -> Self {
        Self { _config: config }
    }
    pub async fn serve(&self) {
        tracing::warn!("HttpServer::serve() is a stub");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

pub async fn start_signal_handler(state: websocket::ShutdownState) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).unwrap();
            let mut sigint = signal(SignalKind::interrupt()).unwrap();
            tokio::select! { _ = sigterm.recv() => state.request_shutdown("SIGTERM"), _ = sigint.recv() => state.request_shutdown("SIGINT") }
        }
        #[cfg(windows)]
        {
            use tokio::signal::windows::ctrl_c;
            if let Ok(mut rx) = ctrl_c() {
                if let Some(()) = rx.recv().await {
                    state.request_shutdown("Ctrl+C / console close");
                }
            }
        }
    });
}

pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let thread = std::thread::current();
        error!(panic_message = %msg, panic_location = %location, thread_name = %thread.name().unwrap_or("unknown"), "xavier_panic");
        default_hook(panic_info);
    }));
}
