use eframe::egui::{CentralPanel, SidePanel, ViewportBuilder};

struct ClientApp;

impl ClientApp {
    fn new() -> Self {
        Self
    }
}

impl eframe::App for ClientApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Remote Window Server v2");
            ui.label("This is a client application that connects to the remote window server.");
        });

        SidePanel::right("right_panel")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Remote Window Server v2");
                ui.label("This is a client application that connects to the remote window server.");
            });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([980.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Remote Window Server v2",
        options,
        Box::new(|_cc| Ok(Box::new(ClientApp::new()))),
    )
}
