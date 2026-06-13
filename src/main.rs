#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod analysis;
mod app;
mod logging;
mod parser;
mod signal;
mod ui;

use crate::logging::init_logging;

fn main() {
    init_logging();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/blackbird-icon.png"))
        .expect("The icon data must be valid");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_icon(icon),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            present_mode: eframe::wgpu::PresentMode::AutoNoVsync,
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        "Blackbird",
        options,
        Box::new(|_cc| Ok(Box::<app::App>::default())),
    )
    .unwrap();
}
