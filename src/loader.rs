//! The load pipeline: paths in, parsed-and-analysed logs out.
//!
//! Lives in the library half so the whole path — open, parse each sublog,
//! analyse it — is drivable from a test without a UI. The UI keeps path
//! picking and rendering; everything between the two is here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::analysis::{Analysis, GyroNoiseAnalyzer, StepResponseAnalyzer};
use crate::parser::{LogFile, ParseError, ParsedLog};

/// One file's sublogs, each with its analysis computed at load time.
#[derive(Debug, Default)]
pub struct LoadedLog {
    pub file_name: String,
    pub logs: Vec<ParsedLog>,
    /// One entry per sublog in `logs`, same order.
    pub analysis: Vec<Analysis>,
}

#[derive(Debug)]
pub enum LoadEvent {
    /// Decoding sublog `sublog` (0-based) of `sublog_count`, `fraction`
    /// (0..=1) of the way through that sublog's data.
    Progress {
        file_name: String,
        sublog: usize,
        sublog_count: usize,
        fraction: f32,
    },
    /// A file finished — holds every sublog of it that parsed.
    Ready(LoadedLog),
    /// One sublog, or a whole file, failed. The rest keeps loading.
    Failed {
        file_name: String,
        error: String,
    },
    Cancelled {
        file_name: String,
    },
}

/// Where load events go. The UI plugs in an mpsc `Sender` and polls the
/// receiver each frame; tests plug in a `Vec` and drive the load inline.
pub trait LoadSink {
    fn emit(&mut self, event: LoadEvent);
}

impl LoadSink for Vec<LoadEvent> {
    fn emit(&mut self, event: LoadEvent) {
        self.push(event);
    }
}

impl LoadSink for Sender<LoadEvent> {
    fn emit(&mut self, event: LoadEvent) {
        // A dropped receiver means the UI moved on — nothing to report to.
        self.send(event).ok();
    }
}

/// Shared stop switch. Checked between sublogs, so cancellation takes at most
/// one sublog's parse to land.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Drives open → parse → analyse. Holds the analysis knobs, so a caller that
/// wants different peak thresholds sets them here instead of the pipeline
/// hard-coding `GyroNoiseAnalyzer::default()`.
#[derive(Debug, Clone, Default)]
pub struct LogLoader {
    pub analyzer: GyroNoiseAnalyzer,
    pub step_response: StepResponseAnalyzer,
}

impl LogLoader {
    /// Every analyser this pipeline runs over one parsed sublog.
    fn analyse(&self, parsed: &ParsedLog) -> Analysis {
        let started = Instant::now();
        let analysis = Analysis {
            spectral: self.analyzer.analyze(&parsed.flight_data, &parsed.metadata),
            step: self.step_response.analyze(&parsed.flight_data),
        };
        tracing::debug!(
            "{}: log {} analysed in {:.0} ms",
            parsed.metadata.file_name,
            parsed.log_index + 1,
            started.elapsed().as_secs_f64() * 1e3
        );
        analysis
    }

    /// Open and load one file on the calling thread.
    pub fn load_path(&self, path: &Path, cancel: &CancelToken, sink: &mut impl LoadSink) {
        tracing::info!("loading {}", path.display());
        match LogFile::open(path) {
            Ok(file) => self.load_file(&file, cancel, sink),
            Err(error) => {
                tracing::warn!("{}: {error}", path.display());
                sink.emit(LoadEvent::Failed {
                    file_name: file_name_of(path),
                    error: error.to_string(),
                });
            }
        }
    }

    /// Parse every sublog on the calling thread, analysing each as it lands.
    /// A corrupt sublog is reported and skipped — one bad flight in a `.bbl`
    /// no longer costs the other seven.
    pub fn load_file(&self, file: &LogFile, cancel: &CancelToken, sink: &mut impl LoadSink) {
        let started = Instant::now();
        let sublog_count = file.log_count();
        let name = &file.file_name;
        tracing::debug!("{name}: {sublog_count} sublog(s) to parse");
        let mut loaded = LoadedLog {
            file_name: name.clone(),
            ..Default::default()
        };

        for sublog in 0..sublog_count {
            let mut progress = |fraction| {
                sink.emit(LoadEvent::Progress {
                    file_name: name.clone(),
                    sublog,
                    sublog_count,
                    fraction,
                });
                !cancel.is_cancelled()
            };

            if !progress(0.0) {
                tracing::info!("{name}: cancelled before log {}", sublog + 1);
                sink.emit(LoadEvent::Cancelled {
                    file_name: name.clone(),
                });
                return;
            }

            match file.parse_log_with_progress(sublog, progress) {
                Ok(parsed) => {
                    loaded.analysis.push(self.analyse(&parsed));
                    loaded.logs.push(parsed);
                }
                Err(ParseError::Cancelled) => {
                    tracing::info!("{name}: cancelled during log {}", sublog + 1);
                    sink.emit(LoadEvent::Cancelled {
                        file_name: name.clone(),
                    });
                    return;
                }
                Err(error) => {
                    // One bad sublog is not a bad file: the rest still loads,
                    // so this is a warning and the loop carries on.
                    tracing::warn!("{name}: log {} failed: {error}", sublog + 1);
                    sink.emit(LoadEvent::Failed {
                        file_name: name.clone(),
                        error: format!("log {}: {error}", sublog + 1),
                    });
                }
            }
        }

        if loaded.logs.is_empty() {
            tracing::warn!("{name}: nothing loaded — no sublog parsed");
            return;
        }

        tracing::info!(
            "{name}: {}/{sublog_count} sublog(s) loaded in {:.2} s",
            loaded.logs.len(),
            started.elapsed().as_secs_f64()
        );
        sink.emit(LoadEvent::Ready(loaded));
    }

    /// One thread per path. Events arrive interleaved, in whatever order the
    /// threads produce them.
    pub fn spawn(&self, paths: Vec<PathBuf>) -> LoadHandle {
        let (tx, rx) = mpsc::channel();
        let cancel = CancelToken::default();
        let expected = paths.len();
        tracing::debug!("spawning {expected} loader thread(s)");

        for path in paths {
            let (mut tx, cancel, loader) = (tx.clone(), cancel.clone(), self.clone());
            std::thread::spawn(move || loader.load_path(&path, &cancel, &mut tx));
        }

        LoadHandle {
            rx,
            cancel,
            expected,
        }
    }
}

/// A load in flight: the event stream plus the switch that stops it.
pub struct LoadHandle {
    pub rx: Receiver<LoadEvent>,
    pub cancel: CancelToken,
    /// Files in this load — one `Ready` or `Failed` each.
    pub expected: usize,
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cancel_token_is_shared_across_clones() {
        let cancel = CancelToken::default();
        let clone = cancel.clone();
        cancel.cancel();
        assert!(clone.is_cancelled());
    }
}
