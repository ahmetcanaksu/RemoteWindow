use std::{
    convert::TryInto,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use minifb::{Key, Scale, ScaleMode, Window, WindowOptions};
use remote_window::{
    config,
    connection::{ClientConnection, UdpClientConnection, UdpServerConnection},
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

    // 2. Clone the Arc for the first thread
    let t1_state = Arc::clone(&state);
    // Connection thread
    let t1 = thread::spawn(move || {
        let server_addr = config::server_addr();
        let mut connection = UdpClientConnection::connect(&server_addr).unwrap();
        connection.request_frame().unwrap();
        connection
            .socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let frame_header = connection.read_frame_header().unwrap();
        println!(
            "[t1] Received frame header: width={}, height={}, compression={:?}",
            frame_header.width, frame_header.height, frame_header.compression
        );

        if t1_state.open_display.load(Ordering::SeqCst) == false {
            t1_state
                .width
                .store(frame_header.width as usize, Ordering::SeqCst);
            t1_state
                .height
                .store(frame_header.height as usize, Ordering::SeqCst);
            t1_state.open_display.store(true, Ordering::SeqCst);
        }

        let mut frame_buffer = vec![0u8; (frame_header.width * frame_header.height * 4) as usize];
        let mut bytes_received = 0;
        let total_expected = frame_header.payload_len as usize;

        while bytes_received < total_expected {
            let mut packet_buf = [0u8; 1404]; // 4 bytes index + 1400 bytes data
            let (amt, _) = connection
                .socket
                .recv_from(&mut packet_buf)
                .unwrap_or_else(|_| {
                    panic!(
                        "[t1] Failed to receive packet after receiving {} bytes, expected {} bytes",
                        bytes_received, total_expected
                    )
                });

            if amt > 4 {
                let chunk_index = u32::from_le_bytes(packet_buf[0..4].try_into().unwrap()) as usize;
                if (chunk_index % 100) == 0 {
                    println!(
                        "[t1] Received chunk index: {}, size: {} bytes",
                        chunk_index,
                        amt - 4
                    );
                }

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
        println!("[t1] Frame fully reassembled!");
        loop {}
    });

    let t2_state = Arc::clone(&state);
    //Render thread
    let t2 = thread::spawn(move || {
        if t2_state.open_display.load(Ordering::SeqCst) == false {
            println!("[t2] Waiting for frame header to determine screen dimensions...");
            while t2_state.open_display.load(Ordering::SeqCst) == false {
                thread::sleep(Duration::from_millis(50));
            }
        }

        let width = t2_state.width.load(Ordering::SeqCst);
        let height = t2_state.height.load(Ordering::SeqCst);
        println!(
            "[t2] Frame header received, opening window with dimensions: {}x{}",
            width, height
        );

        let mut window = Window::new(
            "Test - ESC to exit",
            1920,
            1080,
            WindowOptions {
                scale: Scale::FitScreen,
                scale_mode: minifb::ScaleMode::UpperLeft,
                resize: true,
                ..Default::default()
            },
        )
        .unwrap();

        let mut present_buffer = vec![0_u32; 1920 * 1080];

        while window.is_open() && !window.is_key_down(Key::Escape) {
            // In a real implementation, we would read from a shared buffer updated by the connection thread
            // For this example, we just display a blank screen
            /*    window
            .update_with_buffer(
                &vec![0u32; width * height],
                width,
                height,
            )
            .unwrap(); */

            window
                .update_with_buffer(&present_buffer, width, height)
                .unwrap();
            window.update();
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
}
