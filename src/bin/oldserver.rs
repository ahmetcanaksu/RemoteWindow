use captrs::{Bgr8, Capturer};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

const CHUNK_SIZE: usize = 600;

fn handle_connection(
    stream: &mut TcpStream,
    capturer: &mut Capturer,
    (w, h): (u32, u32),
) -> std::io::Result<()> {
    let mut waiting_package_direct = true;

    loop {
        // Wait until new frame requested.
        if waiting_package_direct {
            let mut buffer = [0_u8; 1];
            stream.read(&mut buffer)?;
            if buffer[0] == 0x88 {
                thread::sleep(Duration::from_millis(80));
                waiting_package_direct = false;
            }
        } else {
            //Write dimension sending info.
            stream.write_all(&[0x33, 0x34, 0x35, 0x36])?;
            stream.flush().unwrap();
            // Send frame dimensions.
            stream.write_all(&w.to_le_bytes())?;
            stream.write_all(&h.to_le_bytes())?;
            stream.flush().unwrap();

            if let Ok(ps) = capturer.capture_frame() {
                // Create chunks and pointer
                let mut chunks: (usize, &mut [u8; CHUNK_SIZE * 4]) = (0, &mut [0; CHUNK_SIZE * 4]);

                for Bgr8 { r, g, b, .. } in &ps {
                    let rgb = (*r as u32) << 16 | (*g as u32) << 8 | (*b as u32);
                    //Write chunk as pointer offset increased.
                    chunks.1[(chunks.0 * 4)..((chunks.0 * 4) + 4)]
                        .copy_from_slice(&rgb.to_le_bytes());
                    chunks.0 += 1;

                    // If chunk size reached send the buffer.
                    if chunks.0 == CHUNK_SIZE {
                        stream.write_all(chunks.1)?;
                        stream.flush()?;
                        chunks.0 = 0;
                        chunks.1.fill(0);
                    }
                }
            }
            waiting_package_direct = true;
        }
    }
}

fn main() {
    let mut capturer = Capturer::new(0).unwrap();
    let (w, h) = capturer.geometry();

    //Create tcp server
    let listener = TcpListener::bind("127.0.0.1:82").unwrap();
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("New connection: {}", stream.peer_addr().unwrap());
                if handle_connection(&mut stream, &mut capturer, (w, h)).is_err() {
                    println!("Connection closed: {}", stream.peer_addr().unwrap());
                } else {
                    println!("Connection closed: {}", stream.peer_addr().unwrap());
                }
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}
