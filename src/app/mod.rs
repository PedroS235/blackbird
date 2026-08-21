mod colors;
mod log_store;
mod notification;
mod perf; // [DEBUG-perf9]
mod sidepanel;
mod tabs;
pub(crate) mod theme;
mod ui;
mod update;

use std::{collections::VecDeque, sync::mpsc};

use eframe::App;
use elegance::ProgressBar;

use crate::{
    app::{
        log_store::{LoadState, LogStore},
        notification::Notification,
        tabs::Tabs,
        update::UpdateCheck,
    },
    loader::{LoadEvent, LogLoader},
};

const MAX_NOTIFICATIONS: usize = 50;

pub struct BlackbirdApp {
    app_name: &'static str,
    logs: LogStore,
    notifications: VecDeque<Notification>,
    load_state: LoadState,
    tabs: Tabs,
    theme_preference: egui::ThemePreference,
    update: UpdateCheck,
    perf: Option<perf::Perf>, // [DEBUG-perf9]
}

impl Default for BlackbirdApp {
    fn default() -> Self {
        Self {
            app_name: "Blackbird",
            logs: Default::default(),
            notifications: Default::default(),
            load_state: LoadState::Idle,
            tabs: Tabs::default(),
            theme_preference: Default::default(),
            update: Default::default(),
            perf: perf::Perf::from_env(), // [DEBUG-perf9]
        }
    }
}

impl App for BlackbirdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now(); // [DEBUG-perf9]
        ui.ctx().set_theme(self.theme_preference);
        theme::apply(ui.ctx(), ui.ctx().theme() == egui::Theme::Dark);
        self.poll_load(ui.ctx());
        self.show_loading_modal(ui.ctx());

        self.show_update_strip(ui);
        self.show_sidepanel(ui);
        self.show_notifications(ui);
        self.tabs.show(ui, &self.logs);

        // [DEBUG-perf9]
        let settled = matches!(self.load_state, LoadState::Idle);
        if let Some(probe) = self.perf.as_mut() {
            probe.record(ui.ctx(), _frame, t0, settled);
        }
    }

    // [DEBUG-perf9]
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Some(probe) = self.perf.as_mut() {
            probe.inject(ctx, input);
        }
    }
}

impl BlackbirdApp {
    /// `Default` cannot do this: the update check needs the `Context` to wake
    /// the UI when its answer lands, and only `eframe` hands that out.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            update: UpdateCheck::spawn(&cc.egui_ctx),
            ..Default::default()
        };
        // [DEBUG-perf9] skip the dialog, so the probe runs unattended
        if let Some(path) = perf::open_path() {
            app.load(vec![path]);
        }
        app
    }

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

    /// Drains whatever the loader threads produced since the last frame.
    /// Failures are collected and reported after the drain — `notify` takes
    /// `&mut self`, which the borrow on `load_state` rules out mid-loop.
    fn poll_load(&mut self, ctx: &egui::Context) {
        let LoadState::Loading {
            handle,
            progress,
            current,
        } = &mut self.load_state
        else {
            return;
        };

        let mut errors = Vec::new();
        let mut finished = false;

        loop {
            match handle.rx.try_recv() {
                Ok(LoadEvent::Progress {
                    file_name,
                    sublog,
                    sublog_count,
                    fraction,
                }) => {
                    *current = format!("{file_name} — log {} / {sublog_count}", sublog + 1);
                    progress.insert(file_name, (sublog as f32 + fraction) / sublog_count as f32);
                }
                Ok(LoadEvent::Ready(log)) => {
                    progress.insert(log.file_name.clone(), 1.0);
                    self.logs.push(log);
                }
                Ok(LoadEvent::Failed { file_name, error }) => {
                    errors.push(format!("{file_name}: {error}"))
                }
                Ok(LoadEvent::Cancelled { file_name }) => {
                    progress.insert(file_name, 1.0);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }

        if finished {
            tracing::debug!("every loader thread finished");
            self.load_state = LoadState::Idle;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        for error in errors {
            self.notify(notification::Level::Error, error);
        }
    }

    fn show_loading_modal(&self, ctx: &egui::Context) {
        let fraction = self.load_state.fraction();
        let LoadState::Loading {
            handle, current, ..
        } = &self.load_state
        else {
            return;
        };

        egui::Modal::new(egui::Id::new("loading_modal")).show(ctx, |ui| {
            ui.set_min_width(300.0);
            ui.vertical_centered(|ui| {
                ui.heading("Loading logs");
                ui.add_space(8.0);
                ui.add(ProgressBar::new(fraction).text(if handle.expected > 1 {
                    format!("{:.0}% of {} files", fraction * 100.0, handle.expected)
                } else {
                    format!("{:.0}%", fraction * 100.0)
                }));
                ui.add_space(4.0);
                ui.label(current.as_str());
                ui.add_space(8.0);
                if ui.button("Cancel").clicked() {
                    handle.cancel.cancel();
                }
            });
        });
    }

    fn open_logs(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Blackbox Log", &["bbl", "bfl", "BBL", "BFL"])
            .pick_files()
        else {
            tracing::debug!("file dialog dismissed without a selection");
            return;
        };

        tracing::info!("opening {} file(s) from the picker", paths.len());
        self.load(paths);
    }

    fn load(&mut self, paths: Vec<std::path::PathBuf>) {
        self.load_state = LoadState::Loading {
            handle: LogLoader::default().spawn(paths),
            progress: Default::default(),
            current: String::new(),
        };
    }
}
