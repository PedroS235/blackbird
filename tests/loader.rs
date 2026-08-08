//! The load pipeline end to end, driven inline on the test thread through the
//! `Vec<LoadEvent>` sink — no UI, no threads.

use std::path::{Path, PathBuf};

use blackbird::loader::{CancelToken, LoadEvent, LoadedLog, LogLoader};
use blackbird::parser::{Axis, LogFile};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(loader: &LogLoader, name: &str) -> Vec<LoadEvent> {
    let mut events = Vec::new();
    loader.load_path(&fixture(name), &CancelToken::default(), &mut events);
    events
}

fn ready(events: Vec<LoadEvent>) -> LoadedLog {
    events
        .into_iter()
        .find_map(|e| match e {
            LoadEvent::Ready(loaded) => Some(loaded),
            _ => None,
        })
        .expect("a Ready event")
}

#[test]
fn single_log_file_emits_progress_then_ready() {
    let events = load(&LogLoader::default(), "new202612_BF_steadyhover.BFL");

    assert!(matches!(
        events.first(),
        Some(LoadEvent::Progress {
            sublog: 0,
            sublog_count: 1,
            ..
        })
    ));

    // Progress is reported through the sublog, not just at its start.
    let fractions: Vec<f32> = events
        .iter()
        .filter_map(|e| match e {
            LoadEvent::Progress { fraction, .. } => Some(*fraction),
            _ => None,
        })
        .collect();
    assert!(
        fractions.len() > 2,
        "got {} progress events",
        fractions.len()
    );
    assert!(fractions.windows(2).all(|w| w[0] <= w[1]));

    let loaded = ready(events);
    assert_eq!(loaded.file_name, "new202612_BF_steadyhover.BFL");
    assert_eq!(loaded.logs.len(), 1);
    // One analysis per sublog is the invariant the log store indexes on.
    assert_eq!(loaded.analysis.len(), loaded.logs.len());
}

#[test]
fn analysis_runs_at_load_time() {
    let loaded = ready(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let spectral = &loaded.analysis[0].spectral;
    assert!(spectral.axis(Axis::Roll).is_some());
}

/// The knob the UI used to have no way to reach: raising the peak floor above
/// anything in the log has to leave the peak list empty.
#[test]
fn analysis_knobs_reach_the_analyzer() {
    let mut loader = LogLoader::default();
    loader.analyzer.peak_min_above_floor_db = 1_000.0;

    let loaded = ready(load(&loader, "new202612_BF_steadyhover.BFL"));
    let roll = loaded.analysis[0].spectral.axis(Axis::Roll).unwrap();
    assert!(roll.peaks.is_empty());
}

#[test]
fn cancel_before_start_yields_no_logs() {
    let file = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();

    let mut events = Vec::new();
    LogLoader::default().load_file(&file, &cancel, &mut events);

    assert!(matches!(
        events.as_slice(),
        [LoadEvent::Progress { .. }, LoadEvent::Cancelled { .. }]
    ));
}

/// Cancelling lands mid-sublog, not at the next sublog boundary — a single
/// 17 MB flight would otherwise have to be waited out.
#[test]
fn cancel_stops_a_sublog_in_flight() {
    let file = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
    let cancel = CancelToken::default();

    // A sink that pulls the switch as soon as decoding has actually started.
    let mut events = Vec::new();
    struct StopAfterFirstFraction<'a> {
        events: &'a mut Vec<LoadEvent>,
        cancel: &'a CancelToken,
    }
    impl blackbird::loader::LoadSink for StopAfterFirstFraction<'_> {
        fn emit(&mut self, event: LoadEvent) {
            if matches!(&event, LoadEvent::Progress { fraction, .. } if *fraction > 0.0) {
                self.cancel.cancel();
            }
            self.events.push(event);
        }
    }

    LogLoader::default().load_file(
        &file,
        &cancel,
        &mut StopAfterFirstFraction {
            events: &mut events,
            cancel: &cancel,
        },
    );

    assert!(matches!(events.last(), Some(LoadEvent::Cancelled { .. })));
    assert!(!events.iter().any(|e| matches!(e, LoadEvent::Ready(_))));
}

#[test]
fn unreadable_path_fails_without_ready() {
    let mut events = Vec::new();
    LogLoader::default().load_path(
        Path::new("/nonexistent/file.bbl"),
        &CancelToken::default(),
        &mut events,
    );

    assert!(matches!(
        events.as_slice(),
        [LoadEvent::Failed { file_name, .. }] if file_name == "file.bbl"
    ));
}

/// Progress is per sublog, not per file — an eight-flight `.bbl` reports eight
/// steps. Ignored by default: parsing all eight is slow, same as the sibling
/// `.bbl` parse tests in `parser`.
#[test]
#[ignore]
fn multi_log_file_reports_each_sublog() {
    let events = load(&LogLoader::default(), "eight_logs_in_one.bbl");

    let mut sublogs: Vec<usize> = events
        .iter()
        .filter_map(|e| match e {
            LoadEvent::Progress {
                sublog,
                sublog_count: 8,
                ..
            } => Some(*sublog),
            _ => None,
        })
        .collect();
    sublogs.dedup();

    assert_eq!(sublogs, (0..8).collect::<Vec<_>>());
}

/// The hover fixture logs `setpoint`, but the pilot is holding position — no
/// window clears the 20 deg/s stick mask, so every axis is legitimately empty.
#[test]
fn a_hover_moves_the_sticks_too_little_for_a_step_response() {
    let loaded = ready(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let step = &loaded.analysis[0].step;

    assert!(Axis::ALL.iter().all(|&axis| step.axis(axis).is_none()));
}

/// Same log, mask dropped to what a hover's stick jitter reaches: the
/// deconvolution runs end to end and lands traces on a real flight.
#[test]
fn dropping_the_stick_mask_recovers_traces_from_the_hover() {
    let mut loader = LogLoader::default();
    loader.step_response.min_setpoint_dps = 1.0;

    let loaded = ready(load(&loader, "new202612_BF_steadyhover.BFL"));
    let roll = loaded.analysis[0]
        .step
        .axis(Axis::Roll)
        .expect("roll traces");

    assert!(!roll.traces.is_empty());
    assert_eq!(roll.mean.len(), roll.time_ms.len());
    assert!(roll.mean.iter().all(|v| v.is_finite()));
    assert!(roll.traces.iter().all(|t| t.len() == roll.mean.len()));
}
