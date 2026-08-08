mod gyro;
mod power_battery;
mod rssi;

use egui::Ui;

use super::TabCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TimeseriesTab {
    #[default]
    Gyro,
    PowerBattery,
    Rssi,
}

/// Which of the log's optional channels are present. Derived from the flight
/// data by whoever reads it — here — rather than carried in `TabCtx`.
#[derive(Debug, Clone, Copy)]
struct Available {
    power: bool,
    rssi: bool,
}

impl Available {
    fn has(self, tab: TimeseriesTab) -> bool {
        match tab {
            TimeseriesTab::Gyro => true,
            TimeseriesTab::PowerBattery => self.power,
            TimeseriesTab::Rssi => self.rssi,
        }
    }
}

#[derive(Default)]
pub(super) struct Timeseries {
    selected: TimeseriesTab,
}

impl Timeseries {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        let available = Available {
            power: ctx.flight.has_power(),
            rssi: ctx.flight.has_rssi(),
        };
        self.resolve(available);

        ui.horizontal(|ui| {
            for (tab, label) in [
                (TimeseriesTab::Gyro, "Gyro"),
                (TimeseriesTab::PowerBattery, "Power & Battery"),
                (TimeseriesTab::Rssi, "Receiver RSSI"),
            ] {
                let selectable = egui::Button::selectable(self.selected == tab, label);
                if ui.add_enabled(available.has(tab), selectable).clicked() {
                    self.selected = tab;
                }
            }
        });
        ui.add_space(4.0);

        match self.selected {
            TimeseriesTab::Gyro => gyro::show(ui, ctx.flight),
            TimeseriesTab::PowerBattery => power_battery::show(ui, ctx.flight),
            TimeseriesTab::Rssi => rssi::show(ui, ctx.flight),
        }
    }

    /// Switching to a log that lacks the channel you were looking at drops you
    /// back on Gyro, which every log has, rather than on a blank panel.
    fn resolve(&mut self, available: Available) {
        if !available.has(self.selected) {
            self.selected = TimeseriesTab::Gyro;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn tabs(selected: TimeseriesTab) -> Timeseries {
        Timeseries { selected }
    }

    #[test]
    fn full_log_leaves_every_selection_alone() {
        let available = Available {
            power: true,
            rssi: true,
        };
        for tab in [
            TimeseriesTab::Gyro,
            TimeseriesTab::PowerBattery,
            TimeseriesTab::Rssi,
        ] {
            let mut tabs = tabs(tab);
            tabs.resolve(available);
            assert_eq!(tabs.selected, tab);
            assert!(available.has(tab));
        }
    }

    #[test]
    fn power_falls_back_without_power_data() {
        let mut tabs = tabs(TimeseriesTab::PowerBattery);
        tabs.resolve(Available {
            power: false,
            rssi: true,
        });
        assert_eq!(tabs.selected, TimeseriesTab::Gyro);
    }

    #[test]
    fn rssi_falls_back_without_rssi_data() {
        let mut tabs = tabs(TimeseriesTab::Rssi);
        tabs.resolve(Available {
            power: true,
            rssi: false,
        });
        assert_eq!(tabs.selected, TimeseriesTab::Gyro);
    }

    #[test]
    fn gyro_is_always_available() {
        let mut tabs = tabs(TimeseriesTab::Gyro);
        tabs.resolve(Available {
            power: false,
            rssi: false,
        });
        assert_eq!(tabs.selected, TimeseriesTab::Gyro);
    }

    #[test]
    fn power_stays_when_only_rssi_is_missing() {
        let mut tabs = tabs(TimeseriesTab::PowerBattery);
        tabs.resolve(Available {
            power: true,
            rssi: false,
        });
        assert_eq!(tabs.selected, TimeseriesTab::PowerBattery);
    }
}
