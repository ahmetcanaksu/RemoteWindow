use std::{thread, time::Duration};

use remote_window::{
    capture::{create_default_capturer, ScreenCapturer},
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
    let mut capturer = create_capturer_blocking().unwrap();

    let frame = capturer.capture_frame().unwrap();

    let chunk_frame_timer = std::time::Instant::now();
    let compressed_chunk_frame = compress_frames_with_chunk(&frame, 3000);
    println!(
        "Compressed frame with chunking in {:?}, size: {} bytes",
        chunk_frame_timer.elapsed(),
        compressed_chunk_frame.len()
    );

    let full_frame_timer = std::time::Instant::now();
    let compressed_full_frame = compress_frames(&frame);
    println!(
        "Compressed full frame in {:?}, size: {} bytes",
        full_frame_timer.elapsed(),
        compressed_full_frame.len()
    );

    if compressed_chunk_frame.len() < compressed_full_frame.len() {
        println!(
            "Chunked compression resulted in smaller size by {} bytes {} bytes in total",
            compressed_full_frame.len() - compressed_chunk_frame.len(),
            compressed_chunk_frame.len()
        );
    } else if compressed_full_frame.len() < compressed_chunk_frame.len() {
        println!(
            "Full frame compression resulted in smaller size by {} bytes {} bytes in total",
            compressed_chunk_frame.len() - compressed_full_frame.len(),
            compressed_full_frame.len()
        );
    } else {
        println!("Both compression methods resulted in the same size");
    }

    if full_frame_timer.elapsed() < chunk_frame_timer.elapsed() {
        println!(
            "Full frame compression was faster by {:?}",
            chunk_frame_timer.elapsed() - full_frame_timer.elapsed()
        );
    } else if chunk_frame_timer.elapsed() < full_frame_timer.elapsed() {
        println!(
            "Chunked compression was faster by {:?}",
            full_frame_timer.elapsed() - chunk_frame_timer.elapsed()
        );
    } else {
        println!("Both compression methods took the same time");
    }

    /*     println!("Captured frame with {} pixels", frame.len());
       println!("First 10 pixels: {:?}", &frame[..10]);
       println!("First 10 pixels as little-endian bytes: {:?}", frame[..10].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>());

       let ztsdCompressed =
           ZstdCompression::with_level(3)
           .compress(
               frame[..10].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().as_slice()
           ).unwrap();

       println!("Zstd compressed first 10 pixels ({} bytes): {:?}", ztsdCompressed.len(), ztsdCompressed);
    */
    /* let listener = TcpListener::bind("localhost:80");
    if let Ok(socket) = listener {
        for stream in socket.incoming() {
            match stream {
                Err(e) => println!("Accept err {}", e),
                Ok(stream) => {
                    thread::spawn(|| handle_client(stream));
                }
            }
            //handle_client(stream.unwrap());
        }
    } else if let Err(error) = listener {
        println!("Failed to bind server: {:#?}", error);
    } */
}
