use std::{
    io::BufWriter,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use remote_window::{
    capture::{create_default_capturer, ScreenCapturer},
    compression::{create_frame_compression_from_env, CompressionKind},
    config,
    connection::{
        bind_tcp_listener, ServerConnection, TcpServerConnection, TransportMode,
        UdpServerConnection,
    },
};

fn main() {
    println!("Server starting...");
    config::print_config();
    let transport: TransportMode = TransportMode::from_env();
    let bind_addr = config::bind_addr();

    let mut capturer = create_capturer_blocking().unwrap();
    let compression = create_frame_compression_from_env();
    let mut capturer = create_capturer_blocking().unwrap();
    let (w, h) = capturer.geometry();
    let frame_size = (w * h * 4) as usize;
    let shared_frame = Arc::new(ArcSwap::from_pointee(vec![0u8; frame_size]));
    drop(capturer);

    let producer_handle = Arc::clone(&shared_frame);
    let t1 = thread::spawn(move || {
        //let mut last_log = Instant::now();
        let mut capturer = create_capturer_blocking().unwrap();
        let target_fps = 60;
        let frame_duration = Duration::from_micros(1_000_000 / target_fps);

        loop {
            let start_time = Instant::now();
            //let mut frame_size = 0;
            match capturer.capture_frame() {
                Ok(frame) => {
                    let raw_bytes: &[u8] = bytemuck::cast_slice(&frame);
                    // Store the new frame. This is an atomic pointer swap.
                    // The old frame will be dropped automatically when no one is reading it.
                    //frame_size = raw_bytes.len();
                    producer_handle.store(Arc::new(raw_bytes.to_vec()));
                }
                Err(e) => {
                    eprintln!("[t1]: Error capturing frame: {:?}", e);
                    thread::sleep(Duration::from_secs(1));
                }
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
        // 1400 bytes is a "safe" payload size to stay under MTU
        const MAX_PACKET_SIZE: usize = 1400;
        let mut connection = UdpServerConnection::bind(&bind_addr).unwrap();
        println!("[t2] UDP server listening on {}", bind_addr);

        loop {
            connection.wait_for_frame_request().unwrap();
            println!(
                "[t2] Received frame request from client: {:?}",
                connection.peer
            );
            let latest_snapshot = consumer_handle.load(); // Instant access

            println!(
                "[t2] Latest frame size: {} bytes, sending to client...",
                latest_snapshot.len()
            );

            //Send frame header
            connection
                .send_frame_header(w, h, 0, latest_snapshot.len() as u32, CompressionKind::None)
                .unwrap();

            let chunks = latest_snapshot.chunks(MAX_PACKET_SIZE);
            println!(
                "[t2] Sending frame in {} chunks of up to {} bytes each",
                chunks.len(),
                MAX_PACKET_SIZE
            );
            for (i, chunk) in chunks.enumerate() {
                let mut packet = Vec::with_capacity(1404);
                packet.extend_from_slice(&(i as u32).to_le_bytes()); // Chunk Index
                packet.extend_from_slice(chunk);

                connection.socket.send_to(&packet, &connection.peer.unwrap()).unwrap();
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
