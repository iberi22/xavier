//! Messaging Integrations module for Xavier
//!
//! Supports Discord, Telegram (via separate module), and others.

pub mod discord;

pub use discord::DiscordClient;
