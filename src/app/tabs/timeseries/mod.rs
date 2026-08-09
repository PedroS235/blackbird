mod gyro;
mod power_battery;
mod rssi;

use egui::Ui;

use super::{TabCtx, tab_bar};

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
            // Unconditional, and now honestly so: the Gyro panel falls back to
            // the filtered trace, and says why when there is no gyro at all.
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

        let tabs = [
            (TimeseriesTab::Gyro, "Gyro"),
            (TimeseriesTab::PowerBattery, "Power & Battery"),
            (TimeseriesTab::Rssi, "Receiver RSSI"),
        ]
        .map(|(tab, label)| (tab, label, available.has(tab)));
        tab_bar(ui, &mut self.selected, &tabs);
        ui.add_space(4.0);

        match self.selected {
            TimeseriesTab::Gyro => gyro::show(ui, ctx.flight),
            TimeseriesTab::PowerBattery => power_battery::show(ui, ctx.flight),
            TimeseriesTab::Rssi => rssi::show(ui, ctx.flight),
        }
    }

    /// Switching to a log that lacks the channel you were looking at drops you
    /// back on Gyro, which always has something to say, rather than on a blank
    /// panel.
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
