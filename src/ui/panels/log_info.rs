use crate::parser::Metadata;

const PID_KEYS: &[(&str, &str)] = &[
    ("rollPID", "Roll"),
    ("pitchPID", "Pitch"),
    ("yawPID", "Yaw"),
];

const FILTER_KEYS: &[(&str, &str)] = &[
    ("gyro_lowpass_hz", "Gyro LPF"),
    ("gyro_lowpass2_hz", "Gyro LPF2"),
    ("dterm_lowpass_hz", "D-term LPF"),
    ("dterm_lowpass2_hz", "D-term LPF2"),
    ("gyro_rpm_notch_harmonics", "RPM harmonics"),
    ("gyro_rpm_notch_min", "RPM min"),
];

pub fn show(ui: &mut egui::Ui, h: &Metadata, sample_rate_hz: f32) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(&h.craft_name);
        ui.label(format!("Firmware: {}", h.firmware));
        if !h.board.is_empty() {
            ui.label(format!("Board: {}", h.board));
        }
        ui.label(format!("Duration: {:.1}s", h.duration.as_secs_f32()));
        ui.label(format!("Sample rate: {:.0} Hz", sample_rate_hz));

        ui.separator();
        ui.strong("PIDs");
        for (key, label) in PID_KEYS {
            if let Some(val) = h.raw_headers.get(*key) {
                ui.label(format!("{label}: {val}"));
            }
        }

        ui.separator();
        ui.strong("Filters");
        for (key, label) in FILTER_KEYS {
            if let Some(val) = h.raw_headers.get(*key) {
                ui.label(format!("{label}: {val}"));
            }
        }

        let known: std::collections::HashSet<&str> = PID_KEYS
            .iter()
            .chain(FILTER_KEYS.iter())
            .map(|(k, _)| *k)
            .collect();

        let mut extras: Vec<(&String, &String)> = h
            .raw_headers
            .iter()
            .filter(|(k, _)| !known.contains(k.as_str()))
            .collect();
        extras.sort_by_key(|(k, _)| k.as_str());

        if !extras.is_empty() {
            ui.separator();
            ui.strong("Other headers");
            for (k, v) in extras {
                ui.label(format!("{k}: {v}"));
            }
        }
    });
}
