mod mainview;
mod notification;
mod sidepanel;
mod ui;

use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver},
};

use eframe::App;

use crate::{
    app::{
        mainview::{MainTab, PidAnalysisTab, TimeseriesTab},
        notification::Notification,
    },
    parser::{self, LogFile, ParsedLog},
};

const MAX_NOTIFICATIONS: usize = 50;

pub struct BlackbirdApp {
    app_name: &'static str,
    logs: Vec<LoadedLog>,
    notifications: VecDeque<Notification>,
    load_state: LoadState,
    main_tab: MainTab,
    timeseries_tab: TimeseriesTab,
    pidanalysis_tab: PidAnalysisTab,
    gyro_filtered_visible: [bool; 3],
    gyro_raw_visible: [bool; 3],
    setpoint_visible: [bool; 3],
    vbat_visible: bool,
    current_visible: bool,
    rssi_visible: bool,
}

impl Default for BlackbirdApp {
    fn default() -> Self {
        Self {
            app_name: "Blackbird",
            logs: Default::default(),
            notifications: Default::default(),
            load_state: LoadState::Idle,
            main_tab: MainTab::default(),
            timeseries_tab: TimeseriesTab::default(),
            pidanalysis_tab: PidAnalysisTab::default(),
            gyro_filtered_visible: [true; 3],
            gyro_raw_visible: [true; 3],
            setpoint_visible: [true; 3],
            vbat_visible: true,
            current_visible: true,
            rssi_visible: true,
        }
    }
}

impl App for BlackbirdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_load(ui.ctx());
        self.show_loading_modal(ui.ctx());

        self.show_sidepanel(ui);
        self.show_notifications(ui);
        self.show_mainview(ui);
    }
}

impl BlackbirdApp {
    pub fn notify(&mut self, level: notification::Level, msg: impl Into<String>) {
        let msg = msg.into();
        match level {
            notification::Level::Error => tracing::error!("{}", msg),
            notification::Level::Warning => tracing::warn!("{}", msg),
            notification::Level::Info => tracing::info!("{}", msg),
        };

        self.notifications.push_back(Notification {
            level,
            message: msg,
        });

        if self.notifications.len() > MAX_NOTIFICATIONS {
            self.notifications.pop_front();
        }
    }

    fn poll_load(&mut self, ctx: &egui::Context) {
        let LoadState::Loading {
            rx,
            loaded,
            current,
            ..
        } = &mut self.load_state
        else {
            return;
        };

        loop {
            match rx.try_recv() {
                Ok(LoadEvent::Progress(name)) => {
                    *current = name;
                    ctx.request_repaint();
                }
                Ok(LoadEvent::LogReady(log)) => {
                    *loaded += 1;
                    self.logs.push(log);
                    ctx.request_repaint();
                }
                Ok(LoadEvent::Error(msg)) => {
                    self.notify(notification::Level::Error, msg);
                    self.load_state = LoadState::Idle;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.logs.is_empty() {
                        self.notify(notification::Level::Error, "All logs in file were corrupt");
                    }
                    self.load_state = LoadState::Idle;
                    return;
                }
            }
        }
    }

    fn show_loading_modal(&self, ctx: &egui::Context) {
        let LoadState::Loading {
            expected,
            loaded,
            current,
            ..
        } = &self.load_state
        else {
            return;
        };

        egui::Modal::new(egui::Id::new("loading_modal")).show(ctx, |ui| {
            ui.set_min_width(300.0);
            ui.vertical_centered(|ui| {
                ui.heading("Loading logs");
                ui.add_space(8.0);
                ui.add(
                    egui::ProgressBar::new(*loaded as f32 / *expected as f32)
                        .text(format!("{} / {}", loaded, expected)),
                );
                ui.add_space(4.0);
                ui.label(current.as_str());
            });
        });
    }

    fn open_logs(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Blackbox Log", &["bbl", "bfl", "BBL", "BFL"])
            .pick_files()
        else {
            return;
        };

        let (oks, errs): (Vec<_>, Vec<_>) = paths
            .iter()
            .map(|p| parser::LogFile::open(p).map_err(|e| (p, e)))
            .partition(Result::is_ok);

        for err in errs {
            let (path, e) = err.unwrap_err();
            self.notify(
                notification::Level::Error,
                format!("{}: {e}", path.display()),
            );
        }

        let new_logs: Vec<LogFile> = oks.into_iter().map(Result::unwrap).collect();
        // self.logs.extend(new_logs);

        let (tx, rx) = mpsc::channel();
        self.load_state = LoadState::Loading {
            rx,
            expected: new_logs.len(),
            loaded: 0,
            current: String::new(),
        };

        new_logs.into_iter().for_each(|log| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                tx.send(LoadEvent::Progress(log.file_name.clone())).ok();
                match log.parse_logs() {
                    Ok(parsed) => {
                        tx.send(LoadEvent::LogReady(LoadedLog {
                            log: parsed,
                            analysis: None,
                            selected: true,
                            active_sublog: 0,
                        }))
                        .ok();
                    }
                    Err(e) => {
                        tx.send(LoadEvent::Error(e.to_string())).ok();
                    }
                }
            });
        });
    }
}

// Temporary Stub
struct AnalysisResult;

struct LoadedLog {
    log: Vec<ParsedLog>,
    analysis: Option<AnalysisResult>,
    selected: bool,
    active_sublog: usize,
}

enum LoadEvent {
    Progress(String),
    LogReady(LoadedLog),
    Error(String),
}

enum LoadState {
    Idle,
    Loading {
        rx: Receiver<LoadEvent>,
        expected: usize,
        loaded: usize,
        current: String,
    },
}
