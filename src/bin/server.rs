use RemoteWindow::capture::{create_default_capturer, ScreenCapturer};
use RemoteWindow::connection::{
    bind_tcp_listener, ServerConnection, TcpServerConnection, TransportMode, UdpServerConnection,
    DEFAULT_ADDR,
};
use std::{
    env,
    thread,
    time::Duration,
};

const CHUNK_SIZE: usize = 600;
const CHUNK_BYTES: usize = CHUNK_SIZE * 4;

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
    (w, h): (u32, u32),
) -> std::io::Result<()> {
    let mut frames_sent: u64 = 0;
    let mut last_log = std::time::Instant::now();

    loop {
        connection.wait_for_frame_request()?;
        thread::sleep(Duration::from_millis(80));

        let pixels = capturer.capture_frame()?;
        let pixel_count = pixels.len() as u32;
        connection.send_frame_header(w, h, pixel_count)?;

        // Stream exact byte chunks. TCP adds length prefix in transport layer.
        let mut chunk = Vec::with_capacity(CHUNK_BYTES);

        for rgb in pixels {
            chunk.extend_from_slice(&rgb.to_le_bytes());

            if chunk.len() == CHUNK_BYTES {
                connection.send_chunk(&chunk)?;
                chunk.clear();
            }
        }

        if !chunk.is_empty() {
            connection.send_chunk(&chunk)?;
        }

        frames_sent += 1;
        if last_log.elapsed().as_secs_f32() >= 1.0 {
            println!(
                "[server] peer={} streaming ok: {} fps, frame={}x{}, pixels={}",
                connection.peer_label(),
                frames_sent,
                w,
                h,
                pixel_count
            );
            frames_sent = 0;
            last_log = std::time::Instant::now();
        }
    }
}

fn run_tcp_server(bind_addr: &str) -> std::io::Result<()> {
    let mut capturer = create_capturer_blocking()?;
    let listener = bind_tcp_listener(bind_addr)?;
    println!("TCP server listening on {}", bind_addr);
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

                match handle_connection(&mut connection, capturer.as_mut(), (w, h)) {
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
    let mut connection = UdpServerConnection::bind(bind_addr)?;

    println!("UDP server listening on {}", bind_addr);
    loop {
        connection.wait_for_frame_request()?;
        let (w, h) = capturer.geometry();

        let pixels = match capturer.capture_frame() {
            Ok(frame) => frame,
            Err(e) => {
                println!("Capture failed: {:?}", e);
                capturer = create_capturer_blocking()?;
                continue;
            }
        };
        connection.send_frame_header(w, h, pixels.len() as u32)?;

        let mut chunk = Vec::with_capacity(CHUNK_BYTES);

        for rgb in pixels {
            chunk.extend_from_slice(&rgb.to_le_bytes());

            if chunk.len() == CHUNK_BYTES {
                connection.send_chunk(&chunk)?;
                chunk.clear();
            }
        }

        if !chunk.is_empty() {
            connection.send_chunk(&chunk)?;
        }
    }
}

fn main() {
    let transport = TransportMode::from_env();
    let bind_addr = env::var("RW_BIND_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());

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
