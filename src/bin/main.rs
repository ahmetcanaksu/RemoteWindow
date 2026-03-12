use core::time::Duration;
use std::fs::File;
use std::io::ErrorKind::WouldBlock;
use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::{thread, time};

/*
fn handle_client(mut stream: TcpStream) {
    let mut capturer = Capturer::new(0).unwrap();
    let (w, h) = capturer.geometry();
    let size = w as u64 * h as u64;
    println!("START: {}x{}", w, h);
    'test: loop {
        let (mut x, mut y) = (1, 1);
        let ps = capturer.capture_frame().unwrap();
        let (mut tot_r, mut tot_g, mut tot_b) = (0, 0, 0);

        for Bgr8 { r, g, b, .. } in ps.into_iter() {
            if x == 256 {
                if y == 128 {
                    //println!("OK, {}x{}", x, y);
                    let to_be_sent = format!("{},{},{}\n", r, g, b);
                    let c = stream.write_all(to_be_sent.as_bytes());
                    if let Ok(_) = c {
                        //println!("SENT {}", to_be_sent);
                    } else if let Err(_) = c {
                        stream.shutdown(Shutdown::Both).unwrap();
                        break 'test;
                        println!("ERR");
                    }
                    y = 1;
                    x = 1;
                } else {
                    //println!("X COMPLETE {}x{}", x, y);
                    y += 1;
                    x = 1;
                }
            } else {
                x += 1;
            }
            tot_r += r as u64;
            tot_g += g as u64;
            tot_b += b as u64;
        }
        tot_r = tot_r / size;
        tot_g = tot_g / size;
        tot_b = tot_b / size;
        //thread::sleep(Duration::from_millis(10));
    }
}
*/

fn main() {
    println!("Starting server...");
    let displays = scrap::Display::all().unwrap();
    println!("Available displays:");
    for (i, display) in displays.iter().enumerate() {
        println!("{}: {}x{}", i, display.width(), display.height());
    }
    let display = scrap::Display::primary().unwrap();
    let mut inner = scrap::Capturer::new(display).unwrap();
    let frame = loop {
        match inner.frame() {
            Ok(frame) => break frame,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => panic!("capture error: {}", e),
        }
    };
    println!("Capturer created for primary display: {}x{}", frame.len(), frame.len());
}
