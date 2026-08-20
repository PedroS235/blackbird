//! The load pipeline end to end, driven inline on the test thread through the
//! `Vec<LoadEvent>` sink — no UI, no threads.

use std::path::{Path, PathBuf};

use blackbird::analysis::{
    FilterLoop, FilterOverlay, HarmonicBand, NoStepResponse, OverlayFamily, OverlayShape,
};
use blackbird::loader::{CancelToken, LoadEvent, LoadedLog, LogLoader};
use blackbird::parser::metadata::RateType;
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

fn overlays(events: Vec<LoadEvent>) -> Vec<FilterOverlay> {
    ready(events).analysis[0].spectral.overlays.clone()
}

fn family(overlays: &[FilterOverlay], family: OverlayFamily) -> Vec<&FilterOverlay> {
    overlays.iter().filter(|o| o.family == family).collect()
}

/// `eRPM` reaches the plot as bands at the frequencies the motors actually
/// turned: four motors, and three orders because the fixture configured three
/// RPM filter harmonics.
#[test]
fn a_fixtures_motors_become_harmonic_bands_at_their_own_frequencies() {
    let overlays = overlays(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let [harmonics] = family(&overlays, OverlayFamily::Harmonics)[..] else {
        panic!("the fixture logs eRPM, so it has motor harmonics");
    };
    let OverlayShape::Harmonics(bands) = &harmonics.shape else {
        panic!("harmonics are a harmonic group");
    };

    assert_eq!(bands.len(), 12, "four motors at three orders");
    assert!(bands.iter().all(|b| b.filtered), "weights are 90,50,90");

    let motor_zero: Vec<&HarmonicBand> = bands.iter().filter(|b| b.motor == 0).collect();
    let fundamental = motor_zero[0];

    // A hovering quad's motors turn somewhere between a lazy idle and full
    // song; anything outside this is a pole count or a unit gone wrong.
    assert!(
        (60.0..600.0).contains(&fundamental.low_hz),
        "fundamental at {:.0}..{:.0} Hz",
        fundamental.low_hz,
        fundamental.high_hz
    );
    assert!(fundamental.high_hz > fundamental.low_hz);

    // The third harmonic is three times the first, per motor.
    assert!((motor_zero[2].low_hz - 3.0 * fundamental.low_hz).abs() < 1e-6);
}

/// A harmonic whose RPM filter weight is zero is tracked and not attenuated,
/// and the geometry has to say which is which. One of the `.bbl` fixture's
/// eight flights was flown on weights `100,0,80`.
#[test]
#[ignore]
fn a_zero_weight_harmonic_is_marked_unfiltered_end_to_end() {
    let loaded = ready(load(&LogLoader::default(), "eight_logs_in_one.bbl"));

    let unfiltered: Vec<u32> = loaded
        .analysis
        .iter()
        .flat_map(|a| &a.spectral.overlays)
        .filter_map(|o| match &o.shape {
            OverlayShape::Harmonics(bands) => Some(bands),
            _ => None,
        })
        .flatten()
        .filter(|b| !b.filtered)
        .map(|b| b.order)
        .collect();

    assert!(
        unfiltered.iter().all(|&order| order == 2),
        "only the second harmonic was zero-weighted: {unfiltered:?}"
    );
    assert!(!unfiltered.is_empty(), "no zero-weight harmonic was found");
}

/// The dynamic notch is drawn as the range it may reach, and — this fixture
/// was flown in `FFT_FREQ` — as where the tracker actually sat.
#[test]
fn a_fft_freq_fixture_carries_both_the_configured_range_and_the_traced_centre() {
    let overlays = overlays(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let dyn_notch = family(&overlays, OverlayFamily::DynNotch);

    assert_eq!(
        dyn_notch[0].shape,
        OverlayShape::Band {
            low_hz: 90.0,
            high_hz: 400.0
        }
    );
    // Two notches configured, one centre logged — the label says so.
    assert_eq!(dyn_notch[0].label, "Dyn notch range (×2)");

    let OverlayShape::Traced(per_axis) = &dyn_notch[1].shape else {
        panic!("a log flown in FFT_FREQ carries the traced centre");
    };
    let roll = per_axis[Axis::Roll].as_ref().expect("roll was traced");
    assert!((roll.weight.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(roll.freq_hz.first().is_some_and(|&f| f > 90.0));
    assert!(roll.freq_hz.last().is_some_and(|&f| f < 400.0));
}

/// The negative case: flown in another debug mode, `debug[0..3]` is something
/// else, and the overlay degrades to the configured range rather than
/// disappearing.
#[test]
#[ignore]
fn a_log_flown_in_another_debug_mode_still_gets_the_configured_range() {
    let loaded = ready(load(&LogLoader::default(), "eight_logs_in_one.bbl"));
    let overlays = &loaded.analysis[0].spectral.overlays;

    assert!(!loaded.logs[0].metadata.logs_dyn_notch_trace());
    let dyn_notch = family(overlays, OverlayFamily::DynNotch);
    assert_eq!(dyn_notch.len(), 1, "the range, and no trace");
    assert!(matches!(dyn_notch[0].shape, OverlayShape::Band { .. }));
}

/// A dynamic lowpass is the range its cutoff swept, not the ceiling — this
/// fixture ran gyro LPF1 dynamic across 250..500 Hz.
#[test]
fn a_dynamic_lowpass_reaches_the_plot_as_its_range() {
    let overlays = overlays(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let gyro = family(&overlays, OverlayFamily::Lowpass(FilterLoop::Gyro));

    assert_eq!(
        gyro[0].shape,
        OverlayShape::Band {
            low_hz: 250.0,
            high_hz: 500.0
        }
    );
    assert_eq!(gyro[1].shape, OverlayShape::Line { hz: 500.0 });
}

/// A notch that was never enabled is not a filter at zero hertz.
#[test]
fn notches_the_pilot_never_enabled_are_not_drawn() {
    let overlays = overlays(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));

    assert!(family(&overlays, OverlayFamily::Notch(FilterLoop::Gyro)).is_empty());
    assert!(family(&overlays, OverlayFamily::Notch(FilterLoop::Dterm)).is_empty());
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

/// The rate headers a pilot flew on reach the log card and the preset row as
/// decoded values, not as raw header strings.
#[test]
fn a_fixtures_headers_decode_to_the_rates_it_was_flown_on() {
    let loaded = ready(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let rates = loaded.logs[0].metadata.rates.expect("rate config");

    assert_eq!(rates.rate_type, RateType::Actual);
    assert_eq!(rates.rc_rates, Some([12.0, 12.0, 12.0]));
    assert_eq!(rates.rates, [70.0, 70.0, 60.0]);
    assert_eq!(rates.expo, Some([45.0, 45.0, 35.0]));
    assert_eq!(rates.to_string(), "Actual 70/70/60");
}

/// The hover fixture logs `setpoint`, but the pilot is holding position — no
/// window clears the 52 deg/s stick mask, so every axis is legitimately empty.
#[test]
fn a_hover_moves_the_sticks_too_little_for_a_step_response() {
    let loaded = ready(load(&LogLoader::default(), "new202612_BF_steadyhover.BFL"));
    let step = &loaded.analysis[0].step;

    assert!(Axis::ALL.iter().all(|&axis| {
        step.axis(axis).unwrap_err()
            == NoStepResponse::SticksTooStill {
                min_setpoint_dps: 52.0,
            }
    }));
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

    assert!(roll.count > 0);
    assert_eq!(roll.mean.len(), roll.time_ms.len());
    assert!(roll.mean.iter().all(|v| v.is_finite()));
    assert!(roll.sample.iter().all(|t| t.len() == roll.mean.len()));
}

/// The bug this whole pass exists to kill, guarded on real flight rather than
/// a synthetic: a curve that leaves the origin already part-way to the
/// commanded rate is reading a windowing artefact as a fast craft.
#[test]
fn responses_from_a_real_flight_start_from_rest() {
    let loaded = ready(load(&LogLoader::default(), "eight_logs_in_one.bbl"));

    let starts: Vec<f64> = loaded
        .analysis
        .iter()
        .flat_map(|a| Axis::ALL.iter().filter_map(|&axis| a.step.axis(axis).ok()))
        .map(|response| response.mean[0])
        .collect();

    assert!(!starts.is_empty(), "no axis of the fixture was analysed");
    assert!(
        starts.iter().all(|v| v.abs() < 0.1),
        "curves start at {starts:?}"
    );
}

/// The regression guard on real flight rather than a synthetic: a quad that
/// answers its sticks overshoots by some tens of percent and peaks tens of
/// milliseconds in. Numbers outside that are reading an artefact, not a tune.
#[test]
fn a_real_flight_reports_plausible_step_metrics() {
    let loaded = ready(load(&LogLoader::default(), "eight_logs_in_one.bbl"));

    let metrics: Vec<_> = loaded
        .analysis
        .iter()
        .flat_map(|a| Axis::ALL.iter().filter_map(|&axis| a.step.axis(axis).ok()))
        .map(|response| response.metrics.clone())
        .collect();

    assert!(!metrics.is_empty(), "no axis of the fixture was analysed");
    for m in &metrics {
        assert!(
            (0.0..=100.0).contains(&m.overshoot_pct),
            "implausible overshoot {:.0}%",
            m.overshoot_pct
        );
        assert!(
            (10.0..=200.0).contains(&m.peak_ms),
            "implausible peak at {:.0} ms",
            m.peak_ms
        );
        assert!(
            m.delay_ms > 0.0 && m.delay_ms < m.peak_ms,
            "delay {:.0} ms against a peak at {:.0} ms",
            m.delay_ms,
            m.peak_ms
        );
        assert!(m.spread_pct.start() <= m.spread_pct.end());
    }
}

/// Bounding what is retained is a memory change, not an answer change — on a
/// real multi-flight log every axis keeps a sample within the cap while still
/// reporting how many windows the mean actually came from.
#[test]
fn a_real_flight_retains_a_bounded_sample_of_a_larger_stack() {
    let loader = LogLoader::default();
    let cap = loader.step_response.max_traces;
    let loaded = ready(load(&loader, "eight_logs_in_one.bbl"));

    let responses: Vec<_> = loaded
        .analysis
        .iter()
        .flat_map(|a| Axis::ALL.iter().filter_map(|&axis| a.step.axis(axis).ok()))
        .map(|r| (r.sample.len(), r.count))
        .collect();

    assert!(!responses.is_empty(), "no axis of the fixture was analysed");
    assert!(responses.iter().all(|&(sample, _)| sample <= cap));
    assert!(
        responses.iter().any(|&(sample, count)| count > sample),
        "no axis stacked more windows than the cap: {responses:?}"
    );
}
