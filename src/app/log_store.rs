use std::collections::HashMap;

use crate::analysis::SpectralAnalysis;
use crate::loader;
use crate::parser::ParsedLog;

pub(super) struct LoadedLog {
    pub(super) log: Vec<ParsedLog>,
    /// One `SpectralAnalysis` per sublog in `log`, computed once at load time.
    pub(super) analysis: Vec<SpectralAnalysis>,
    pub(super) active_sublog: usize,
}

impl From<loader::LoadedLog> for LoadedLog {
    fn from(loaded: loader::LoadedLog) -> Self {
        Self {
            log: loaded.logs,
            analysis: loaded.analysis,
            active_sublog: 0,
        }
    }
}

pub(super) enum LoadState {
    Idle,
    Loading {
        handle: loader::LoadHandle,
        /// Completion 0..=1 per file seen so far. Files whose thread hasn't
        /// reported yet are simply absent, and count as 0.
        progress: HashMap<String, f32>,
        current: String,
    },
}

impl LoadState {
    /// Mean completion across every file in the load, so the bar advances
    /// within a single sublog instead of only when a whole file lands.
    pub(super) fn fraction(&self) -> f32 {
        match self {
            LoadState::Idle => 0.0,
            LoadState::Loading {
                handle, progress, ..
            } => progress.values().sum::<f32>() / handle.expected.max(1) as f32,
        }
    }
}

/// Owns the loaded logs and which one is shown. Selection is single-choice —
/// exactly one log is selected once any are loaded — enforced here instead of
/// as a `bool` on each log, which let two logs disagree about which was
/// "selected" (see 2006f0f).
#[derive(Default)]
pub(super) struct LogStore {
    logs: Vec<LoadedLog>,
    selected: Option<usize>,
}

impl LogStore {
    /// Auto-selects only when this is the first log ever loaded — later
    /// loads never steal focus from what the user is already looking at.
    pub(super) fn push(&mut self, log: LoadedLog) {
        self.logs.push(log);
        if self.selected.is_none() {
            self.selected = Some(self.logs.len() - 1);
        }
    }

    pub(super) fn select(&mut self, index: usize) {
        debug_assert!(index < self.logs.len());
        self.selected = Some(index);
    }

    pub(super) fn current_flight(&self) -> Option<(&ParsedLog, &SpectralAnalysis)> {
        let loaded = self.logs.get(self.selected?)?;
        let idx = loaded.active_sublog;
        Some((loaded.log.get(idx)?, loaded.analysis.get(idx)?))
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut LoadedLog, bool)> {
        let selected = self.selected;
        self.logs
            .iter_mut()
            .enumerate()
            .map(move |(i, loaded)| (i, loaded, selected == Some(i)))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn loaded_log() -> LoadedLog {
        LoadedLog {
            log: vec![ParsedLog::default()],
            analysis: vec![SpectralAnalysis::default()],
            active_sublog: 0,
        }
    }

    #[test]
    fn push_selects_when_empty() {
        let mut store = LogStore::default();
        store.push(loaded_log());
        assert_eq!(store.selected, Some(0));
    }

    #[test]
    fn push_does_not_steal_focus() {
        let mut store = LogStore::default();
        store.push(loaded_log());
        store.push(loaded_log());
        assert_eq!(store.selected, Some(0));
    }

    #[test]
    fn select_persists_across_push() {
        let mut store = LogStore::default();
        store.push(loaded_log());
        store.push(loaded_log());
        store.select(1);
        store.push(loaded_log());
        assert_eq!(store.selected, Some(1));
    }

    #[test]
    fn current_flight_none_when_empty() {
        let store = LogStore::default();
        assert!(store.current_flight().is_none());
    }

    #[test]
    fn current_flight_some_when_selected() {
        let mut store = LogStore::default();
        store.push(loaded_log());
        assert!(store.current_flight().is_some());
    }

    #[test]
    #[should_panic]
    fn select_out_of_range_panics() {
        let mut store = LogStore::default();
        store.select(0);
    }
}
