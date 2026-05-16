mod analysis;
mod app;
mod parser;
mod ui;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/blackbird-icon.png"))
        .expect("The icon data must be valid");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_icon(icon),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            // AutoNoVsync avoids GPU timeouts when the Wayland compositor stops
            // presenting frames (e.g. window in background / occluded).
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
