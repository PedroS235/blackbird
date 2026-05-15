use std::ops::RangeInclusive;
use std::sync::mpsc::{self, Receiver};

use crate::parser::{self, ParsedLog};
use crate::ui::panels::timeseries::TimeseriesPanel;
use crate::ui::panels::log_info;

pub struct App {
    logs: Vec<ParsedLog>,
    active_log: usize,
    pub plot_state: PlotState,
    timeseries_panel: TimeseriesPanel,
    load_state: LoadState,
    error: Option<String>,
}

enum LoadState {
    Idle,
    Loading(Receiver<Result<Vec<ParsedLog>, String>>),
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
        Self {
            logs: Vec::new(),
            active_log: 0,
            plot_state: PlotState::default(),
            timeseries_panel: TimeseriesPanel::default(),
            load_state: LoadState::Idle,
            error: None,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_load(ui.ctx());

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let loading = matches!(self.load_state, LoadState::Loading(_));
                if loading {
                    ui.add(egui::Spinner::new());
                    ui.label("Loading…");
                } else if ui.button("Open Log").clicked() {
                    self.pick_file();
                }
                if let Some(log) = self.logs.get(self.active_log) {
                    ui.separator();
                    ui.label(&log.header.craft_name);
                    ui.label(&log.header.firmware);
                    if let Some(hz) = log.header.sample_rate_hz {
                        ui.label(format!("{hz} Hz"));
                    }
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
                        let frames = self.logs[i].data.time_us.len();
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
                    log_info::show(ui, &log.header);
                });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.logs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a .bbl or .bfl file to get started");
                });
            } else {
                let log = &self.logs[self.active_log];
                self.timeseries_panel
                    .show(ui, &log.data, &mut self.plot_state);
            }
        });
    }
}

impl App {
    fn poll_load(&mut self, ctx: &egui::Context) {
        let LoadState::Loading(rx) = &self.load_state else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(logs)) => {
                self.logs = logs;
                self.active_log = 0;
                self.error = None;
                self.plot_state = PlotState::default();
                self.load_state = LoadState::Idle;
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                self.load_state = LoadState::Idle;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Parse thread crashed".to_owned());
                self.load_state = LoadState::Idle;
            }
        }
    }

    fn pick_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Blackbox Log", &["bbl", "bfl"])
            .pick_file()
        else {
            return;
        };

        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(format!("Failed to read: {e}"));
                return;
            }
        };

        let (tx, rx) = mpsc::channel();
        self.load_state = LoadState::Loading(rx);
        self.error = None;

        std::thread::spawn(move || {
            let count = parser::log_count(&bytes);
            if count == 0 {
                let _ = tx.send(Err("No valid logs found in file".to_owned()));
                return;
            }

            let mut loaded = Vec::with_capacity(count);
            for i in 0..count {
                match parser::parse(&bytes, i) {
                    Ok(log) => loaded.push(log),
                    Err(e) => tracing::warn!("Log {i} skipped: {e}"),
                }
            }

            if loaded.is_empty() {
                let _ = tx.send(Err("All logs in file were corrupt".to_owned()));
            } else {
                let _ = tx.send(Ok(loaded));
            }
        });
    }
}
