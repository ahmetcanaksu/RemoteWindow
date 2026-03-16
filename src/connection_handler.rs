use std::{mem, time::{Duration, Instant}};

use crate::{
    capture::ScreenCapturer, compression::FrameCompression, config, connection::ServerConnection,
};

fn pixels_as_le_bytes<'a>(pixels: &'a [u32], scratch: &'a mut Vec<u8>) -> &'a [u8] {
    if cfg!(target_endian = "little") {
        let byte_len = mem::size_of_val(pixels);
        // Safe: u32 buffer is contiguous, properly aligned, and we only reinterpret as bytes.
        unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, byte_len) }
    } else {
        scratch.clear();
        scratch.reserve(pixels.len() * 4);
        for &rgb in pixels {
            scratch.extend_from_slice(&rgb.to_le_bytes());
        }
        scratch.as_slice()
    }
}

fn handle_connection(
    connection: &mut dyn ServerConnection,
    capturer: &mut dyn ScreenCapturer,
    compression: &dyn FrameCompression,
    (w, h): (u32, u32),
) -> std::io::Result<()> {
    let mut frames_sent: u64 = 0;
    let mut last_log = std::time::Instant::now();
    let mut raw_frame_scratch = Vec::new();

    let frame_interval = Duration::from_millis(config::frame_interval_ms());

    loop {
        let frame_start = Instant::now();
        let pixels = capturer.capture_frame()?;
        let pixel_count = pixels.len() as u32;
        let raw_frame = pixels_as_le_bytes(&pixels, &mut raw_frame_scratch);
    }
}
