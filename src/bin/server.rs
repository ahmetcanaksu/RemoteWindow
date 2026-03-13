use RemoteWindow::capture::{create_default_capturer, ScreenCapturer};
use RemoteWindow::compression::{create_frame_compression_from_env, FrameCompression};
use RemoteWindow::config;
use RemoteWindow::connection::{
    bind_tcp_listener, ServerConnection, TcpServerConnection, TransportMode, UdpServerConnection,
};
use std::{
    mem,
    thread,
    time::{Duration, Instant},
};

const TCP_CHUNK_BYTES: usize = 65_534; // just under u16::MAX
const UDP_CHUNK_BYTES: usize = 1_200; // safe payload to avoid IP fragmentation

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

        let payload = compression.compress(raw_frame)?;
        connection.send_frame_header(
            w,
            h,
            pixel_count,
            payload.len() as u32,
            compression.kind(),
        )?;

        for chunk in payload.chunks(TCP_CHUNK_BYTES) {
            connection.send_chunk(chunk)?;
        }

        // pace: sleep only the remaining time in this frame's interval
        let elapsed = frame_start.elapsed();
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }

        frames_sent += 1;
        if last_log.elapsed().as_secs_f32() >= 5.0 {
            println!(
                "[server] peer={} streaming ok: {} fps, frame={}x{}, pixels={}, codec={}, payload={} bytes",
                connection.peer_label(),
                frames_sent,
                w,
                h,
                pixel_count,
                compression.kind().name(),
                payload.len()
            );
            frames_sent = 0;
            last_log = std::time::Instant::now();
        }
    }
}

fn run_tcp_server(bind_addr: &str) -> std::io::Result<()> {
    let mut capturer = create_capturer_blocking()?;
    let compression = create_frame_compression_from_env();
    let listener = bind_tcp_listener(bind_addr)?;
    println!("TCP server listening on {}", bind_addr);
    println!("[server] frame compression: {}", compression.kind().name());
    println!("[server] waiting for incoming TCP connections...");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let mut connection = TcpServerConnection::new(stream);
                println!("New connection: {}", connection.peer_label());
                let (w, h) = capturer.geometry();
                println!(
                    "[server] starting stream for {} with geometry {}x{}",
                    connection.peer_label(),
                    w,
                    h
                );

                match handle_connection(&mut connection, capturer.as_mut(), compression.as_ref(), (w, h)) {
                    Ok(_) => println!("Connection closed: {}", connection.peer_label()),
                    Err(e) => println!(
                        "[server] stream ended for {}: {}",
                        connection.peer_label(),
                        e
                    ),
                }

                capturer = create_capturer_blocking()?;
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }

    Ok(())
}

fn run_udp_server(bind_addr: &str) -> std::io::Result<()> {
    let mut capturer = create_capturer_blocking()?;
    let compression = create_frame_compression_from_env();
    let mut connection = UdpServerConnection::bind(bind_addr)?;
    let frame_interval = Duration::from_millis(config::frame_interval_ms());
    let mut frames_sent: u64 = 0;
    let mut last_log = Instant::now();
    let mut raw_frame_scratch = Vec::new();

    println!("UDP server listening on {}", bind_addr);
    println!("[server] frame compression: {}", compression.kind().name());
    loop {
        println!("[server] waiting for UDP client registration...");
        connection.wait_for_frame_request()?;
        println!("[server] UDP client registered: {}", connection.peer_label());

        loop {
            let frame_start = Instant::now();
            let (w, h) = capturer.geometry();

            let pixels = match capturer.capture_frame() {
                Ok(frame) => frame,
                Err(e) => {
                    println!("Capture failed: {:?}", e);
                    capturer = create_capturer_blocking()?;
                    break;
                }
            };
            let pixel_count = pixels.len() as u32;
            let raw_frame = pixels_as_le_bytes(&pixels, &mut raw_frame_scratch);
            let payload = compression.compress(raw_frame)?;

            if let Err(e) = connection.send_frame_header(
                w,
                h,
                pixel_count,
                payload.len() as u32,
                compression.kind(),
            ) {
                println!("[server] UDP header send failed for {}: {}", connection.peer_label(), e);
                break;
            }

            let mut send_failed = false;
            for chunk in payload.chunks(UDP_CHUNK_BYTES) {
                if let Err(e) = connection.send_chunk(chunk) {
                    println!(
                        "[server] UDP chunk send failed for {}: {}",
                        connection.peer_label(),
                        e
                    );
                    send_failed = true;
                    break;
                }
            }

            if send_failed {
                break;
            }

            frames_sent += 1;
            if last_log.elapsed().as_secs_f32() >= 5.0 {
                println!(
                    "[server] peer={} streaming ok: {} fps, frame={}x{}, pixels={}, codec={}, payload={} bytes",
                    connection.peer_label(),
                    frames_sent,
                    w,
                    h,
                    pixel_count,
                    compression.kind().name(),
                    payload.len()
                );
                frames_sent = 0;
                last_log = Instant::now();
            }

            let elapsed = frame_start.elapsed();
            if elapsed < frame_interval {
                thread::sleep(frame_interval - elapsed);
            }
        }
    }
}

fn main() {
    config::print_config();
    let transport = TransportMode::from_env();
    let bind_addr = config::bind_addr();

    let result = match transport {
        TransportMode::Tcp => run_tcp_server(&bind_addr),
        TransportMode::Udp => run_udp_server(&bind_addr),
        TransportMode::Both => {
            let udp_bind_addr = bind_addr.clone();
            let udp_thread = thread::spawn(move || run_udp_server(&udp_bind_addr));
            let tcp_result = run_tcp_server(&bind_addr);
            let _ = udp_thread.join();
            tcp_result
        }
    };

    if let Err(e) = result {
        eprintln!("Server exited with error: {}", e);
        std::process::exit(1);
    }
}
