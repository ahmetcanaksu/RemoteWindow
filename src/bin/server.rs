use std::{
    io::BufWriter,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Controls::{TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON};
use windows::Win32::UI::WindowsAndMessaging::IDOK;

use arc_swap::ArcSwap;
use remote_window::{
    capture::{
        create_default_capturer, getSystemMonitors, get_monitors, MonitorData, ScreenCapturer,
    },
    compression::{create_frame_compression_from_env, CompressionKind},
    config,
    connection::{
        bind_tcp_listener, ServerConnection, TcpServerConnection, TransportMode,
        UdpServerConnection,
    },
    dxgi_capturer::DxgiCapturer,
};

pub fn select_monitor_dialog(monitors: &[MonitorData]) -> usize {
    // 1. Convert your monitor list into native Windows buttons
    let mut buttons: Vec<TASKDIALOG_BUTTON> = Vec::new();
    let mut button_labels: Vec<Vec<u16>> = Vec::new();

    for (i, mon) in monitors.iter().enumerate() {
        let label = format!(
            "Monitor {}: {}x{} ({})\0",
            i,
            mon.width,
            mon.height,
            if mon.is_primary {
                "Primary"
            } else {
                "Extended"
            }
        );
        let utf16: Vec<u16> = label.encode_utf16().collect();

        buttons.push(TASKDIALOG_BUTTON {
            nButtonID: (i + 100) as i32, // ID to identify which was clicked
            pszButtonText: PCWSTR(utf16.as_ptr()),
        });
        button_labels.push(utf16); // Keep alive until dialog closes
    }

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        pszWindowTitle: w!("AhmedoViewer - Display Selection"),
        pszMainInstruction: w!("Which display would you like to share?"),
        pszContent: w!("Select a monitor to begin the high-speed UDP stream."),
        pButtons: buttons.as_ptr(),
        cButtons: buttons.len() as u32,
        ..Default::default()
    };

    let mut selected_id = 0;
    unsafe {
        TaskDialogIndirect(&config, Some(&mut selected_id), None, None).unwrap();
    }

    // Convert back to index
    (selected_id - 100) as usize
}

fn main() {
    println!("Server starting...");
    config::print_config();
    let transport: TransportMode = TransportMode::from_env();
    let bind_addr = config::bind_addr();

    let displays = get_monitors();

    println!("Detected displays: {:?}", displays);
    if displays.is_empty() {
        eprintln!("No displays detected. Exiting.");
        return;
    }

    let choice: usize = select_monitor_dialog(&displays);
    let selected_display: &MonitorData = &displays[choice];
    println!(
        "Starting stream for: {}x{}",
        selected_display.width, selected_display.height
    );

    /*  let mut capturer = create_capturer_blocking().unwrap(); */
    let (w, h) = (
        selected_display.width as u32,
        selected_display.height as u32,
    );
    let frame_size = (w * h * 4) as usize;
    let shared_frame = Arc::new(ArcSwap::from_pointee(vec![0u8; frame_size]));

    let producer_handle = Arc::clone(&shared_frame);
    let t1 = thread::spawn(move || {
        let selected_display: &MonitorData = &displays[choice];
        //let mut last_log = Instant::now();
        let mut dxgi_capturer = DxgiCapturer::new(selected_display.name.clone());

        let target_fps = 60;
        let frame_duration = Duration::from_micros(1_000_000 / target_fps);

        loop {
            let start_time = Instant::now();
            //let mut frame_size = 0;
            let mut raw_frame =
                vec![0u8; (selected_display.width * selected_display.height * 4) as usize];
            if dxgi_capturer.capture_frame_into(&mut raw_frame) {
                static mut SAVED: bool = false;
                unsafe {
                    if !SAVED {
                        let is_all_zeros = raw_frame.iter().all(|&x| x == 0);
                        if is_all_zeros {
                            println!(
                                "[ERROR] The captured buffer is completely empty (all zeros)!"
                            );
                        } else {
                            let mut png_buffer = raw_frame.clone();
                            // Manual BGRA -> RGBA swap
                            for chunk in png_buffer.chunks_exact_mut(4) {
                                chunk.swap(0, 2);
                            }

                            match image::save_buffer(
                                "server_debug.png",
                                &png_buffer,
                                w as u32,
                                h as u32,
                                image::ExtendedColorType::Rgba8,
                            ) {
                                Ok(_) => {
                                    println!(
                                        "SUCCESS: PNG saved ({} bytes)",
                                        std::fs::metadata("server_debug.png").unwrap().len()
                                    );
                                    SAVED = true;
                                }
                                Err(e) => println!("[ERROR] Failed to save PNG: {:?}", e),
                            }
                            SAVED = true;
                        }
                    }
                }

                //let raw_bytes: &[u8] = bytemuck::cast_slice(&frame);
                // Store the new frame. This is an atomic pointer swap.
                // The old frame will be dropped automatically when no one is reading it.
                //frame_size = raw_bytes.len();
                producer_handle.store(Arc::new(raw_frame));
            } else {
                eprintln!("[t1]: Error capturing frame");
                thread::sleep(Duration::from_secs(1));
            }

            // Maintain 60 FPS rhythm
            let elapsed = start_time.elapsed();
            if elapsed < frame_duration {
                thread::sleep(frame_duration - elapsed);
            }

            // Log capture performance every 5 seconds
            /* if last_log.elapsed() >= Duration::from_secs(15) {
                println!(
                    "[t1] Captured frame at {:?}, size: {} bytes",
                    Instant::now(),
                    frame_size
                );
                last_log = Instant::now();
            } */
        }
    });

    let consumer_handle = Arc::clone(&shared_frame);
    let t2: thread::JoinHandle<()> = thread::spawn(move || {
        let compression = create_frame_compression_from_env();

        // 1400 bytes is a "safe" payload size to stay under MTU
        const MAX_PACKET_SIZE: usize = 1400;
        let mut connection = UdpServerConnection::bind(&bind_addr).unwrap();
        println!("[t2] UDP server listening on {}", bind_addr);

        // --- FPS LOGGING VARIABLES ---
        let mut frames_sent_this_sec = 0;
        let mut last_fps_log = Instant::now();
        let mut last_fps = 0.0;

        loop {
            // Wait for client request (This acts as your "V-Sync")
            if let Err(e) = connection.wait_for_frame_request() {
                eprintln!("[t2] Error waiting for request: {:?}", e);
                continue;
            }
            let latest_snapshot = consumer_handle.load(); // Instant access

            /* println!(
                "[t2] Latest frame size: {} bytes, sending to client...",
                latest_snapshot.len()
            ); */

            let compressed_data = compression.compress(&latest_snapshot).unwrap();

            //Send frame header
            //            connection
            //                .send_frame_header(w, h, 0, latest_snapshot.len() as u32, CompressionKind::None)
            //                .unwrap();

            connection
                .send_frame_header(
                    w,
                    h,
                    latest_snapshot.len() as u32,
                    compressed_data.len() as u32,
                    compression.kind(),
                    last_fps as u32, // Include server FPS in the header
                )
                .unwrap();

            //let chunks = latest_snapshot.chunks(MAX_PACKET_SIZE);
            /* println!(
                "[t2] Sending frame in {} chunks of up to {} bytes each",
                chunks.len(),
                MAX_PACKET_SIZE
            ); */
            let chunks = compressed_data.chunks(MAX_PACKET_SIZE);
            for (i, chunk) in chunks.enumerate() {
                let mut packet = Vec::with_capacity(1404);
                packet.extend_from_slice(&(i as u32).to_le_bytes()); // Chunk Index
                packet.extend_from_slice(chunk);

                connection
                    .socket
                    .send_to(&packet, &connection.peer.unwrap())
                    .unwrap();

                // --- CRITICAL PERFORMANCE TWEAK ---
                // If you are sending 1440p raw, the server is "too fast".
                // Yielding every 200 packets helps the client's OS buffer survive.
                if i % 200 == 0 {
                    thread::yield_now();
                }
            }

            frames_sent_this_sec += 1;
            if last_fps_log.elapsed() >= Duration::from_secs(1) {
                let elapsed = last_fps_log.elapsed().as_secs_f32();
                last_fps = frames_sent_this_sec as f32 / elapsed;
                /* println!(
                    "[t2] Server Performance: {:.2} FPS sent to {:?}",
                    last_fps,
                    connection.peer.unwrap()
                ); */
                frames_sent_this_sec = 0;
                last_fps_log = Instant::now();
            }
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
}

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

fn run_udp_server(bind_addr: &str) -> std::io::Result<()> {
    let mut connection = UdpServerConnection::bind(bind_addr)?;
    let frame_interval = Duration::from_millis(config::frame_interval_ms());
    let mut frames_sent: u64 = 0;
    let mut last_log = Instant::now();
    // 1400 bytes is a "safe" payload size to stay under MTU
    const MAX_PACKET_SIZE: usize = 1400;
    println!("UDP server listening on {}", bind_addr);

    loop {}
    Ok(())
}
