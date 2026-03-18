use std::{
    convert::TryInto,
    fs,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use arc_swap::ArcSwap;
use minifb::{Key, Scale, ScaleMode, Window, WindowOptions};
use remote_window::{
    color::Color,
    compression::{create_frame_compression_from_env, CompressionKind},
    config::{self, fps_cap, print_config_to_cursor},
    connection::{ClientConnection, UdpClientConnection, UdpServerConnection},
    cursor::{Boundaries, Cursor, Font},
    performance_track::PerformanceTracker,
    screen::ScreenBuffer,
};

struct SharedState {
    open_display: AtomicBool,
    width: AtomicUsize,
    height: AtomicUsize,
}

fn main() {
    println!("Client starting...");
    config::print_config();
    let state = Arc::new(SharedState {
        open_display: AtomicBool::new(false),
        width: AtomicUsize::new(0),
        height: AtomicUsize::new(0),
    });
    let screen = ScreenBuffer::new(1920, 1080);
    let shared_frame = Arc::new(ArcSwap::from_pointee(screen));

    // 2. Clone the Arc for the first thread
    let t1_state = Arc::clone(&state);
    let t1_frame = Arc::clone(&shared_frame); // Clone the ArcSwap

    let server_fps = Arc::new(AtomicUsize::new(0));
    let server_fps_clone = Arc::clone(&server_fps);

    let received_fps = Arc::new(AtomicUsize::new(0));
    let received_fps_clone = Arc::clone(&received_fps);

    // Connection thread
    let t1 = thread::spawn(move || {
        let compression = create_frame_compression_from_env();
        let server_addr = config::server_addr();
        let mut connection = UdpClientConnection::connect(&server_addr).unwrap();
        connection
            .socket
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();

        let mut frame_count = 0;
        let mut frame_start_time = std::time::Instant::now();
        let mut frame_buffer = Vec::new();
        let mut server_fps_reset_time = std::time::Instant::now();
        loop {
            connection.request_frame().unwrap();
            let frame_header = connection.read_frame_header().unwrap();

            if server_fps_reset_time.elapsed() >= Duration::from_secs(1) {
                server_fps_clone.store(frame_header.server_fps as usize, Ordering::SeqCst);
                server_fps_reset_time = std::time::Instant::now();
            }

            let total_expected = frame_header.payload_len as usize;
            if frame_buffer.len() != total_expected {
                frame_buffer.resize(total_expected, 0u8);

                // Also update the window dimensions if needed
                if !t1_state.open_display.load(Ordering::SeqCst) {
                    t1_state
                        .width
                        .store(frame_header.width as usize, Ordering::SeqCst);
                    t1_state
                        .height
                        .store(frame_header.height as usize, Ordering::SeqCst);
                    t1_state.open_display.store(true, Ordering::SeqCst);
                }
            }

            let mut bytes_received = 0;
            let total_expected = frame_header.payload_len as usize;

            while bytes_received < total_expected {
                let mut packet_buf = [0u8; 1404]; // 4 bytes index + 1400 bytes data
                                                  /* let (amt, _) = connection
                                                  .socket
                                                  .recv_from(&mut packet_buf)
                                                  .unwrap_or_else(|e| {
                                                      eprintln!(
                                                          "[t1] Failed to receive packet after receiving {} bytes, expected {} bytes, error: {}",
                                                          bytes_received, total_expected, e
                                                      );
                                                      (0,connection.socket.local_addr().unwrap())
                                                  }); */
                match connection.socket.recv_from(&mut packet_buf) {
                    Ok((amt, _)) => {
                        if amt > 4 {
                            let chunk_index =
                                u32::from_le_bytes(packet_buf[0..4].try_into().unwrap()) as usize;
                            /* if (chunk_index % 100) == 0 {
                                println!(
                                    "[t1] Received chunk index: {}, size: {} bytes",
                                    chunk_index,
                                    amt - 4
                                );
                            } */

                            let chunk_data = &packet_buf[4..amt];

                            let start_offset = chunk_index * 1400;
                            let end_offset = start_offset + chunk_data.len();

                            // Safety check to prevent buffer overflow
                            if end_offset <= frame_buffer.len() {
                                frame_buffer[start_offset..end_offset].copy_from_slice(chunk_data);
                                bytes_received += chunk_data.len();
                            }
                        }
                    }

                    Err(e) => {
                        eprintln!("[t1] Error receiving packet: {}", e);
                        break;
                    }
                }
            }
            /* println!("[t1] Frame fully reassembled!"); */

            if bytes_received >= total_expected {
                let uncompressed = compression
                    .decompress(&frame_buffer, frame_header.pixel_count as usize)
                    .expect("Failed to decompress frame");

                
                // Only swap if we actually got the whole thing
                let u32_view: &[u32] = bytemuck::cast_slice(&uncompressed);
                println!(
                    "[t1] Frame received: {} bytes ({} pixels), decompressing and updating shared frame...",
                    uncompressed.len(),
                    frame_header.pixel_count
                );
                // Create the custom struct
                let new_screen = ScreenBuffer {
                    width: frame_header.width as usize,
                    height: frame_header.height as usize,
                    buffer: u32_view.to_vec(), // Still a copy, but keeps your utilities intact
                };

                // Store the whole struct atomically
                t1_frame.store(Arc::new(new_screen));

                //t1_frame.store(Arc::new(u32_view.to_vec()));
                frame_count += 1;
            }
            /*
                let u32_view: &[u32] = bytemuck::cast_slice(&frame_buffer);
                t1_frame.store(Arc::new(u32_view.to_vec()));
            */

            if frame_start_time.elapsed() >= Duration::from_secs(1) {
                let elapsed = frame_start_time.elapsed().as_secs_f32();
                let fps = frame_count as f32 / elapsed;

                /* println!(
                    "[t1] Performance: {:.2} FPS | Bytes expected: {}",
                    fps, total_expected
                ); */

                frame_count = 0;
                frame_start_time = std::time::Instant::now();
                received_fps_clone.store(fps.round() as usize, Ordering::SeqCst);
            }
        }
    });

    let t2_state = Arc::clone(&state);
    let t2_frame = Arc::clone(&shared_frame); // Clone the ArcSwap
                                              //Render thread
    let t2 = thread::spawn(move || {
        if t2_state.open_display.load(Ordering::SeqCst) == false {
            println!("[t2] Waiting for frame header to determine screen dimensions...");
            while t2_state.open_display.load(Ordering::SeqCst) == false {
                thread::sleep(Duration::from_millis(50));
            }
        }

        let file: Vec<u8> = fs::read("./font.ttf".to_string()).unwrap();
        //let font = Font::from_bytes(file, FontSettings::default()).unwrap();

        let width = t2_state.width.load(Ordering::SeqCst);
        let height = t2_state.height.load(Ordering::SeqCst);
        /* println!(
            "[t2] Frame header received, opening window with dimensions: {}x{}",
            width, height
        ); */

        let mut cursor = Cursor::new(
            Font::from_bytes(file, 18.0),
            Boundaries {
                start_x: 15,
                start_y: 15,
                width: width,
                height: height,
            },
        );

        let mut window = Window::new(
            "Test - ESC to exit",
            width,
            height,
            WindowOptions {
                scale: Scale::FitScreen,
                scale_mode: minifb::ScaleMode::UpperLeft,
                resize: true,
                ..Default::default()
            },
        )
        .unwrap();

        let mut frame_count = 0;
        let mut frame_start_time = std::time::Instant::now();
        let mut fps_tracker = PerformanceTracker::new(100);

        window.limit_update_rate(None);

        let frame_time = Duration::from_secs_f32(1.0 / (fps_cap() + 10) as f32);
        let mut last_draw_time = std::time::Instant::now();

        while window.is_open() && !window.is_key_down(Key::Escape) {
            window.update();
            if last_draw_time.elapsed() >= frame_time {
                let current_video_frame: arc_swap::Guard<Arc<ScreenBuffer>> = t2_frame.load();

                // 2. Clone it so we can draw on it without affecting the original
                // This is your "Double Buffer"
                let mut composition_buffer = current_video_frame.as_ref().clone();
                frame_count += 1;

                if frame_start_time.elapsed() >= Duration::from_secs(1) {
                    let elapsed = frame_start_time.elapsed().as_secs_f32();
                    let fps = frame_count as f32 / elapsed;

                    let server_fps = server_fps.load(Ordering::SeqCst);
                    let received_fps = received_fps.load(Ordering::SeqCst);
                    fps_tracker.add_samples(server_fps as f32, received_fps as f32, fps);
                    /* println!(
                        "[t2] Performance: {:.2} FPS | Server FPS: {} | Received FPS: {}",
                        fps, server_fps, received_fps
                    ); */
                    cursor.clean_buffer();
                    cursor.color = Color::from_rgb(0, 150, 255);
                    cursor.println(&format!("Server   FPS: {}", server_fps));
                    cursor.color = Color::from_rgb(255, 200, 0);
                    cursor.println(&format!("Received FPS: {}", received_fps));
                    cursor.color = Color::from_rgb(0, 255, 100);
                    cursor.println(&format!("Render   FPS: {}", fps.round()));
                    cursor.color = Color::from_rgb(255, 255, 255);
                    cursor.println("-----------------------");
                    config::print_config_to_cursor(&mut cursor);

                    frame_count = 0;
                    frame_start_time = std::time::Instant::now();
                }

                // Draw the chart at (50, 50) with 200px width
                composition_buffer.draw_performance_chart(
                    &fps_tracker,
                    15,
                    220,
                    200,
                    100,
                    &cursor.font,
                );

                // 3. Render the cursor onto the clone
                // Note: You should use draw_char_transparent here!
                cursor.render(&mut composition_buffer);

                window
                    .update_with_buffer(&composition_buffer.buffer, width, height)
                    .unwrap();
                last_draw_time = std::time::Instant::now();
            }
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
}
