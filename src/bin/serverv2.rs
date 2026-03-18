use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use eframe::egui::{self, Color32, RichText, Stroke};
use remote_window::{
    capture::{create_default_capturer, DisplayInfo, ScreenCapturer},
    compression::create_frame_compression_from_env,
    config,
    connection::{ServerConnection, UdpServerConnection},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use remote_window::capture::get_monitor_list;

#[cfg(target_os = "macos")]
use remote_window::macos::SwiftCapturer;

struct ServerApp {
    displays: Vec<DisplayInfo>,
    selected_index: Option<usize>,
    bind_addr: String,
    server_started: Arc<AtomicBool>,
    status: String,
}

impl ServerApp {
    fn new() -> Self {
        let displays = available_displays();
        let selected_index = if displays.is_empty() { None } else { Some(0) };

        Self {
            displays,
            selected_index,
            bind_addr: config::bind_addr(),
            server_started: Arc::new(AtomicBool::new(false)),
            status: "Choose a display and press Start Server".to_string(),
        }
    }
}

impl eframe::App for ServerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(RichText::new("Remote Window Server v2").size(28.0));
            ui.add_space(8.0);
            ui.label("Select the display you want to stream");
            ui.add_space(12.0);

            ui.horizontal_wrapped(|ui| {
                for (i, display) in self.displays.iter().enumerate() {
                    let is_selected = self.selected_index == Some(i);
                    let fill = if is_selected {
                        Color32::from_rgb(20, 100, 180)
                    } else {
                        Color32::from_rgb(33, 36, 44)
                    };

                    let stroke = if is_selected {
                        Stroke::new(2.0, Color32::from_rgb(80, 180, 255))
                    } else {
                        Stroke::new(1.0, Color32::from_rgb(60, 65, 76))
                    };

                    let mut clicked = false;
                    egui::Frame::group(ui.style())
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(8.0)
                        .show(ui, |ui| {
                            ui.set_min_width(220.0);
                            ui.set_min_height(120.0);
                            ui.vertical_centered(|ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(format!("Display {}", i + 1))
                                        .strong()
                                        .size(18.0)
                                        .color(Color32::WHITE),
                                );
                                ui.label(
                                    RichText::new(format!("{} x {}", display.width, display.height))
                                        .color(Color32::from_rgb(210, 220, 230)),
                                );
                                ui.label(
                                    RichText::new(format!("ID: {}", display.id))
                                        .small()
                                        .color(Color32::from_rgb(180, 190, 205)),
                                );
                                ui.add_space(6.0);
                                if ui.button("Select").clicked() {
                                    clicked = true;
                                }
                            });
                        });

                    if clicked {
                        self.selected_index = Some(i);
                    }
                }
            });

            if self.displays.is_empty() {
                ui.add_space(10.0);
                ui.colored_label(Color32::from_rgb(255, 120, 120), "No displays were detected on this platform.");
            }

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(format!("Bind address: {}", self.bind_addr));

            let can_start = !self.server_started.load(Ordering::SeqCst)
                && self.selected_index.is_some()
                && !self.displays.is_empty();

            if ui
                .add_enabled(can_start, egui::Button::new(RichText::new("Start Server").size(18.0)))
                .clicked()
            {
                let selected = self.selected_index.unwrap();
                let selected_display = self.displays[selected];
                let bind_addr = self.bind_addr.clone();
                let started_flag = Arc::clone(&self.server_started);

                started_flag.store(true, Ordering::SeqCst);
                self.status = format!(
                    "Server running on {} and streaming Display {} ({}x{})",
                    bind_addr,
                    selected + 1,
                    selected_display.width,
                    selected_display.height
                );

                thread::spawn(move || {
                    if let Err(e) = run_server(bind_addr, selected, selected_display) {
                        eprintln!("[serverv2] server stopped with error: {:?}", e);
                    }
                });
            }

            ui.add_space(10.0);
            if self.server_started.load(Ordering::SeqCst) {
                ui.colored_label(Color32::from_rgb(80, 220, 130), &self.status);
            } else {
                ui.label(&self.status);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    println!("Server v2 starting...");
    config::print_config();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Remote Window Server v2",
        options,
        Box::new(|_cc| Ok(Box::new(ServerApp::new()))),
    )
}

fn available_displays() -> Vec<DisplayInfo> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        return get_monitor_list();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

fn run_server(
    bind_addr: String,
    selected_display_index: usize,
    selected_display: DisplayInfo,
) -> std::io::Result<()> {
    let (w, h) = (selected_display.width as u32, selected_display.height as u32);
    let frame_size = (w * h * 4) as usize;
    let shared_frame = Arc::new(ArcSwap::from_pointee(vec![0u8; frame_size]));

    let producer_handle = Arc::clone(&shared_frame);
    let capture_thread = thread::spawn(move || {
        let mut last_log = Instant::now();
        let target_fps = 60;
        let frame_duration = Duration::from_micros(1_000_000 / target_fps);

        let mut capturer = create_capturer_blocking_for_display(selected_display_index).unwrap();

        loop {
            let start_time = Instant::now();

            match capturer.capture_frame() {
                Ok(frame) => {
                    let raw_frame: &[u8] = bytemuck::cast_slice(&frame);
                    producer_handle.store(Arc::new(raw_frame.to_vec()));
                }
                Err(e) => {
                    eprintln!("[capture] error capturing frame: {:?}", e);
                    continue;
                }
            }

            let elapsed = start_time.elapsed();
            if elapsed < frame_duration {
                thread::sleep(frame_duration - elapsed);
            }

            if last_log.elapsed() >= Duration::from_secs(15) {
                println!("[capture] streaming {}x{}", w, h);
                last_log = Instant::now();
            }
        }
    });

    let consumer_handle = Arc::clone(&shared_frame);
    let sender_thread = thread::spawn(move || {
        let compression = create_frame_compression_from_env();
        const MAX_PACKET_SIZE: usize = 1400;
        let mut connection = UdpServerConnection::bind(&bind_addr).unwrap();
        println!("[sender] UDP server listening on {}", bind_addr);

        let mut frames_sent_this_sec = 0;
        let mut last_fps_log = Instant::now();
        let mut last_fps = 0.0;

        loop {
            if let Err(e) = connection.wait_for_frame_request() {
                eprintln!("[sender] error waiting for request: {:?}", e);
                continue;
            }

            let latest_snapshot = consumer_handle.load();
            let compressed_data = compression.compress(&latest_snapshot).unwrap();

            connection
                .send_frame_header(
                    w,
                    h,
                    latest_snapshot.len() as u32,
                    compressed_data.len() as u32,
                    compression.kind(),
                    last_fps as u32,
                )
                .unwrap();

            let chunks = compressed_data.chunks(MAX_PACKET_SIZE);
            for (i, chunk) in chunks.enumerate() {
                let mut packet = Vec::with_capacity(1404);
                packet.extend_from_slice(&(i as u32).to_le_bytes());
                packet.extend_from_slice(chunk);

                connection
                    .socket
                    .send_to(&packet, &connection.peer.unwrap())
                    .unwrap();

                if i % 200 == 0 {
                    thread::yield_now();
                }
            }

            frames_sent_this_sec += 1;
            if last_fps_log.elapsed() >= Duration::from_secs(1) {
                let elapsed = last_fps_log.elapsed().as_secs_f32();
                last_fps = frames_sent_this_sec as f32 / elapsed;
                frames_sent_this_sec = 0;
                last_fps_log = Instant::now();
            }
        }
    });

    let _ = capture_thread.join();
    let _ = sender_thread.join();
    Ok(())
}

fn create_capturer_blocking_for_display(
    display_index: usize,
) -> std::io::Result<Box<dyn ScreenCapturer>> {
    loop {
        println!("[serverv2] initializing screen capturer...");

        #[cfg(target_os = "macos")]
        {
            match SwiftCapturer::new_with_display_index(display_index as u32) {
                Ok(capturer) => {
                    println!("[serverv2] screen capturer initialized");
                    let (w, h) = capturer.geometry();
                    println!("[serverv2] capturer geometry {}x{}", w, h);
                    return Ok(Box::new(capturer));
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(e);
                }
                Err(e) => {
                    println!("[serverv2] failed to create capturer: {:?}", e);
                    println!("[serverv2] retrying in 1s");
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            match create_default_capturer() {
                Ok(capturer) => {
                    println!("[serverv2] screen capturer initialized");
                    let (w, h) = capturer.geometry();
                    println!("[serverv2] capturer geometry {}x{}", w, h);
                    return Ok(capturer);
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(e);
                }
                Err(e) => {
                    println!("[serverv2] failed to create capturer: {:?}", e);
                    println!("[serverv2] retrying in 1s");
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        }
    }
}
