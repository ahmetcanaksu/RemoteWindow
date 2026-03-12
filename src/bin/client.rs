use fontdue::{Font, FontSettings};
use minifb::{Key, Scale, Window, WindowOptions};
use std::convert::TryInto;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::env;
use std::time::Duration;
use std::{fs, thread};
use RemoteWindow::Color;
use RemoteWindow::connection::{create_client_connection, TransportMode, DEFAULT_ADDR};

fn main() {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    const CHUNK_SIZE: usize = 600;

    let file = fs::read("./fira_code.ttf".to_string()).unwrap();
    let font = Font::from_bytes(file, FontSettings::default()).unwrap();

    //Create ArcMutex for screen buffer
    let screen_buffer = Arc::new(Mutex::new(vec![0; WIDTH * HEIGHT]));
    //Create ArcAtomicBool for screen buffer update state
    let screen_updated = Arc::new(AtomicBool::new(false));

    let screen_updated_clone = screen_updated.clone();
    let screen_buffer_clone = screen_buffer.clone();

    let connection_thread = thread::spawn(move || {
        let mut reconnect_attempts: u64 = 0;

        loop {
            let transport = TransportMode::from_env();
            let server_addr = env::var("RW_SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
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

            'connection_loop: loop {
                if let Err(e) = connection.request_frame() {
                    println!("[client/net] frame request failed: {}", e);
                    break;
                }

                let header = loop {
                    match connection.read_frame_header() {
                        Ok(header) => break header,
                        Err(ref e)
                            if e.kind() == ErrorKind::WouldBlock
                                || e.kind() == ErrorKind::TimedOut =>
                        {
                            println!("[client/net] waiting for frame header...");
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            println!("Header read error: {}", e);
                            break 'connection_loop;
                        }
                    }
                };

                let w = header.width;
                let h = header.height;
                let frame_pixel_count = header.pixel_count as usize;
                let mut rendered_pixel_count: usize = 0;

                // Read frame chunks until all expected pixels are consumed.
                while rendered_pixel_count < frame_pixel_count {
                    let mut pixel_buffer = [0; CHUNK_SIZE * 4];
                    let read_size = match connection.read_chunk(&mut pixel_buffer) {
                        Ok(size) => size,
                        Err(e) => {
                            println!("Chunk read error: {}", e);
                            rendered_pixel_count = 0;
                            break;
                        }
                    };
                    if read_size % 4 != 0 {
                        println!(
                            "[client/net] invalid chunk byte size: {} (not multiple of 4)",
                            read_size
                        );
                        rendered_pixel_count = 0;
                        break;
                    }

                    let read_pixels = read_size / 4;

                    for chunk in 0..read_pixels {
                        let u32_buffer: [u8; 4] =
                            pixel_buffer[chunk * 4..(chunk + 1) * 4].try_into().unwrap();
                        if frame_pixel_count > (rendered_pixel_count + chunk) {
                            screen_buffer_clone.lock().unwrap()[rendered_pixel_count + chunk] =
                                u32::from_le_bytes(u32_buffer);
                        }
                    }

                    rendered_pixel_count += read_pixels;
                }

                if rendered_pixel_count == frame_pixel_count {
                    frames_received += 1;
                    screen_updated_clone.store(true, Ordering::Relaxed);

                    if last_net_log.elapsed().as_secs_f32() >= 1.0 {
                        println!(
                            "[client/net] receiving frames ok: {} fps, last frame {}x{}",
                            frames_received,
                            w,
                            h
                        );
                        frames_received = 0;
                        last_net_log = std::time::Instant::now();
                    }
                } else {
                    println!(
                        "[client/net] dropped incomplete frame: got {} / {} pixels",
                        rendered_pixel_count,
                        frame_pixel_count
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

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // If screen updated read from buffer.
        if screen_updated.load(Ordering::Relaxed) {
            waiting_since = std::time::Instant::now();
            match screen_buffer.try_lock() {
                Ok(mut screen_buffer) => {
                    // Update buffer from window.

                    let color = Color::green();
                    let background_color = Color::from_rgb(0, 0, 0);
                    let font_size = 33;

                    let mut put_pixel = |x: usize, y: usize, color: Color| {
                        screen_buffer[y * WIDTH + x] = color.to_hex_rgb();
                    };

                    let mut draw_char = |chr: char, x: usize, y: usize| {
                        let (metrics, bitmap) = font.rasterize(chr, font_size as f32);
                        let mut current_x = x;
                        let mut current_y = y;

                        for y in 0..metrics.height {
                            for x in 0..metrics.width {
                                let char_s = bitmap[x + y * metrics.width];

                                let mut char_color = Color::from_rgb(
                                    char_s as u32,
                                    char_s as u32,
                                    char_s as u32,
                                );

                                if char_color.red != 0
                                    && char_color.green != 0
                                    && char_color.blue != 0
                                {
                                    char_color = color
                                } else if char_color.red == 0
                                    && char_color.green == 0
                                    && char_color.blue == 0
                                {
                                    char_color = background_color;
                                }

                                put_pixel(
                                    current_x,
                                    current_y
                                        + ((metrics.ymin * -1) as usize)
                                        + (if (font_size as usize) < metrics.height {
                                            0
                                        } else {
                                            (font_size as usize) - metrics.height
                                        }),
                                    char_color,
                                );

                                current_x += 1;
                            }
                            current_y += 1;
                            current_x = x;
                        }
                    };

                    let mut draw_string = |string: &str, x: usize, y: usize| {
                        let mut current_x = x;
                        let current_y = y;
                        for chr in string.chars() {
                            draw_char(chr, current_x, current_y);
                            current_x += 16;
                        }
                    };

                    rendered_frames += 1;
                    if last_draw.elapsed().as_secs_f32() > 1.0 {
                        fps = rendered_frames;
                        rendered_frames = 0;
                        last_draw = std::time::Instant::now();
                    }

                    draw_string(format!("FPS: {}", fps).as_str(), 0, 0);

                    window
                        .update_with_buffer(&screen_buffer, WIDTH, HEIGHT)
                        .unwrap();
                    screen_updated.store(false, Ordering::Relaxed);

                    if last_ui_log.elapsed().as_secs_f32() >= 1.0 {
                        println!("[client/ui] presenting frames: {} fps", fps);
                        last_ui_log = std::time::Instant::now();
                    }
                }
                Err(_) => {
                    println!("LOCK ERROR");
                }
            }
        } else if waiting_since.elapsed().as_secs_f32() >= 2.0 {
            println!("[client/ui] waiting for next frame update...");
            waiting_since = std::time::Instant::now();
        }
    }

    connection_thread.join().unwrap();
}
