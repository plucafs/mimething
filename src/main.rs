#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod categories;
mod desktop;
mod mime;
mod settings;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Default Apps Editor"),
        ..Default::default()
    };
    eframe::run_native(
        "Default Apps Editor",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
