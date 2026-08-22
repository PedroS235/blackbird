use std::collections::HashMap;

use crate::analysis::Analysis;
use crate::loader;
use crate::parser::{FlightData, Metadata, ParsedLog};

/// A loaded file's identity, minted in [`LogStore::push`] and never reused.
///
/// Panel state names flights by id rather than by index: `remove` shifts every
/// later index down, so an index held outside the store resolves to a
/// *different* file afterwards and keeps being drawn under the old label. An id
/// of a removed file resolves to `None` for good, which is the failure mode a
/// compare set can survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct LogId(u64);

impl LogId {
    /// Ids are minted by the store; this is for the tests that need a handful
    /// of distinct ones without standing one up.
    #[cfg(test)]
    pub(super) fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// One flight: which file it came from, and which sublog inside it.
pub(super) type FlightKey = (LogId, usize);

/// One flight's data, resolved out of the store — the same three borrows a tab
/// is handed for its own flight.
pub(super) struct FlightRef<'a> {
    pub(super) flight: &'a FlightData,
    pub(super) analysis: &'a Analysis,
    pub(super) metadata: &'a Metadata,
}

/// Read-only view of every loaded flight, for the panels that draw more than
/// their own. Deliberately not `&LogStore`: `select` and `remove` have to stay
/// out of a panel's reach, since the sidepanel iterates the store mutably in
/// the same frame.
pub(super) trait FlightCatalog {
    /// Every loaded flight, in file order then sublog order.
    fn flights(&self) -> Vec<FlightKey>;
    /// The sidepanel's selection — the base flight of any comparison.
    fn selected(&self) -> Option<FlightKey>;
    fn resolve(&self, key: FlightKey) -> Option<FlightRef<'_>>;
    /// File name and sublog number, the way a chip or a menu entry says it.
    fn label(&self, key: FlightKey) -> Option<String>;
}

pub(super) struct LoadedLog {
    /// Assigned by [`LogStore::push`], the only place ids are minted.
    id: LogId,
    pub(super) log: Vec<ParsedLog>,
    /// One `Analysis` per sublog in `log`, computed once at load time.
    pub(super) analysis: Vec<Analysis>,
    pub(super) active_sublog: usize,
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
    /// Monotonic, never rewound on removal — index reuse is what ids exist to
    /// avoid.
    next_id: u64,
}

impl LogStore {
    /// Auto-selects only when this is the first log ever loaded — later
    /// loads never steal focus from what the user is already looking at.
    pub(super) fn push(&mut self, loaded: loader::LoadedLog) {
        let id = LogId(self.next_id);
        self.next_id += 1;
        tracing::debug!(
            "stored {} as {id:?} with {} sublog(s)",
            loaded.file_name,
            loaded.logs.len()
        );

        self.logs.push(LoadedLog {
            id,
            log: loaded.logs,
            analysis: loaded.analysis,
            active_sublog: 0,
        });
        if self.selected.is_none() {
            self.selected = Some(self.logs.len() - 1);
            tracing::debug!("{id:?} selected — the first log loaded");
        }
    }

    pub(super) fn select(&mut self, index: usize) {
        debug_assert!(index < self.logs.len());
        self.selected = Some(index);
        tracing::debug!("selected {}", self.name_of(index));
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut LoadedLog, bool)> {
        let selected = self.selected;
        self.logs
            .iter_mut()
            .enumerate()
            .map(move |(i, loaded)| (i, loaded, selected == Some(i)))
    }

    /// Removes a log and keeps `selected` pointing at the same log it did
    /// before (shifted down if it sat after `index`), falling back to the
    /// next log — or `None` once the store is empty — when `index` itself
    /// was selected.
    pub(super) fn remove(&mut self, index: usize) {
        debug_assert!(index < self.logs.len());
        tracing::info!("closed {}", self.name_of(index));
        self.logs.remove(index);
        self.selected = match self.selected {
            Some(sel) if sel == index => {
                if self.logs.is_empty() {
                    None
                } else {
                    Some(sel.min(self.logs.len() - 1))
                }
            }
            Some(sel) if sel > index => Some(sel - 1),
            sel => sel,
        };
    }

    /// What to call a log in a diagnostic. Never fails: an out-of-range index
    /// is a bug the caller's `debug_assert` catches, and a log line is not
    /// where that should surface.
    fn name_of(&self, index: usize) -> String {
        self.logs
            .get(index)
            .and_then(|loaded| loaded.log.first())
            .map(|parsed| parsed.metadata.file_name.clone())
            .unwrap_or_else(|| format!("log #{index}"))
    }

    fn find(&self, id: LogId) -> Option<&LoadedLog> {
        self.logs.iter().find(|loaded| loaded.id == id)
    }
}

impl FlightCatalog for LogStore {
    fn flights(&self) -> Vec<FlightKey> {
        self.logs
            .iter()
            .flat_map(|loaded| (0..loaded.log.len()).map(move |i| (loaded.id, i)))
            .collect()
    }

    fn selected(&self) -> Option<FlightKey> {
        let loaded = self.logs.get(self.selected?)?;
        Some((loaded.id, loaded.active_sublog))
    }

    fn resolve(&self, (id, sublog): FlightKey) -> Option<FlightRef<'_>> {
        let loaded = self.find(id)?;
        let parsed = loaded.log.get(sublog)?;
        Some(FlightRef {
            flight: &parsed.flight_data,
            analysis: loaded.analysis.get(sublog)?,
            metadata: &parsed.metadata,
        })
    }

    fn label(&self, (id, sublog): FlightKey) -> Option<String> {
        let loaded = self.find(id)?;
        let name = loaded.log.get(sublog)?.metadata.file_name.clone();
        Some(match loaded.log.len() {
            1 => name,
            count => format!("{name} · log {}/{count}", sublog + 1),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// One file, named, with `sublogs` sublogs — the name is what a resolved
    /// flight is recognised by.
    fn file(name: &str, sublogs: usize) -> loader::LoadedLog {
        loader::LoadedLog {
            file_name: name.to_string(),
            logs: (0..sublogs)
                .map(|i| ParsedLog {
                    metadata: Metadata {
                        file_name: name.to_string(),
                        ..Default::default()
                    },
                    log_index: i,
                    ..Default::default()
                })
                .collect(),
            analysis: (0..sublogs).map(|_| Analysis::default()).collect(),
        }
    }

    fn loaded_log() -> loader::LoadedLog {
        file("log.bbl", 1)
    }

    fn ids(store: &LogStore) -> Vec<LogId> {
        store.logs.iter().map(|loaded| loaded.id).collect()
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
    fn nothing_resolves_when_empty() {
        let store = LogStore::default();
        assert!(store.selected().is_none());
        assert!(store.flights().is_empty());
    }

    #[test]
    fn the_selected_flight_resolves() {
        let mut store = LogStore::default();
        store.push(loaded_log());
        let key = store.selected().expect("a log is selected");
        assert!(store.resolve(key).is_some());
    }

    /// The bound is a `debug_assert`: a mis-wired click is a caller bug worth
    /// catching in development, and not worth killing a pilot's app over in
    /// release, where the index is simply stored and `selected()` resolves to
    /// `None`. So the panic exists only where the assertion does.
    #[test]
    #[should_panic]
    #[cfg_attr(not(debug_assertions), ignore = "select's bound is a debug_assert")]
    fn select_out_of_range_panics() {
        let mut store = LogStore::default();
        store.select(0);
    }

    /// Ids are minted, not derived from a position, so a removal cannot hand a
    /// later log an id an earlier one already used.
    #[test]
    fn ids_are_never_reused() {
        let mut store = LogStore::default();
        for name in ["a", "b", "c"] {
            store.push(file(name, 1));
        }
        let before = ids(&store);

        store.remove(0);
        store.push(file("d", 1));

        let fresh = *ids(&store).last().expect("just pushed");
        assert!(
            !before.contains(&fresh),
            "{fresh:?} was already handed out: {before:?}"
        );
    }

    /// The load-bearing one: this is exactly what an index-keyed compare set
    /// gets wrong. The removed flight is gone, and the flights that shifted
    /// down are still the same flights under the same ids.
    #[test]
    fn a_removal_does_not_move_the_flights_that_outlived_it() {
        let mut store = LogStore::default();
        for name in ["a", "b", "c"] {
            store.push(file(name, 1));
        }
        let [a, b, c] = <[LogId; 3]>::try_from(ids(&store)).expect("three logs");

        store.remove(0);

        assert!(
            store.resolve((a, 0)).is_none(),
            "a removed log still resolves"
        );
        for (id, name) in [(b, "b"), (c, "c")] {
            let resolved = store.resolve((id, 0)).expect("outlived the removal");
            assert_eq!(resolved.metadata.file_name, name);
        }
    }

    /// A sublog index past the end of *its own* file must not fall through to
    /// another file's sublog.
    #[test]
    fn a_sublog_past_the_end_does_not_resolve() {
        let mut store = LogStore::default();
        store.push(file("a", 2));
        store.push(file("b", 2));
        let id = ids(&store)[0];

        assert!(store.resolve((id, 1)).is_some());
        assert!(store.resolve((id, 2)).is_none());
    }

    #[test]
    fn every_sublog_of_every_file_is_offered_once() {
        let mut store = LogStore::default();
        store.push(file("a", 3));
        store.push(file("b", 1));
        let [a, b] = <[LogId; 2]>::try_from(ids(&store)).expect("two logs");

        assert_eq!(store.flights(), vec![(a, 0), (a, 1), (a, 2), (b, 0)]);
    }

    /// The sublog number is only worth saying when the file has more than one.
    #[test]
    fn labels_name_the_file_and_the_sublog() {
        let mut store = LogStore::default();
        store.push(file("multi.bbl", 3));
        store.push(file("single.bfl", 1));
        let [multi, single] = <[LogId; 2]>::try_from(ids(&store)).expect("two logs");

        assert_eq!(
            store.label((multi, 1)).as_deref(),
            Some("multi.bbl · log 2/3")
        );
        assert_eq!(store.label((single, 0)).as_deref(), Some("single.bfl"));
        assert_eq!(store.label((single, 1)), None);
    }
}
