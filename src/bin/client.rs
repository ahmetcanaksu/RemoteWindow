use minifb::{Key, Scale, Window, WindowOptions};
use std::sync::mpsc::{sync_channel, TryRecvError, TrySendError};
use std::time::Duration;
use std::thread;
use RemoteWindow::compression::{create_frame_compression, CompressionKind};
use RemoteWindow::config;
use RemoteWindow::connection::{create_client_connection, TransportMode};

fn main() {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;

    // Keep only the newest decoded frame to avoid queue buildup under load.
    let (frame_tx, frame_rx) = sync_channel::<Vec<u32>>(1);

    let connection_thread = thread::spawn(move || {
        let mut reconnect_attempts: u64 = 0;

        loop {
            let transport = TransportMode::from_env();
            let server_addr = config::server_addr();
            println!(
                "[client/net] connecting to {} using {:?} (attempt #{})",
                server_addr,
                transport,
                reconnect_attempts + 1
            );

            let mut connection = match create_client_connection(transport, &server_addr) {
                Ok(connection) => connection,
                Err(e) => {
                    println!("Connection error: {}", e);
                    reconnect_attempts += 1;
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            println!("[client/net] connected to {}", server_addr);
            let mut frames_received: u64 = 0;
            let mut last_net_log = std::time::Instant::now();
            let mut active_codec_kind: Option<CompressionKind> = None;
            let mut active_codec = create_frame_compression(CompressionKind::Lz4);

            if matches!(transport, TransportMode::Udp) {
                if let Err(e) = connection.request_frame() {
                    println!("[client/net] UDP registration failed: {}", e);
                    break;
                }
            }

            loop {
                let header = match connection.read_frame_header() {
                    Ok(header) => header,
                    Err(e) => {
                        println!("Header read error: {}", e);
                        break;
                    }
                };

                let w = header.width;
                let h = header.height;
                let frame_pixel_count = header.pixel_count as usize;
                let expected_raw_len = frame_pixel_count * 4;
                let expected_payload_len = header.payload_len as usize;
                if active_codec_kind != Some(header.compression) {
                    active_codec_kind = Some(header.compression);
                    active_codec = create_frame_compression(header.compression);
                }

                let mut payload = vec![0_u8; expected_payload_len];
                let mut payload_len = 0usize;

                while payload_len < expected_payload_len {
                    let read_size = match connection.read_chunk(&mut payload[payload_len..]) {
                        Ok(size) => size,
                        Err(e) => {
                            println!("Chunk read error: {}", e);
                            payload_len = 0;
                            break;
                        }
                    };

                    if payload_len + read_size > expected_payload_len {
                        println!(
                            "[client/net] frame payload overflow: got {} + {} bytes, expected {}",
                            payload_len,
                            read_size,
                            expected_payload_len
                        );
                        payload_len = 0;
                        break;
                    }

                    payload_len += read_size;
                }

                if payload_len == expected_payload_len {
                    let raw_frame = match active_codec.decompress(&payload, expected_raw_len) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            println!("[client/net] decompression error: {}", e);
                            break;
                        }
                    };

                    if raw_frame.len() != expected_raw_len {
                        println!(
                            "[client/net] decoded frame size mismatch: got {} bytes, expected {}",
                            raw_frame.len(),
                            expected_raw_len
                        );
                        break;
                    }

                    // Convert bytes to pixels with a bulk copy on little-endian targets.
                    let buf_len = WIDTH * HEIGHT;
                    let copy_pixels = frame_pixel_count.min(buf_len);
                    let mut local_pixels = vec![0_u32; buf_len];
                    if cfg!(target_endian = "little") {
                        let byte_len = copy_pixels * 4;
                        // Safe: both source and destination are valid for byte_len bytes and non-overlapping.
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                raw_frame.as_ptr(),
                                local_pixels.as_mut_ptr() as *mut u8,
                                byte_len,
                            );
                        }
                    } else {
                        for (dst, pixel_bytes) in local_pixels
                            .iter_mut()
                            .zip(raw_frame.chunks_exact(4).take(copy_pixels))
                        {
                            *dst = u32::from_le_bytes([
                                pixel_bytes[0],
                                pixel_bytes[1],
                                pixel_bytes[2],
                                pixel_bytes[3],
                            ]);
                        }
                    }

                    match frame_tx.try_send(local_pixels) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            // UI hasn't consumed previous frame yet; drop this one.
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }

                    frames_received += 1;

                    if last_net_log.elapsed().as_secs_f32() >= 1.0 {
                        println!(
                            "[client/net] receiving frames ok: {} fps, last frame {}x{}, codec={}, payload={} bytes",
                            frames_received,
                            w,
                            h,
                            header.compression.name(),
                            expected_payload_len
                        );
                        frames_received = 0;
                        last_net_log = std::time::Instant::now();
                    }
                } else {
                    println!(
                        "[client/net] dropped incomplete frame payload: got {} / {} bytes",
                        payload_len,
                        expected_payload_len
                    );
                    break;
                }
            }

            println!("[client/net] disconnected; retrying...");
            reconnect_attempts += 1;
            thread::sleep(Duration::from_millis(250));
        }
    });

    let mut window = Window::new(
        "Test - ESC to exit",
        WIDTH,
        HEIGHT,
        WindowOptions {
            scale: Scale::FitScreen,
            scale_mode: minifb::ScaleMode::UpperLeft,
            resize: true,
            ..Default::default()
        },
    )
    .unwrap();

    let mut last_draw = std::time::Instant::now();
    let mut rendered_frames = 0;
    let mut fps = 0;
    let mut last_ui_log = std::time::Instant::now();
    let mut waiting_since = std::time::Instant::now();
    let mut present_buffer = vec![0_u32; WIDTH * HEIGHT];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let mut got_new_frame = false;
        loop {
            match frame_rx.try_recv() {
                Ok(frame) => {
                    present_buffer = frame;
                    got_new_frame = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if got_new_frame {
            waiting_since = std::time::Instant::now();
            rendered_frames += 1;
            if last_draw.elapsed().as_secs_f32() > 1.0 {
                fps = rendered_frames;
                rendered_frames = 0;
                last_draw = std::time::Instant::now();
                window.set_title(format!("RemoteWindow - FPS {}", fps).as_str());
            }

            window
                .update_with_buffer(&present_buffer, WIDTH, HEIGHT)
                .unwrap();

            if last_ui_log.elapsed().as_secs_f32() >= 5.0 {
                println!("[client/ui] presenting frames: {} fps", fps);
                last_ui_log = std::time::Instant::now();
            }
        } else if waiting_since.elapsed().as_secs_f32() >= 2.0 {
            println!("[client/ui] waiting for next frame update...");
            waiting_since = std::time::Instant::now();
            window.update();
        } else {
            window.update();
        }
    }

    connection_thread.join().unwrap();
}
