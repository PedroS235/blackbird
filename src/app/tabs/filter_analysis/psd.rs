use egui::{Align2, RichText, Ui, Vec2b};
use egui_plot::{HLine, Line, Plot, PlotPoint, PlotPoints, PlotUi, Span, Text, VLine};
use elegance::Palette;

use super::drawn_axes;
use crate::analysis::{
    AxisSpectral, DynNotchReach, FilterOverlay, FilterResponse, FrequencyPeak, HarmonicBand,
    OverlayFamily, OverlayShape, SpectralAnalysis,
};
use crate::app::colors;
use crate::app::tabs::stacked_plot_height;
use crate::app::ui::harmonic_key;
use crate::app::ui::hover;
use crate::app::ui::overlay_menu::{self, OverlayVisibility};
use crate::parser::{Axis, PerAxis};
use crate::signal::fft::Psd as PsdData;

/// How many peaks carry a written label. The rest are drawn as bare lines:
/// past three the labels overlap each other and the curve underneath, and the
/// three loudest are the ones a pilot is filtering for anyway.
const LABELLED_PEAKS: usize = 3;

/// Fill alpha for a band the filter is actually attenuating. Low enough that
/// the curve reads through a stack of overlapping harmonics.
const BAND_FILL_ALPHA: u8 = 28;

/// A harmonic's stretch is drawn over the raw trace, so it is drawn thicker —
/// at the same width the recolour would read as the curve rather than as a mark
/// on it.
const HARMONIC_WIDTH: f32 = 2.0;

/// The traced response is a gain curve, and the plot's y axis is signal power.
/// Its zero is pinned to the loudest bin of this axis's own spectrum, so the V
/// hangs over the noise it is cutting, at the same decibels per pixel, and
/// stays put when the pilot zooms.
fn response_anchor_db(spec: &AxisSpectral) -> f64 {
    spec.raw_psd
        .power_db
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Keeps an explicit checkbox rather than a legend: the filtered trace is a
/// conditional build, not a hide, and the panel emits a named marker per
/// detected peak — a legend here would list a dozen frequency labels.
#[derive(Default)]
pub(super) struct Psd {
    filtered_visible: PerAxis<bool>,
    overlays: OverlayVisibility,
}

impl Psd {
    pub(super) fn show(&mut self, ui: &mut Ui, analysis: &SpectralAnalysis) {
        // A log whose peaks all sat inside the noise floor cannot fill the
        // peaks toggle either, and it greys out like any other.
        let has_peaks = Axis::ALL
            .iter()
            .filter_map(|&axis| analysis.axis(axis))
            .any(|spec| !spec.peaks.is_empty());
        overlay_menu::show(
            ui,
            &mut self.overlays,
            &OverlayFamily::ALL,
            |family| analysis.overlays.iter().any(|o| o.family == family),
            Some(has_peaks),
        );

        let palette = colors::palette(ui.ctx());

        // Twelve outlines carry no in-plot labels, so the key to them lives
        // here — and only while the family is drawing, so a clean panel stays
        // clean.
        let bands = analysis.harmonic_bands();
        if self.overlays.shows(OverlayFamily::Harmonics) && !bands.is_empty() {
            harmonic_key::show(ui, &palette, bands);
        }
        ui.add_space(4.0);

        // After the toggle row, and over the axes that draw: measuring first
        // would size the plots against height the row then took — including
        // the second line it wraps onto on a narrow window.
        let plot_height = stacked_plot_height(ui, drawn_axes(analysis));
        let visible: Vec<&FilterOverlay> = analysis
            .overlays
            .iter()
            .filter(|o| self.overlays.shows(o.family))
            .collect();

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} psd", axis.name())).strong());
                ui.checkbox(&mut self.filtered_visible[axis], "show filtered");
            });

            let mut readout_series: Vec<(String, &[f64], &[f64])> =
                vec![("raw".into(), &spec.raw_psd.freq_hz, &spec.raw_psd.power_db)];
            if self.filtered_visible[axis]
                && let Some(filtered_psd) = &spec.filtered_psd
            {
                readout_series.push((
                    "filtered".into(),
                    &filtered_psd.freq_hz,
                    &filtered_psd.power_db,
                ));
            }

            Plot::new(plot_id(axis))
                .label_formatter(hover::readout("Hz", 1, readout_series))
                .height(plot_height)
                .x_axis_label("Hz")
                .y_axis_label("dB")
                .allow_zoom(Vec2b::new(true, true))
                .allow_scroll(Vec2b::new(true, false))
                .allow_drag(Vec2b::new(true, true))
                .show_y(true)
                .show_x(false)
                .show_crosshair(false)
                .show(ui, |plot_ui| {
                    let raw_points: PlotPoints = spec
                        .raw_psd
                        .freq_hz
                        .iter()
                        .zip(&spec.raw_psd.power_db)
                        .map(|(&f, &v)| [f, v])
                        .collect();
                    plot_ui.line(Line::new("raw", raw_points).color(palette.text_faint));

                    if self.filtered_visible[axis]
                        && let Some(filtered_psd) = &spec.filtered_psd
                    {
                        let filtered_points: PlotPoints = filtered_psd
                            .freq_hz
                            .iter()
                            .zip(&filtered_psd.power_db)
                            .map(|(&f, &v)| [f, v])
                            .collect();
                        plot_ui.line(
                            Line::new("filtered", filtered_points)
                                .color(colors::axis_color(&palette, axis)),
                        );
                    }

                    let anchor_db = response_anchor_db(spec);
                    for overlay in &visible {
                        draw_overlay(plot_ui, &palette, overlay, axis, anchor_db, &spec.raw_psd);
                    }
                    if self.overlays.shows_peaks() {
                        draw_peaks(plot_ui, &palette, spec);
                    }
                });

            // Under the plot rather than on it: the verdict has to survive the
            // pilot zooming the offending peak off screen. It follows the
            // peaks switch — a pilot who hid the peaks is not asking to be
            // told about them in prose instead.
            if let Some(prose) = self
                .overlays
                .shows_peaks()
                .then(|| out_of_reach_prose(spec, &analysis.overlays))
                .flatten()
            {
                ui.label(RichText::new(prose).color(palette.warning));
            }
        }
    }
}

/// Renaming this throws away the persisted zoom of every pilot who had one.
fn plot_id(axis: Axis) -> String {
    format!("psd_plot_{}", axis.name())
}

fn draw_overlay(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    overlay: &FilterOverlay,
    axis: Axis,
    anchor_db: f64,
    psd: &PsdData,
) {
    let color = colors::filter_color(palette);

    match &overlay.shape {
        OverlayShape::Line { hz } => {
            plot_ui.vline(VLine::new(overlay.label.clone(), *hz).color(color))
        }
        OverlayShape::Band { low_hz, high_hz } => plot_ui.span(
            Span::new(overlay.label.clone(), *low_hz..=*high_hz)
                .fill(color.gamma_multiply_u8(BAND_FILL_ALPHA))
                .border_color(color),
        ),
        OverlayShape::Harmonics(bands) => draw_harmonics(plot_ui, palette, bands, psd),
        OverlayShape::Response(response) => {
            draw_response(plot_ui, palette, response, anchor_db, &overlay.label)
        }
        OverlayShape::Traced(per_axis) => {
            if let Some(response) = per_axis[axis].as_ref() {
                draw_response(plot_ui, palette, response, anchor_db, &overlay.label);
            }
        }
    }
}

/// The spectrum's own curve, recoloured over the frequencies each motor spent
/// the flight at: hue is the motor, style is the order.
///
/// Not a span. Twelve of those are twenty-four vertical edges, and a pilot
/// reads a bracket as a boundary rather than as noise — while every point of
/// the recoloured run is measured data, saying *this part of your spectrum is
/// motor 3's second harmonic*. A peak with no coloured run over it is a peak no
/// motor explains, which is the whole reason the bands were narrowed.
///
/// No in-plot labels. The key is the legend row above the plots; twelve names
/// drawn on the curve would bury it.
fn draw_harmonics(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    bands: &[HarmonicBand],
    psd: &PsdData,
) {
    for band in bands {
        let Some(run) = spectrum_run(psd, band) else {
            continue;
        };
        plot_ui.line(
            Line::new(harmonic_key::band_name(band), run)
                .color(harmonic_key::band_color(palette, band))
                .style(harmonic_key::order_style(band.order))
                .width(HARMONIC_WIDTH),
        );
    }
}

/// The spectrum's points across one band, or `None` where the band falls
/// outside the spectrum entirely — a third harmonic past Nyquist has no curve
/// to recolour.
///
/// Widened by a bin either side: a motor that held one frequency has a band
/// narrower than the FFT's resolution, and a run of one point draws nothing at
/// all.
fn spectrum_run(psd: &PsdData, band: &HarmonicBand) -> Option<PlotPoints<'static>> {
    let n = psd.freq_hz.len().min(psd.power_db.len());
    let freq = psd.freq_hz.get(..n)?;
    if n < 2 || band.low_hz > freq[n - 1] || band.high_hz < freq[0] {
        return None;
    }

    let start = freq
        .partition_point(|&f| f < band.low_hz)
        .saturating_sub(1)
        .min(n - 2);
    let end = (freq.partition_point(|&f| f <= band.high_hz) + 1)
        .max(start + 2)
        .min(n);

    Some(
        freq[start..end]
            .iter()
            .zip(&psd.power_db[start..end])
            .map(|(&f, &v)| [f, v])
            .collect(),
    )
}

/// What a filter actually took off, as the shape it really has.
///
/// A notch is a V and a lowpass is a rolloff. Both were drawn as a line or a
/// band, which a pilot reads as "everything in here is gone" — the one thing
/// neither does. A filter that moved during the flight is the average of the
/// settings it moved through, so one held still draws its own curve and one
/// swept draws the shallower, wider average of the corners it passed.
fn draw_response(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    response: &FilterResponse,
    anchor_db: f64,
    label: &str,
) {
    if !anchor_db.is_finite() {
        return;
    }
    let color = colors::filter_color(palette);

    let curve: PlotPoints = response
        .freq_hz
        .iter()
        .zip(&response.gain_db)
        .map(|(&f, &gain)| [f, anchor_db + gain])
        .collect();
    plot_ui.line(Line::new(label.to_string(), curve).color(color));

    // The line every curve is measured down from — without it a curve is a
    // shape with no scale, and "how far down" is the whole question.
    plot_ui.hline(HLine::new(String::new(), anchor_db).color(color.gamma_multiply(0.4)));

    // Curves share one colour, so each says which stage it is at the point it
    // starts taking something — a notch's near edge, a lowpass's corner.
    if let Some((freq, gain)) = response.corner() {
        plot_ui.text(
            Text::new(
                format!("{label}_label"),
                PlotPoint::new(freq, anchor_db + gain),
                label,
            )
            .color(color)
            .anchor(Align2::CENTER_TOP),
        );
    }
}

/// One mark per peak, and a label on the loudest few. A peak the dynamic notch
/// can never reach takes the warning colour.
fn draw_peaks(plot_ui: &mut PlotUi<'_>, palette: &Palette, spec: &AxisSpectral) {
    let mut by_amplitude: Vec<usize> = (0..spec.peaks.len()).collect();
    by_amplitude.sort_by(|&a, &b| {
        spec.peaks[b]
            .amplitude_db
            .total_cmp(&spec.peaks[a].amplitude_db)
    });
    by_amplitude.truncate(LABELLED_PEAKS);

    for (i, peak) in spec.peaks.iter().enumerate() {
        let out_of_reach = peak.dyn_notch_reach.is_some_and(DynNotchReach::is_outside);
        let color = match out_of_reach {
            true => palette.warning,
            false => colors::peak_color(palette),
        };
        let label = peak_label(peak);
        let labelled = by_amplitude.contains(&i);

        // The label belongs to the peak once. Where the text draws it, the
        // line carries no name of its own — the two used to say the same
        // thing, and half the marks on the plot were the other half repeated.
        let line_name = match labelled {
            true => String::new(),
            false => label.clone(),
        };
        plot_ui.vline(VLine::new(line_name, peak.freq_hz).color(color));

        if labelled {
            plot_ui.text(
                Text::new(
                    format!("peak_{i}"),
                    PlotPoint::new(peak.freq_hz, peak.amplitude_db),
                    label,
                )
                .color(color)
                .anchor(Align2::CENTER_BOTTOM),
            );
        }
    }
}

/// The attenuation is a number the analysis has always computed and the panel
/// has never shown — it is how a pilot judges the filter chain without reading
/// a second plot.
fn peak_label(peak: &FrequencyPeak) -> String {
    let harmonic = match peak.harmonic_of {
        Some(_) => " (harmonic)",
        None => "",
    };
    let attenuation = match peak.attenuated_db {
        // A filter chain that leaves a peak louder than it found it is a real
        // outcome, and must not read as a cut of the same size.
        Some(db) if db < 0.0 => format!(" · {:.0} dB louder filtered", -db),
        Some(db) => format!(" · {db:.0} dB filtered"),
        None => String::new(),
    };

    format!("{:.0} Hz{harmonic}{attenuation}", peak.freq_hz)
}

/// The count and the bound it exceeded, stated in words. Prose under a plot
/// for what the plot cannot hold is the idiom the step response panel uses.
fn out_of_reach_prose(spec: &AxisSpectral, overlays: &[FilterOverlay]) -> Option<String> {
    let (low_hz, high_hz) = dyn_notch_range(overlays)?;

    let clauses: Vec<String> = [
        (
            spec.peaks_reaching(DynNotchReach::AboveMax),
            "above",
            high_hz,
        ),
        (
            spec.peaks_reaching(DynNotchReach::BelowMin),
            "below",
            low_hz,
        ),
    ]
    .into_iter()
    .filter(|&(n, _, _)| n > 0)
    .map(|(n, side, bound)| {
        let peaks = match n {
            1 => "peak sits",
            _ => "peaks sit",
        };
        format!("{n} {peaks} {side} the dynamic notch's {bound:.0} Hz")
    })
    .collect();

    (!clauses.is_empty()).then(|| {
        format!(
            "{} — the tracker can never reach them.",
            clauses.join(", and ")
        )
    })
}

/// The configured range, read back off the overlay that carries it rather than
/// re-plumbed from the header — the band drawn and the prose written have to
/// be the same claim.
fn dyn_notch_range(overlays: &[FilterOverlay]) -> Option<(f64, f64)> {
    overlays
        .iter()
        .filter(|o| o.family == OverlayFamily::DynNotch)
        .find_map(|o| match o.shape {
            OverlayShape::Band { low_hz, high_hz } => Some((low_hz, high_hz)),
            _ => None,
        })
}

#[cfg(test)]
mod test {
    use super::*;

    /// Renaming a plot id silently throws away the persisted zoom of every
    /// pilot who had one.
    #[test]
    fn plot_ids_are_stable() {
        assert_eq!(plot_id(Axis::Roll), "psd_plot_roll");
        assert_eq!(plot_id(Axis::Yaw), "psd_plot_yaw");
    }

    /// The panel opens as a clean spectrum — nothing is drawn over the curve
    /// that the pilot did not ask for.
    #[test]
    fn overlays_start_hidden() {
        let panel = Psd::default();
        assert!(
            OverlayFamily::ALL
                .iter()
                .all(|&family| !panel.overlays.shows(family))
        );
        assert!(!panel.overlays.shows_peaks());
    }

    fn peak(freq_hz: f64, reach: Option<DynNotchReach>) -> FrequencyPeak {
        FrequencyPeak {
            freq_hz,
            amplitude_db: 40.0,
            harmonic_of: None,
            attenuated_db: Some(7.0),
            dyn_notch_reach: reach,
        }
    }

    fn spectral_with(peaks: Vec<FrequencyPeak>) -> AxisSpectral {
        let empty = || PsdData {
            freq_hz: Vec::new().into(),
            power_db: Vec::new(),
        };
        AxisSpectral {
            raw_psd: empty(),
            filtered_psd: None,
            raw_spectrum: crate::signal::fft::Spectrum {
                freq_hz: Vec::new().into(),
                magnitude: Vec::new(),
            },
            filtered_spectrum: None,
            throttle_map: None,
            time_map: None,
            peaks,
            noise_floor_db: 0.0,
        }
    }

    fn dyn_notch_overlay() -> Vec<FilterOverlay> {
        vec![FilterOverlay {
            label: "Dyn notch range".to_string(),
            family: OverlayFamily::DynNotch,
            shape: OverlayShape::Band {
                low_hz: 90.0,
                high_hz: 400.0,
            },
        }]
    }

    #[test]
    fn the_prose_names_the_bound_the_peaks_exceeded() {
        let spec = spectral_with(vec![
            peak(600.0, Some(DynNotchReach::AboveMax)),
            peak(700.0, Some(DynNotchReach::AboveMax)),
            peak(200.0, Some(DynNotchReach::Inside)),
        ]);

        let prose = out_of_reach_prose(&spec, &dyn_notch_overlay()).expect("two peaks are out");
        assert!(
            prose.starts_with("2 peaks sit above the dynamic notch's 400 Hz"),
            "{prose}"
        );
    }

    #[test]
    fn a_single_peak_below_the_minimum_reads_as_one() {
        let spec = spectral_with(vec![peak(50.0, Some(DynNotchReach::BelowMin))]);

        let prose = out_of_reach_prose(&spec, &dyn_notch_overlay()).expect("one peak is out");
        assert!(
            prose.starts_with("1 peak sits below the dynamic notch's 90 Hz"),
            "{prose}"
        );
    }

    /// Nothing out of reach, nothing said — and with no dynamic notch there is
    /// no claim to make at all.
    #[test]
    fn no_prose_without_something_to_report() {
        let inside = spectral_with(vec![peak(200.0, Some(DynNotchReach::Inside))]);
        assert_eq!(out_of_reach_prose(&inside, &dyn_notch_overlay()), None);

        let unconfigured = spectral_with(vec![peak(600.0, None)]);
        assert_eq!(out_of_reach_prose(&unconfigured, &[]), None);
    }

    /// The number a pilot judges the filter chain by, which the panel has
    /// never shown.
    #[test]
    fn a_labelled_peak_states_what_the_filters_took_off_it() {
        assert_eq!(peak_label(&peak(212.0, None)), "212 Hz · 7 dB filtered");
    }

    /// 100 Hz bins from 0 to 900.
    fn psd() -> PsdData {
        PsdData {
            freq_hz: (0..10).map(|i| i as f64 * 100.0).collect::<Vec<_>>().into(),
            power_db: (0..10).map(|i| -(i as f64)).collect(),
        }
    }

    fn band(low_hz: f64, high_hz: f64) -> HarmonicBand {
        HarmonicBand {
            motor: 0,
            order: 1,
            low_hz,
            high_hz,
            filtered: true,
        }
    }

    fn xs(points: PlotPoints<'_>) -> Vec<f64> {
        points.points().iter().map(|p| p.x).collect()
    }

    /// The mark is the spectrum's own points, so what it says about the noise
    /// at a frequency is what the curve underneath says.
    #[test]
    fn a_bands_mark_is_the_spectrums_own_points_across_it() {
        let run = spectrum_run(&psd(), &band(250.0, 450.0)).expect("the band is in range");

        // Widened a bin either side of the bins *inside* the band, so the mark
        // reaches past 250 and 450 rather than stopping short of them.
        assert_eq!(xs(run), vec![200.0, 300.0, 400.0, 500.0]);
    }

    /// A motor that held one frequency has a band narrower than the FFT's
    /// resolution. A single point draws nothing, so the run is still a segment.
    #[test]
    fn a_band_narrower_than_a_bin_still_draws_a_segment() {
        let run = spectrum_run(&psd(), &band(310.0, 320.0)).expect("the band is in range");
        assert!(xs(run).len() >= 2);
    }

    /// A third harmonic past Nyquist has no curve to recolour, and inventing
    /// one at the edge would put a mark where nothing was measured.
    #[test]
    fn a_band_off_the_end_of_the_spectrum_draws_nothing() {
        assert!(spectrum_run(&psd(), &band(2000.0, 3000.0)).is_none());
        assert!(spectrum_run(&psd(), &band(-200.0, -100.0)).is_none());
    }

    /// A band reaching past the last bin is cut to the spectrum rather than
    /// indexing off the end of it.
    #[test]
    fn a_band_overhanging_the_last_bin_is_cut_to_the_spectrum() {
        let run = spectrum_run(&psd(), &band(800.0, 5000.0)).expect("the band starts in range");
        assert_eq!(xs(run).last().copied(), Some(900.0));
    }

    /// A filter chain that leaves a peak louder than it found it says so,
    /// rather than reading as a cut of the same size.
    #[test]
    fn a_peak_the_filters_made_worse_is_not_shown_as_filtered() {
        let mut amplified = peak(212.0, None);
        amplified.attenuated_db = Some(-3.0);

        assert_eq!(peak_label(&amplified), "212 Hz · 3 dB louder filtered");
    }
}
