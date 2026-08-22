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
                present_mode: present_mode(std::env::var("BLACKBIRD_PRESENT").ok().as_deref()),
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

/// How frames reach the screen. Vsync by default, deliberately.
///
/// `AutoNoVsync` renders as fast as the GPU allows, which on Windows means
/// DX12 honouring `Immediate`: the app then presents thousands of frames a
/// second, and the resulting queue starves the compositor badly enough to
/// freeze the whole desktop, not just this window. Measured at 900 frames per
/// arm, DX12 was the only backend it happened on — Vulkan uncapped was fine,
/// and so was DX12 with vsync. A log analyser has nothing to gain from
/// presenting faster than the monitor can show, so the cap is the fix.
///
/// `BLACKBIRD_PRESENT` overrides it, which is how the above was measured and
/// how the next such report can be narrowed without a rebuild. A mode the
/// surface does not support panics at configure time — Wayland/RADV offers
/// only `Mailbox` and `Fifo`, so `immediate` dies there — which is acceptable
/// for a debugging switch and is the reason the default is not one of them.
fn present_mode(setting: Option<&str>) -> eframe::wgpu::PresentMode {
    use eframe::wgpu::PresentMode::*;
    match setting {
        Some("novsync") => AutoNoVsync,
        Some("fifo") => Fifo,
        Some("mailbox") => Mailbox,
        Some("immediate") => Immediate,
        _ => AutoVsync,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use eframe::wgpu::PresentMode;

    /// The regression this file exists to hold: an unset override must not
    /// leave the app presenting uncapped, which froze the desktop on Windows.
    #[test]
    fn default_present_mode_is_vsynced() {
        assert_eq!(present_mode(None), PresentMode::AutoVsync);
        assert_eq!(present_mode(Some("")), PresentMode::AutoVsync);
        assert_eq!(present_mode(Some("nonsense")), PresentMode::AutoVsync);
    }

    #[test]
    fn override_reaches_every_mode() {
        assert_eq!(present_mode(Some("novsync")), PresentMode::AutoNoVsync);
        assert_eq!(present_mode(Some("fifo")), PresentMode::Fifo);
        assert_eq!(present_mode(Some("mailbox")), PresentMode::Mailbox);
        assert_eq!(present_mode(Some("immediate")), PresentMode::Immediate);
    }
}
