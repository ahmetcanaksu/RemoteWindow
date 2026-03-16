use fontdue::{Font, FontSettings};
use minifb::{Key, Scale, Window, WindowOptions};
use std::convert::TryInto;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{fs, thread};
use std::{io::Read, net::TcpStream};
use RemoteWindow::Color;

fn main() {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    const CHUNK_SIZE: usize = 600;

    let file: Vec<u8> = fs::read("./fira_code.ttf".to_string()).unwrap();
    let font = Font::from_bytes(file, FontSettings::default()).unwrap();

    //Create ArcMutex for screen buffer
    let screen_buffer = Arc::new(Mutex::new(vec![0; WIDTH * HEIGHT]));
    //Create ArcAtomicBool for screen buffer update state
    let screen_updated = Arc::new(AtomicBool::new(true));

    let screen_updated_clone = screen_updated.clone();
    let screen_buffer_clone = screen_buffer.clone();

    let connection_thread = thread::spawn(move || {
        let mut capture_flag = true;
        let mut capture_buffer_size = false;
        let mut w = 0;
        let mut h = 0;

        let mut socket = TcpStream::connect("127.0.0.1:82").unwrap();
        // Notify server for frame render.
        socket.write(&[0x88]).unwrap();

        loop {
            // If server is ready to send the dimensions of the frame
            if capture_flag {
                let mut buffer = [0; 4];
                socket.read(&mut buffer).unwrap();
                if buffer[0] == 0x33 && buffer[1] == 0x34 && buffer[2] == 0x35 && buffer[3] == 0x36
                {
                    capture_flag = false;
                    capture_buffer_size = true;
                }
            } else if capture_buffer_size {
                // Read the frame dimensions from the server
                let mut w_buffer = [0; 4];
                let mut h_buffer = [0; 4];
                socket.read(&mut w_buffer).unwrap();
                socket.read(&mut h_buffer).unwrap();
                w = u32::from_le_bytes(w_buffer);
                h = u32::from_le_bytes(h_buffer);
                capture_buffer_size = false;
                capture_flag = false;
            } else {
                let mut rendered_pixel_count: usize = 0;

                //Read through the socket until the end of the frame
                while rendered_pixel_count < (h as usize * w as usize) {
                    let mut pixel_buffer = [0; CHUNK_SIZE * 4];
                    // Read the next chunk of pixels from the server
                    socket.read(&mut pixel_buffer).unwrap();

                    // Write chunk as rgb entries to the screen buffer.
                    for chunk in 0..CHUNK_SIZE {
                        // 1920 * 1080 = 2073600
                        let u32_buffer: [u8; 4] =
                            pixel_buffer[chunk * 4..(chunk + 1) * 4].try_into().unwrap();
                        if (h as usize * w as usize) > (rendered_pixel_count + chunk) {
                            screen_buffer_clone.lock().unwrap()[rendered_pixel_count + chunk] =
                                u32::from_le_bytes(u32_buffer);
                        }
                    }
                    rendered_pixel_count += CHUNK_SIZE;
                }
                capture_flag = true;
                capture_buffer_size = false;
                // Notify server for new frame render.
                socket.write_all(&[0x88]).unwrap();
                screen_updated_clone.store(true, Ordering::Relaxed);
            }
        }
    });

    let screen_thread = thread::spawn(move || {
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

        while window.is_open() && !window.is_key_down(Key::Escape) {
            // If screen updated read from buffer.
            if screen_updated.load(Ordering::Relaxed) {
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
                            let mut current_y = y;
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
                    }
                    Err(_) => {
                        println!("LOCK ERROR");
                    }
                }
            }
        }
    });

    // Join threads
    connection_thread.join().unwrap();
    screen_thread.join().unwrap();
}
