pub mod capture;
pub mod color;
pub mod compression;
pub mod config;
pub mod connection;
pub mod connection_handler;
pub mod cursor;
pub mod screen;
pub mod performance_track;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub mod dxgi_capturer;
