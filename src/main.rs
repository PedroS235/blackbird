#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;

use blackbird::{analysis, logging, parser, signal};
use logging::init_logging;

fn main() {
    init_logging();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/blackbird-icon.png"))
        .expect("The icon data must be valid");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_icon(icon),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface: eframe::egui_wgpu::SurfaceConfig {
                present_mode: eframe::wgpu::PresentMode::AutoNoVsync,
                desired_maximum_frame_latency: None,
            },
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        "Blackbird",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            app::theme::install(&cc.egui_ctx);
            Ok(Box::<app::BlackbirdApp>::default())
        }),
    )
    .unwrap();
}
