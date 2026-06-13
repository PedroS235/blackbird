use std::ops::RangeInclusive;
use std::sync::mpsc::{self, Receiver};

use crate::analysis::{self, AnalysisResult};
use crate::parser::{self, ParsedLog};
use crate::ui::panels::log_info;
use crate::ui::panels::spectral::SpectralPanel;
use crate::ui::panels::step_response::StepResponsePanel;
use crate::ui::panels::timeseries::TimeseriesPanel;

struct LoadedLog {
    parsed: ParsedLog,
    analysis: AnalysisResult,
}

pub struct App {
    logs: Vec<LoadedLog>,
    active_log: usize,
    pub plot_state: PlotState,
    active_panel: ActivePanel,
    timeseries_panel: TimeseriesPanel,
    spectral_panel: SpectralPanel,
    step_response_panel: StepResponsePanel,
    load_state: LoadState,
    loading_frames: usize,
    error: Option<String>,
}

#[derive(PartialEq, Default)]
enum ActivePanel {
    #[default]
    Timeseries,
    Spectral,
    StepResponse,
}

enum LoadEvent {
    LogReady(LoadedLog),
    Progress(usize),
    Failed(String),
}

enum LoadState {
    Idle,
    Loading {
        rx: Receiver<LoadEvent>,
        expected: usize,
    },
}

pub struct PlotState {
    pub time_range: RangeInclusive<f64>,
    pub zoom: f64,
    pub cursor_time: Option<f64>,
}

impl Default for PlotState {
    fn default() -> Self {
        Self {
            time_range: 0.0..=1.0,
            zoom: 1.0,
            cursor_time: None,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        tracing::info!("Starting application");
        Self {
            logs: Vec::new(),
            active_log: 0,
            plot_state: PlotState::default(),
            active_panel: ActivePanel::default(),
            timeseries_panel: TimeseriesPanel::default(),
            spectral_panel: SpectralPanel::default(),
            step_response_panel: StepResponsePanel::default(),
            load_state: LoadState::Idle,
            loading_frames: 0,
            error: None,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_load(ui.ctx());

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let (loading, expected) = match &self.load_state {
                    LoadState::Loading { expected, .. } => (true, *expected),
                    LoadState::Idle => (false, 0),
                };
                let loaded_count = self.logs.len();
                if loading {
                    ui.add(egui::Spinner::new());
                    let frames = self.loading_frames;
                    if frames > 0 {
                        ui.label(format!(
                            "Loading… {loaded_count}/{expected} — {frames} frames"
                        ));
                    } else {
                        ui.label(format!("Loading… {loaded_count}/{expected}"));
                    }
                } else if ui.button("Open Log").clicked() {
                    self.pick_file();
                }
                if let Some(log) = self.logs.get(self.active_log) {
                    ui.separator();
                    ui.label(&log.parsed.metadata.craft_name);
                    ui.label(&log.parsed.metadata.firmware);
                }
                if let Some(err) = &self.error {
                    ui.separator();
                    ui.colored_label(egui::Color32::RED, err);
                }
            });
        });

        if self.logs.len() > 1 {
            egui::Panel::left("log_switcher")
                .resizable(false)
                .default_size(160.0)
                .show_inside(ui, |ui| {
                    ui.heading("Logs");
                    ui.separator();
                    for i in 0..self.logs.len() {
                        let frames = self.logs[i].parsed.flight_data.time_us.len();
                        let label = format!("Log {}  ({} frames)", i + 1, frames);
                        if ui.selectable_label(self.active_log == i, label).clicked() {
                            self.active_log = i;
                            self.plot_state.cursor_time = None;
                        }
                    }
                });
        }

        if !self.logs.is_empty() {
            let log = &self.logs[self.active_log];
            egui::Panel::right("log_info")
                .resizable(true)
                .default_size(220.0)
                .show_inside(ui, |ui| {
                    log_info::show(ui, &log.parsed.metadata, log.parsed.flight_data.sample_rate.rate_hz);
                });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.logs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a .bbl or .bfl file to get started");
                });
                return;
            }

            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.active_panel,
                    ActivePanel::Timeseries,
                    "Timeseries",
                );
                ui.selectable_value(&mut self.active_panel, ActivePanel::Spectral, "Spectral");
                ui.selectable_value(
                    &mut self.active_panel,
                    ActivePanel::StepResponse,
                    "Step Response",
                );
            });
            ui.separator();

            let log = &self.logs[self.active_log];
            match self.active_panel {
                ActivePanel::Timeseries => {
                    self.timeseries_panel
                        .show(ui, &log.parsed.flight_data, &mut self.plot_state);
                }
                ActivePanel::Spectral => {
                    self.spectral_panel
                        .show(ui, &log.analysis, &log.parsed.metadata);
                }
                ActivePanel::StepResponse => {
                    self.step_response_panel
                        .show(ui, &log.parsed.flight_data, &log.analysis);
                }
            }
        });
    }
}

impl App {
    fn poll_load(&mut self, ctx: &egui::Context) {
        let LoadState::Loading { rx, .. } = &self.load_state else {
            return;
        };

        loop {
            match rx.try_recv() {
                Ok(LoadEvent::LogReady(log)) => {
                    let first = self.logs.is_empty();
                    self.logs.push(log);
                    self.loading_frames = 0;
                    if first {
                        self.active_log = 0;
                        self.plot_state = PlotState::default();
                    }
                    ctx.request_repaint();
                }
                Ok(LoadEvent::Progress(frames)) => {
                    self.loading_frames = frames;
                    ctx.request_repaint();
                }
                Ok(LoadEvent::Failed(msg)) => {
                    self.error = Some(msg);
                    self.load_state = LoadState::Idle;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.logs.is_empty() {
                        self.error = Some("All logs in file were corrupt".to_owned());
                    }
                    self.logs.sort_by_key(|l| l.parsed.log_index);
                    self.step_response_panel.invalidate_cache();
                    self.load_state = LoadState::Idle;
                    return;
                }
            }
        }
    }

    fn pick_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Blackbox Log", &["bbl", "bfl", "BBL", "BFL"])
            .pick_file()
        else {
            return;
        };

        let file = match parser::LogFile::open(&path) {
            Ok(f) => f,
            Err(e) => {
                self.error = Some(format!("Failed to read: {e}"));
                return;
            }
        };

        let count = file.log_count();
        if count == 0 {
            self.error = Some("No valid logs found in file".to_owned());
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.load_state = LoadState::Loading {
            rx,
            expected: count,
        };
        self.logs.clear();
        self.loading_frames = 0;
        self.error = None;

        let file = std::sync::Arc::new(file);
        for i in 0..count {
            let file = file.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                match file.parse_log(i, |frames| {
                    tx.send(LoadEvent::Progress(frames)).ok();
                }) {
                    Ok(parsed) => {
                        let analysis = analysis::analyse(&parsed.flight_data);
                        tx.send(LoadEvent::LogReady(LoadedLog { parsed, analysis }))
                            .ok();
                    }
                    Err(e) => tracing::warn!("Log {i} skipped: {e}"),
                }
            });
        }
    }
}
