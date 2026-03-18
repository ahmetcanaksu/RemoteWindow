use std::{thread, time::Duration};

use remote_window::{
    capture::{create_default_capturer, get_monitor_list, ScreenCapturer},
    compression::{FrameCompression, ZstdCompression},
};

fn create_capturer_blocking() -> std::io::Result<Box<dyn ScreenCapturer>> {
    loop {
        println!("[server] initializing screen capturer...");
        match create_default_capturer() {
            Ok(capturer) => {
                println!("[server] screen capturer initialized successfully");
                let (w, h) = capturer.geometry();
                println!("[server] capturer ready with geometry {}x{}", w, h);
                return Ok(capturer);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(e);
            }
            Err(e) => {
                println!("[server] failed to create capturer: {:?}", e);
                println!("[server] retrying capturer initialization in 1s");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn compress_frames_with_chunk(pixels: &[u32], chunk_size: usize) -> Vec<u8> {
    let mut chunks: (usize, Vec<u8>) = (0, vec![0; chunk_size * 4]);
    let mut compressed_chunks = Vec::new();

    for &rgb in pixels {
        //Write chunk as pointer offset increased.
        chunks.1[(chunks.0 * 4)..((chunks.0 * 4) + 4)].copy_from_slice(&rgb.to_le_bytes());
        chunks.0 += 1;

        // If chunk size reached compress the buffer and reset for next chunk.
        if chunks.0 == chunk_size {
            let compressed = ZstdCompression::with_level(3).compress(&chunks.1).unwrap();
            compressed_chunks.extend(compressed);
            chunks.0 = 0;
            chunks.1.fill(0);
        }
    }

    // Handle remaining pixels in the last chunk if any.
    if chunks.0 > 0 {
        let compressed = ZstdCompression::with_level(3)
            .compress(&chunks.1[..(chunks.0 * 4)])
            .unwrap();
        compressed_chunks.extend(compressed);
    }

    return compressed_chunks;
}

fn compress_frames(pixels: &[u32]) -> Vec<u8> {
    let raw_frame = pixels
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .collect::<Vec<u8>>();
    ZstdCompression::with_level(3).compress(&raw_frame).unwrap()
}

fn main() {
    let displays = get_monitor_list();
    println!("Detected displays: {:#?}", displays);
}
