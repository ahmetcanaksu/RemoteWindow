/// Central env-var configuration for RemoteWindow.
///
/// Every tunable is read once at call time from the environment and exposed as
/// a typed value.  All variable names, defaults, and accepted values are
/// documented here so there is a single place to look.
///
/// | Variable              | Default          | Accepted values                         |
/// |-----------------------|------------------|-----------------------------------------|
/// | RW_BIND_ADDR          | 127.0.0.1:8082   | host:port                               |
/// | RW_SERVER_ADDR        | 127.0.0.1:8082   | host:port                               |
/// | RW_TRANSPORT          | udp              | tcp | udp | both                       |
/// | RW_COMPRESSION        | lz4              | none | zstd | lz4                      |
/// | RW_CAPTURE_BACKEND    | auto             | auto | swift | scrap                  |
/// | RW_FPS_CAP            | 30               | positive integer                        |
use std::env;

use crate::cursor::Cursor;

pub const DEFAULT_ADDR: &str = "127.0.0.1:8082";
pub const DEFAULT_FPS_CAP: u32 = 60;
pub const DEFAULT_TRANSPORT: &str = "udp";
pub const DEFAULT_COMPRESSION: &str = "lz4";
pub const DEFAULT_CAPTURE_BACKEND: &str = "auto";

/// Address the server binds to.  Overridden with `RW_BIND_ADDR`.
pub fn bind_addr() -> String {
    env::var("RW_BIND_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

/// Address the client connects to.  Overridden with `RW_SERVER_ADDR`.
pub fn server_addr() -> String {
    env::var("RW_SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

/// Transport protocol selection.  Overridden with `RW_TRANSPORT`.
pub fn transport() -> String {
    env::var("RW_TRANSPORT").unwrap_or_else(|_| DEFAULT_TRANSPORT.to_string())
}

/// Frame compression algorithm.  Overridden with `RW_COMPRESSION`.
pub fn compression() -> String {
    env::var("RW_COMPRESSION").unwrap_or_else(|_| DEFAULT_COMPRESSION.to_string())
}

/// macOS capture backend selection.  Overridden with `RW_CAPTURE_BACKEND`.
pub fn capture_backend() -> String {
    env::var("RW_CAPTURE_BACKEND").unwrap_or_else(|_| DEFAULT_CAPTURE_BACKEND.to_string())
}

/// Maximum frames per second the server will stream.  Overridden with `RW_FPS_CAP`.
pub fn fps_cap() -> u32 {
    env::var("RW_FPS_CAP")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_FPS_CAP)
}

/// Frame interval in milliseconds derived from `fps_cap()`.
pub fn frame_interval_ms() -> u64 {
    (1000 / fps_cap()) as u64
}

/// Print all active configuration values to stdout.
pub fn print_config() {
    println!("[config] RW_BIND_ADDR       = {}", bind_addr());
    println!("[config] RW_SERVER_ADDR     = {}", server_addr());
    println!("[config] RW_TRANSPORT       = {}", transport());
    println!("[config] RW_COMPRESSION     = {}", compression());
    println!("[config] RW_CAPTURE_BACKEND = {}", capture_backend());
    println!(
        "[config] RW_FPS_CAP         = {} fps ({} ms/frame)",
        fps_cap(),
        frame_interval_ms()
    );
}

pub fn print_config_to_cursor(cursor: &mut Cursor) {
    cursor.println(&format!("RW_BIND_ADDR       = {}", bind_addr()));
    cursor.println(&format!("RW_SERVER_ADDR     = {}", server_addr()));
    cursor.println(&format!("RW_TRANSPORT       = {}", transport()));
    cursor.println(&format!("RW_COMPRESSION     = {}", compression()));
    cursor.println(&format!("RW_CAPTURE_BACKEND = {}", capture_backend()));
    cursor.println(&format!(
        "RW_FPS_CAP         = {} fps ({} ms/frame)",
        fps_cap(),
        frame_interval_ms()
    ));
}
