#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;

use blackbird::{analysis, loader, logging, parser, signal, version};
use logging::init_logging;

fn main() {
    init_logging();
    tracing::info!(
        "Blackbird v{} starting on {}/{}",
        version::CURRENT,
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/blackbird_logo.png"))
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

    let run = eframe::run_native(
        "Blackbird",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            app::theme::install(&cc.egui_ctx);
            Ok(Box::new(app::BlackbirdApp::new(cc)))
        }),
    );

    match run {
        Ok(()) => tracing::info!("Blackbird exited"),
        // The window never came up, or the event loop died. Nothing is left to
        // show it in, so the log is the only place this can be said.
        Err(err) => tracing::error!("Blackbird could not run: {err}"),
    }
}
