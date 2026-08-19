//! Which flights are being compared, and which colour is which flight.
//!
//! One widget does both, and it takes the compare set and the catalog rather
//! than any one panel's state: PSD and Frequency across two logs is the same
//! comparison with the same picker (same x axis in Hz, no time alignment), and
//! a picker welded inside the step response panel would be duplicated or
//! refactored under pressure the day that lands.

use egui::{Color32, RichText, Stroke, Ui};

use crate::app::colors::{self, COMPARE_SLOTS};
use crate::app::log_store::{FlightCatalog, FlightKey};
use crate::parser::Metadata;

/// Chip labels are truncated from the left, so the discriminating tail of
/// `blackbox_012.bbl` survives where the shared head does not.
const MAX_CHIP_CHARS: usize = 24;

/// The flights compared against the base, in the order they were added.
///
/// The base itself is never held here: it is the sidepanel's selection, read
/// fresh every frame, so a sidepanel switch moves which flight sits in slot 0
/// without this ever going stale. Identity is by [`FlightKey`] — never by
/// index, which `LogStore::remove` shifts under whoever held one.
#[derive(Default)]
pub(in crate::app) struct CompareSet {
    added: Vec<FlightKey>,
}

impl CompareSet {
    /// Slot order, base first: slot 0 is always the sidepanel's selection.
    pub(in crate::app) fn slots(&self, base: FlightKey) -> Vec<FlightKey> {
        std::iter::once(base)
            .chain(self.added.iter().copied())
            .collect()
    }

    /// Drops what can no longer be drawn or should never have been: a flight
    /// whose file was removed, and the base flight itself — a flight is not its
    /// own comparison. The flights that survive keep their order, so removing
    /// one entry cannot move another entry's colour.
    fn reconcile(&mut self, base: FlightKey, resolves: impl Fn(FlightKey) -> bool) {
        self.added.retain(|&key| key != base && resolves(key));
    }

    /// At the cap the picker greys its unchecked entries rather than evicting
    /// anything: dropping the flight someone is comparing against is the one
    /// thing that must not happen.
    fn is_full(&self) -> bool {
        self.added.len() + 1 >= COMPARE_SLOTS
    }

    fn contains(&self, key: FlightKey) -> bool {
        self.added.contains(&key)
    }

    /// Adding past the cap does nothing — the caller has already greyed the
    /// entry out, and this is the same rule stated where it cannot be skipped.
    fn toggle(&mut self, key: FlightKey) {
        match self.added.iter().position(|&k| k == key) {
            Some(i) => {
                self.added.remove(i);
            }
            None if !self.is_full() => self.added.push(key),
            None => {}
        }
    }
}

/// Draws the chip row and the picker, and returns the flights to draw in slot
/// order — slot 0 first. The chips are the legend, which is what lets the
/// panels below draw no legend of their own.
pub(in crate::app) fn show(
    ui: &mut Ui,
    set: &mut CompareSet,
    base: FlightKey,
    catalog: &dyn FlightCatalog,
) -> Vec<FlightKey> {
    set.reconcile(base, |key| catalog.resolve(key).is_some());

    let palette = colors::palette(ui.ctx());
    // The base is the tab's own flight, resolved before any panel ran. If it is
    // gone there is no comparison to draw against either.
    let Some(base_meta) = catalog.resolve(base).map(|flight| flight.metadata) else {
        return Vec::new();
    };
    let mut remove = None;

    let slots = set.slots(base);

    ui.horizontal_wrapped(|ui| {
        for (slot, &key) in slots.iter().enumerate() {
            let Some(flight) = catalog.resolve(key) else {
                continue;
            };
            let label = catalog.label(key).unwrap_or_default();
            // A rate change moves what the shared minimum stick input selects,
            // so the two curves stop describing the same manoeuvres.
            let confounded = slot > 0 && flight.metadata.rates != base_meta.rates;

            let chip = chip(
                ui,
                colors::slot_color(&palette, slot),
                &label,
                slot > 0,
                confounded,
            )
            .on_hover_text(hover(flight.metadata, base_meta, slot == 0, confounded));

            if slot > 0 && chip.clicked() {
                remove = Some(key);
            }
        }

        picker(ui, set, base, catalog);
    });

    match remove {
        // Recomputed: the row above drew the set as it was before the click.
        Some(key) => {
            set.toggle(key);
            set.slots(base)
        }
        None => slots,
    }
}

/// A bordered chip in its slot colour. The colour is the whole point, so it is
/// drawn rather than taken from an accent enum that has no slot in it.
fn chip(
    ui: &mut Ui,
    color: Color32,
    label: &str,
    removable: bool,
    confounded: bool,
) -> egui::Response {
    let warning = match confounded {
        true => format!("{} ", egui_phosphor::regular::WARNING),
        false => String::new(),
    };
    let close = match removable {
        true => format!("  {}", egui_phosphor::regular::X),
        false => String::new(),
    };
    let text = format!("{warning}{}{close}", truncate_left(label));

    ui.add(
        egui::Button::new(RichText::new(text).color(color))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, color)),
    )
}

/// The candidate list, grouped by file. The base is not a candidate.
fn picker(ui: &mut Ui, set: &mut CompareSet, base: FlightKey, catalog: &dyn FlightCatalog) {
    let full = set.is_full();

    let candidates = catalog.flights();

    ui.menu_button("+ compare", |ui| {
        let mut file = None;

        for &key in &candidates {
            if key == base {
                continue;
            }
            let Some(flight) = catalog.resolve(key) else {
                continue;
            };

            if file.as_deref() != Some(flight.metadata.file_name.as_str()) {
                file = Some(flight.metadata.file_name.clone());
                ui.label(RichText::new(&flight.metadata.file_name).strong());
            }

            let mut checked = set.contains(key);
            let entry = ui.add_enabled(
                checked || !full,
                egui::Checkbox::new(&mut checked, catalog.label(key).unwrap_or_default()),
            );

            if entry.changed() {
                set.toggle(key);
            }
            if full && !set.contains(key) {
                entry.on_hover_text(format!(
                    "{COMPARE_SLOTS} flights at once, including the selected one — remove a chip \
                     to make room."
                ));
            }
        }

        if candidates.iter().all(|&key| key == base) {
            ui.label(RichText::new("no other flight is loaded").weak());
        }
    });
}

/// Everything the chip label has no room for. `looptime_us` gets a line rather
/// than a glyph: a different loop rate changes the noise, not which manoeuvres
/// the analysis selected.
fn hover(metadata: &Metadata, base: &Metadata, is_base: bool, confounded: bool) -> String {
    let mut lines = vec![
        metadata.craft_name.clone(),
        format!("{} | {}", metadata.firmware, metadata.board),
        format!("{:.1} s", metadata.duration.as_secs_f32()),
    ];

    if let Some(rates) = &metadata.rates {
        lines.push(rates.to_string());
    }
    if let Some(looptime) = metadata.looptime_us {
        let differs = !is_base && Some(looptime) != base.looptime_us;
        lines.push(match differs {
            // A line, not a glyph: a different loop rate changes the noise the
            // filters see, not which manoeuvres the analysis selected.
            true => format!(
                "{:.0} Hz loop \u{2014} the selected flight ran at {:.0} Hz",
                1e6 / looptime as f32,
                base.looptime_us.map(|us| 1e6 / us as f32).unwrap_or(0.0)
            ),
            false => format!("{:.0} Hz loop", 1e6 / looptime as f32),
        });
    }
    if confounded {
        lines.push(
            "Flown on a different rate curve: the shared minimum stick input then selects \
             different manoeuvres in each flight, so the curves answer slightly different \
             questions."
                .to_string(),
        );
    }
    lines.push(
        match is_base {
            true => "The flight selected in the sidepanel. Select another there to move it.",
            false => "Click to stop comparing this flight.",
        }
        .to_string(),
    );

    lines.join("\n")
}

/// From the left, so that the tail — which is what differs between
/// `blackbox_011.bbl` and `blackbox_012.bbl` — is the part kept.
fn truncate_left(label: &str) -> String {
    let chars: Vec<char> = label.chars().collect();
    match chars.len() > MAX_CHIP_CHARS {
        true => format!(
            "…{}",
            chars[chars.len() - MAX_CHIP_CHARS..]
                .iter()
                .collect::<String>()
        ),
        false => label.to_string(),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::log_store::LogId;

    fn keys(count: usize) -> Vec<FlightKey> {
        (0..count as u64).map(|i| (LogId::new(i), 0)).collect()
    }

    /// Every key resolves — the default for a set whose files are all loaded.
    fn all() -> impl Fn(FlightKey) -> bool {
        |_| true
    }

    fn set_of(added: &[FlightKey]) -> CompareSet {
        let mut set = CompareSet::default();
        for &key in added {
            set.toggle(key);
        }
        set
    }

    #[test]
    fn the_base_is_always_slot_zero() {
        let flights = keys(3);
        let set = set_of(&flights[1..]);

        assert_eq!(set.slots(flights[0]), flights);
    }

    /// The cap is four including the base, and reaching it must not silently
    /// evict what is already being compared.
    #[test]
    fn a_fifth_flight_cannot_be_added() {
        let flights = keys(5);
        let mut set = set_of(&flights[1..4]);

        set.toggle(flights[4]);

        assert_eq!(set.slots(flights[0]), flights[..4].to_vec());
    }

    /// A flight is not its own comparison: selecting a compared flight in the
    /// sidepanel drops it from the set rather than drawing it twice.
    #[test]
    fn a_base_already_in_the_set_leaves_no_duplicate() {
        let flights = keys(3);
        let mut set = set_of(&flights[1..]);

        set.reconcile(flights[1], all());

        assert_eq!(set.slots(flights[1]), vec![flights[1], flights[2]]);
    }

    /// The load-bearing one: a removed file's id resolves to nothing, and the
    /// flights after it keep their slots rather than shuffling colour.
    #[test]
    fn an_unresolvable_flight_is_dropped_without_moving_the_others() {
        let flights = keys(4);
        let mut set = set_of(&flights[1..]);
        let gone = flights[2];

        set.reconcile(flights[0], move |key| key != gone);

        assert_eq!(
            set.slots(flights[0]),
            vec![flights[0], flights[1], flights[3]]
        );
    }

    /// Insertion order, so removing one chip cannot move another chip's colour.
    #[test]
    fn removing_an_entry_leaves_the_rest_in_their_slots() {
        let flights = keys(4);
        let mut set = set_of(&flights[1..]);

        set.toggle(flights[1]);

        assert_eq!(
            set.slots(flights[0]),
            vec![flights[0], flights[2], flights[3]]
        );
    }

    #[test]
    fn a_long_name_keeps_the_end_that_tells_it_apart() {
        let truncated = truncate_left("a-very-long-craft-name-blackbox_012.bbl");

        assert!(truncated.starts_with('…'));
        assert!(truncated.ends_with("blackbox_012.bbl"), "{truncated}");
    }
}
