#![allow(dead_code)]

mod core;
mod errors;
mod gui;
mod models;
mod queue;
mod summarizer;
mod worker;

use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    tracing::info!("Klartext-Rust starting up");

    // Load app icon from embedded PNG bytes
    let icon_data = include_bytes!("../assets/klartext_icon_64.png");
    let icon_image = image::load_from_memory(icon_data)
        .expect("Failed to load app icon")
        .to_rgba8();
    let (icon_width, icon_height) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width: icon_width,
        height: icon_height,
    };

    // Configure eframe native window options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Klartext")
            .with_inner_size(egui::vec2(700.0, 800.0))
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };

    // Launch the eframe application with the KlartextApp GUI
    eframe::run_native(
        "Klartext",
        native_options,
        Box::new(|cc| Ok(Box::new(gui::KlartextApp::new(cc)))),
    )
}
