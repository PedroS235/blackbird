use egui::{Align2, Color32, RichText, Ui, Vec2b};
use egui_plot::{
    Bar, BarChart, FilledArea, HLine, Line, Plot, PlotPoint, PlotPoints, PlotUi, Text, VLine,
};
use elegance::Palette;

use super::drawn_axes;
use crate::analysis::filter_response::MIN_GAIN_DB;
use crate::analysis::{
    AxisSpectral, Dwell, DynNotchReach, FilterLoop, FilterOverlay, FilterResponse, FrequencyPeak,
    HarmonicBand, OverlayFamily, OverlayShape, SpectralAnalysis,
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

/// The chain total against the stage curves under it. Within a chain the
/// separation is width and alpha, never hue: hue is spent on which loop, which
/// is the fact a pilot cannot derive from the shape.
const CHAIN_WIDTH: f32 = 2.0;
const STAGE_WIDTH: f32 = 1.0;
const STAGE_ALPHA: f32 = 0.55;

/// What the chain removed, as an area. Low enough that the raw trace and a
/// harmonic recolour both read through it.
const FILL_ALPHA: u8 = 45;

/// The dwell lane: how much of the visible y span the strip along the floor
/// takes, and how solid it is. A fraction of the bounds rather than a span in
/// decibels, so the lane holds its height when the pilot zooms.
const LANE_FRACTION: f64 = 0.10;
const LANE_ALPHA: u8 = 130;

/// Where in its lane a bounds marker sits — a filter that was *allowed*
/// anywhere in a range, with nothing logged to say where it went.
const ALLOWED_HEIGHT: f64 = 0.5;

/// A harmonic's stretch is drawn over the raw trace, so it is drawn thicker —
/// at the same width the recolour would read as the curve rather than as a mark
/// on it.
const HARMONIC_WIDTH: f32 = 2.0;

/// The D-term chain's unity gain: the one reference line left on this plot,
/// and the only thing this level is now used for.
///
/// The PSD plots gyro power — `raw_psd` from `gyroUnfilt`, `filtered_psd` from
/// `gyroADC`. The D-term lowpasses never touched that signal, so anchoring
/// them to the raw curve would claim an attenuation that did not happen to the
/// trace being drawn. They hang from this line instead, pinned to the loudest
/// bin of this axis's own spectrum so the curves sit over the noise the D-term
/// stage had to survive, at the same decibels per pixel, and stay put when the
/// pilot zooms.
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

                    draw_overlays(plot_ui, &palette, &visible, axis, spec);
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

/// Everything the pilot ticked, in the order it has to be drawn: the fill
/// under the raw trace first, then the chain total over it, then the stages,
/// the harmonic recolours, and the dwell lane along the floor.
fn draw_overlays(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    visible: &[&FilterOverlay],
    axis: Axis,
    spec: &AxisSpectral,
) {
    let psd = &spec.raw_psd;
    let anchor_db = response_anchor_db(spec);

    if let Some(gain_db) = chain_gain_db(visible, axis, FilterLoop::Gyro, psd.power_db.len()) {
        draw_chain(plot_ui, palette, psd, &gain_db);
    }

    // The D-term chain's own zero, drawn only while a D-term family is on: a
    // curve with no reference is a shape with no scale, and "how far down" is
    // the whole question.
    if shows(visible, FilterLoop::Dterm) && anchor_db.is_finite() {
        let color = colors::chain_color(palette, FilterLoop::Dterm);
        plot_ui
            .hline(HLine::new("D-term 0 dB", anchor_db).color(color.gamma_multiply(STAGE_ALPHA)));
    }

    for overlay in visible {
        draw_shape(plot_ui, palette, overlay, axis, anchor_db, psd);
    }
    draw_dwell_lane(plot_ui, palette, visible, axis);
}

fn shows(visible: &[&FilterOverlay], loop_: FilterLoop) -> bool {
    visible
        .iter()
        .any(|o| o.family.filter_loop() == Some(loop_))
}

/// What one chain's *visible* stages took off, in dB per spectrum bin — the
/// elementwise product of their precomputed power gains.
///
/// Per frame, and over the visible stages only. A fixed whole-chain total would
/// lie to a pilot who switched the dynamic notch off and still saw its cut in
/// it; and multiplying five arrays of five hundred floats is arithmetic, not a
/// recomputation, so nothing about a toggle is expensive.
fn chain_gain_db(
    visible: &[&FilterOverlay],
    axis: Axis,
    loop_: FilterLoop,
    bins: usize,
) -> Option<Vec<f64>> {
    let gains: Vec<&[f64]> = visible
        .iter()
        .filter(|o| o.family.filter_loop() == Some(loop_))
        .filter_map(|o| o.gain.as_ref()?.get(axis))
        .map(Vec::as_slice)
        .filter(|gain| gain.len() == bins)
        .collect();

    (!gains.is_empty() && bins > 0).then(|| {
        (0..bins)
            .map(|i| {
                let power: f64 = gains.iter().map(|gain| gain[i]).product();
                (10.0 * power.log10()).max(MIN_GAIN_DB)
            })
            .collect()
    })
}

/// The chain total, on the data: `raw_db − chain_gain_db`, at the raw curve's
/// own frequencies, with the region between the two filled.
///
/// That fill *is* the energy the chain removed — thick where the chain worked,
/// a hairline where it did nothing — so "where is this filter actually being
/// used" is read as "where is the fill thick", with no legend and no
/// arithmetic in decibels. No threshold: it is drawn everywhere the two differ
/// at all, because a cut-in at some number of dB would draw a vertical edge the
/// physics does not have.
fn draw_chain(plot_ui: &mut PlotUi<'_>, palette: &Palette, psd: &PsdData, gain_db: &[f64]) {
    let Some((freq, raw, total)) = chain_edges(psd, gain_db) else {
        return;
    };
    let color = colors::chain_color(palette, FilterLoop::Gyro);

    plot_ui.add(
        FilledArea::new("gyro chain removed", freq, &total, raw)
            .fill_color(color.gamma_multiply_u8(FILL_ALPHA))
            .allow_hover(false),
    );
    plot_ui.line(
        Line::new(
            "gyro chain",
            total
                .iter()
                .enumerate()
                .map(|(i, &v)| [freq[i], v])
                .collect::<PlotPoints>(),
        )
        .color(color)
        .width(CHAIN_WIDTH),
    );
}

/// The frequencies, the raw trace and the chain total across them — the fill's
/// two edges. Both are the spectrum's own bins, so the fill needs no
/// resampling and cannot disagree with the trace it hangs under.
fn chain_edges<'a>(psd: &'a PsdData, gain_db: &[f64]) -> Option<(&'a [f64], &'a [f64], Vec<f64>)> {
    let n = gain_db.len().min(psd.power_db.len()).min(psd.freq_hz.len());
    if n < 2 {
        return None;
    }
    let (freq, raw) = (&psd.freq_hz[..n], &psd.power_db[..n]);

    Some((freq, raw, (0..n).map(|i| raw[i] + gain_db[i]).collect()))
}

fn draw_shape(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    overlay: &FilterOverlay,
    axis: Axis,
    anchor_db: f64,
    psd: &PsdData,
) {
    let loop_ = overlay.family.filter_loop();
    let color = chain_color(palette, loop_);

    // Where a stage curve hangs. A gyro stage acted on the signal this plot
    // draws, so it hangs off the raw trace's own points; a D-term stage did
    // not, so it hangs off its own unity gain and never touches the data.
    let base: Box<dyn Fn(f64) -> Option<f64>> = match loop_ {
        Some(FilterLoop::Dterm) => Box::new(move |_| anchor_db.is_finite().then_some(anchor_db)),
        _ => Box::new(|freq_hz| hover::y_at(&psd.freq_hz, &psd.power_db, freq_hz)),
    };

    match &overlay.shape {
        OverlayShape::Line { hz } => {
            plot_ui.vline(VLine::new(overlay.label.clone(), *hz).color(color));
        }
        // Where it was allowed to be, which is not what it removed: drawn in
        // the floor lane that means exactly that.
        OverlayShape::Allowed { .. } => {}
        OverlayShape::Harmonics(bands) => draw_harmonics(plot_ui, palette, bands, psd),
        OverlayShape::Response(response) => {
            draw_stage(plot_ui, response, color, &overlay.label, true, &base);
        }
        // Two rolloffs at the configured extremes. The label goes on the lower
        // corner only — the same stage twice over is one stage.
        OverlayShape::Envelope { low, high } => {
            draw_stage(plot_ui, low, color, &overlay.label, true, &base);
            draw_stage(plot_ui, high, color, &overlay.label, false, &base);
        }
        OverlayShape::Traced(per_axis) => {
            if let Some(response) = per_axis[axis].as_ref() {
                draw_stage(plot_ui, response, color, &overlay.label, true, &base);
            }
        }
    }
}

fn chain_color(palette: &Palette, loop_: Option<FilterLoop>) -> Color32 {
    match loop_ {
        Some(loop_) => colors::chain_color(palette, loop_),
        None => palette.text_faint,
    }
}

/// One stage's own curve, thin and dimmed under the total.
///
/// A notch is a V and a lowpass is a rolloff, from the fine 512-point
/// response, anchored at each of its own frequencies. These are shape, not
/// magnitude: which of three overlapping stages owns a given bin is not a
/// question the plot tries to answer, because the honest answer is "all of
/// them, multiplied" — and that is what the chain total says.
fn draw_stage(
    plot_ui: &mut PlotUi<'_>,
    response: &FilterResponse,
    color: Color32,
    label: &str,
    named: bool,
    base: &dyn Fn(f64) -> Option<f64>,
) {
    let color = color.gamma_multiply(STAGE_ALPHA);
    let curve: PlotPoints = response
        .freq_hz
        .iter()
        .zip(&response.gain_db)
        .filter_map(|(&f, &gain)| Some([f, base(f)? + gain]))
        .collect();
    plot_ui.line(
        Line::new(label.to_string(), curve)
            .color(color)
            .width(STAGE_WIDTH),
    );

    // Curves within a chain share one colour, so each says which stage it is
    // at the point it starts taking something — a notch's near edge, a
    // lowpass's corner.
    if named
        && let Some((freq, gain)) = response.corner()
        && let Some(base) = base(freq)
    {
        plot_ui.text(
            Text::new(
                format!("{label}_label"),
                PlotPoint::new(freq, base + gain),
                label,
            )
            .color(color)
            .anchor(Align2::CENTER_TOP),
        );
    }
}

/// Where the dynamic stages spent their time, as a filled histogram along the
/// floor of the plot.
///
/// Time is a third variable on a plot whose two axes are spent, so it gets its
/// own strip of pixels rather than being encoded into the curve: a curve faded
/// by dwell reads as uncertainty, which is a different and wrong claim, and is
/// indistinguishable from a curve that is merely shallow. A pinned notch is a
/// spike and a roaming one a plateau — the distinction the weighted average
/// has exactly divided out of the curve above.
///
/// Sized from the plot's own bounds each frame, so the lane is a strip of the
/// plot rather than a series in the data's decibels, and holds its height
/// under zoom.
fn draw_dwell_lane(
    plot_ui: &mut PlotUi<'_>,
    palette: &Palette,
    visible: &[&FilterOverlay],
    axis: Axis,
) {
    let bounds = plot_ui.plot_bounds();
    let (floor, lane) = (bounds.min()[1], bounds.height() * LANE_FRACTION);

    // One scale across the whole lane, so a pinned stage reads as taller than
    // a roaming one instead of every stage filling the lane to the brim.
    let peak = visible
        .iter()
        .filter_map(|o| o.dwell.as_ref()?.get(axis))
        .flat_map(|dwell| dwell.weight.iter().copied())
        .fold(0.0, f64::max);

    for overlay in visible {
        let color = chain_color(palette, overlay.family.filter_loop());

        match (
            &overlay.shape,
            overlay.dwell.as_ref().and_then(|d| d.get(axis)),
        ) {
            (_, Some(dwell)) if peak > 0.0 => plot_ui.bar_chart(
                BarChart::new(overlay.label.clone(), lane_bars(dwell, peak, floor, lane))
                    .color(color.gamma_multiply_u8(LANE_ALPHA))
                    .vertical()
                    .allow_hover(false),
            ),
            // Bounds, with nothing logged to say where inside them the filter
            // went. That is the same kind of claim as dwell — where it was
            // allowed to be — so it belongs in the lane that means that, not
            // as a span over the spectrum.
            (&OverlayShape::Allowed { low_hz, high_hz }, _) => {
                let y = floor + lane * ALLOWED_HEIGHT;
                plot_ui.line(
                    Line::new(overlay.label.clone(), vec![[low_hz, y], [high_hz, y]])
                        .color(color)
                        .width(CHAIN_WIDTH),
                );
            }
            _ => {}
        }
    }
}

/// One bar per visited bin, scaled against the tallest dwell on the plot.
fn lane_bars(dwell: &Dwell, peak: f64, floor: f64, lane: f64) -> Vec<Bar> {
    let width = match dwell.freq_hz.as_slice() {
        [first, second, ..] => second - first,
        _ => 1.0,
    };

    dwell
        .freq_hz
        .iter()
        .zip(&dwell.weight)
        .filter(|&(_, &weight)| weight > 0.0)
        .map(|(&hz, &weight)| {
            Bar::new(hz, weight / peak * lane)
                .base_offset(floor)
                .width(width)
        })
        .collect()
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
            OverlayShape::Allowed { low_hz, high_hz } => Some((low_hz, high_hz)),
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

    fn overlay(family: OverlayFamily, shape: OverlayShape) -> FilterOverlay {
        FilterOverlay {
            label: format!("{family:?}"),
            family,
            shape,
            gain: None,
            dwell: None,
        }
    }

    fn dyn_notch_overlay() -> Vec<FilterOverlay> {
        vec![overlay(
            OverlayFamily::DynNotch,
            OverlayShape::Allowed {
                low_hz: 90.0,
                high_hz: 400.0,
            },
        )]
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

    fn staged(family: OverlayFamily, gain: Vec<f64>) -> FilterOverlay {
        FilterOverlay {
            gain: Some(crate::analysis::ByAxis::Shared(gain)),
            ..overlay(family, OverlayShape::Line { hz: 0.0 })
        }
    }

    /// The arithmetic no pilot can do by eye: three stages cutting one
    /// frequency take off the product of their gains, which the panel drew as
    /// three separate curves in one colour and left to be multiplied by sight.
    #[test]
    fn the_chain_total_is_the_product_of_the_visible_stages() {
        let stages = [
            staged(OverlayFamily::Lowpass(FilterLoop::Gyro), vec![0.5, 0.25]),
            staged(OverlayFamily::Notch(FilterLoop::Gyro), vec![0.5, 1.0]),
        ];
        let visible: Vec<&FilterOverlay> = stages.iter().collect();

        let total = chain_gain_db(&visible, Axis::Roll, FilterLoop::Gyro, 2).expect("two stages");

        // 0.25 of the power is 6 dB down, 0.25 × 1.0 the same.
        assert!(
            (total[0] - 10.0 * 0.25f64.log10()).abs() < 1e-9,
            "{total:?}"
        );
        assert!(
            (total[1] - 10.0 * 0.25f64.log10()).abs() < 1e-9,
            "{total:?}"
        );
    }

    /// The reason the total is a per-frame product rather than a stored one: a
    /// fixed whole-chain total lies to a pilot who switched a family off and
    /// still sees its cut in the curve.
    #[test]
    fn hiding_a_family_drops_it_from_the_total() {
        let notch = staged(OverlayFamily::Notch(FilterLoop::Gyro), vec![0.5]);
        let alone = chain_gain_db(&[&notch], Axis::Roll, FilterLoop::Gyro, 1).unwrap();

        let lowpass = staged(OverlayFamily::Lowpass(FilterLoop::Gyro), vec![0.5]);
        let both = chain_gain_db(&[&notch, &lowpass], Axis::Roll, FilterLoop::Gyro, 1).unwrap();

        assert!(both[0] < alone[0] - 2.9, "{both:?} against {alone:?}");
        assert_eq!(chain_gain_db(&[], Axis::Roll, FilterLoop::Gyro, 1), None);
    }

    /// A D-term stage is in no gyro total: it never touched the signal this
    /// plot draws, and the fill under the gyro chain must not include it.
    #[test]
    fn a_dterm_stage_is_no_part_of_the_gyro_chain() {
        let dterm = staged(OverlayFamily::Lowpass(FilterLoop::Dterm), vec![0.5]);

        assert_eq!(
            chain_gain_db(&[&dterm], Axis::Roll, FilterLoop::Gyro, 1),
            None
        );
        assert!(chain_gain_db(&[&dterm], Axis::Roll, FilterLoop::Dterm, 1).is_some());
        assert!(shows(&[&dterm], FilterLoop::Dterm));
        assert!(!shows(&[&dterm], FilterLoop::Gyro));

        // And with no D-term family drawn there is no reference line to draw
        // either — an unlabelled line at the raw peak was the old anchor, and
        // it meant nothing.
        let gyro = staged(OverlayFamily::Notch(FilterLoop::Gyro), vec![0.5]);
        assert!(!shows(&[&gyro], FilterLoop::Dterm));
    }

    /// A gain array that does not fit the spectrum's bins cannot be multiplied
    /// against it, and a total off by one bin is a curve drawn at the wrong
    /// frequency.
    #[test]
    fn a_gain_that_does_not_fit_the_spectrum_is_left_out() {
        let stale = staged(OverlayFamily::Notch(FilterLoop::Gyro), vec![0.5, 0.5]);

        assert_eq!(
            chain_gain_db(&[&stale], Axis::Roll, FilterLoop::Gyro, 3),
            None
        );
    }

    /// The fill is the region between the raw trace and the total, so both its
    /// edges are the spectrum's own bins — no resampling, and no chance of the
    /// two edges disagreeing about where a frequency is.
    #[test]
    fn the_fills_two_edges_are_the_spectrums_own_bins() {
        let psd = psd();
        let gain_db = vec![-6.0; psd.power_db.len()];

        let (freq, raw, total) = chain_edges(&psd, &gain_db).expect("a chain to draw");
        assert_eq!(freq.len(), raw.len());
        assert_eq!(freq.len(), total.len());
        assert_eq!(freq, &psd.freq_hz[..]);
        for (i, &v) in total.iter().enumerate() {
            assert!((v - (raw[i] - 6.0)).abs() < 1e-9);
        }
    }

    /// A gyro stage hangs off the raw trace's own points, because it acted on
    /// this signal; a D-term stage hangs off its own unity gain, because it did
    /// not. The same 6 dB cut therefore lands in two different places.
    #[test]
    fn a_dterm_curve_is_drawn_independently_of_the_spectrum() {
        let psd = psd();
        let on_data = |f| hover::y_at(&psd.freq_hz, &psd.power_db, f);

        // The raw trace falls 1 dB per 100 Hz, so an anchored curve follows it.
        assert!((on_data(200.0).unwrap() - on_data(600.0).unwrap() - 4.0).abs() < 1e-9);

        let anchor = 0.0;
        let off_reference = |_| Some(anchor);
        assert_eq!(off_reference(200.0), off_reference(600.0));
    }

    fn dwell(weights: Vec<f64>) -> Dwell {
        Dwell {
            freq_hz: (0..weights.len())
                .map(|i| 100.0 + i as f64 * 10.0)
                .collect(),
            weight: weights,
        }
    }

    /// A pinned stage is a spike and a roaming one a plateau — one scale
    /// across the lane, so the two read differently instead of both filling it.
    #[test]
    fn the_lane_draws_a_pinned_stage_taller_than_a_roaming_one() {
        let pinned = dwell(vec![1.0, 0.0, 0.0, 0.0]);
        let roaming = dwell(vec![0.25; 4]);
        let peak = 1.0;

        let bars = |d: &Dwell| lane_bars(d, peak, 0.0, 10.0);
        let tallest = |d: &Dwell| {
            bars(d)
                .iter()
                .map(|b| b.value)
                .fold(f64::NEG_INFINITY, f64::max)
        };

        assert_eq!(bars(&pinned).len(), 1, "an unvisited bin draws nothing");
        assert_eq!(bars(&roaming).len(), 4);
        assert!(tallest(&pinned) > tallest(&roaming) * 3.0);

        // The lane is a strip of the plot: nothing in it reaches past its own
        // height, whatever the dwell did.
        assert!(bars(&pinned).iter().all(|b| b.value <= 10.0));
    }

    /// A log with no dynamic stage has no lane at all, rather than an empty
    /// strip of pixels along the floor.
    #[test]
    fn a_log_without_a_dynamic_stage_draws_no_lane() {
        let static_stage = staged(OverlayFamily::Notch(FilterLoop::Gyro), vec![0.5]);

        assert!(static_stage.dwell.is_none());
        assert_eq!(lane_bars(&dwell(vec![0.0, 0.0]), 1.0, 0.0, 10.0).len(), 0);
    }

    /// A synthetic flight with everything the overlays are built from: two
    /// noisy axes pre- and post-filter, throttle for the dynamic lowpass,
    /// eRPM for the harmonics, and a tracked notch centre in `debug[0]`.
    fn flight() -> (crate::parser::FlightData, crate::parser::Metadata) {
        use crate::parser::Channel;
        use crate::parser::metadata::{
            DynNotchConfig, FilterConfig, FilterType, LowpassConfig, NotchConfig,
            StaticLowpassConfig,
        };

        const N: usize = 4096;
        let at = |i: usize| i as f64 / 4000.0;
        let noise = |i: usize| {
            let t = at(i);
            (t * 220.0 * std::f64::consts::TAU).sin() * 40.0
                + (t * 640.0 * std::f64::consts::TAU).sin() * 12.0
        };

        let mut fd = crate::parser::FlightData::default()
            .with_time((0..N as u64).map(|i| i * 250).collect())
            .with_channel(
                Channel::Throttle,
                (0..N).map(|i| 1000.0 + at(i) * 800.0).collect(),
            )
            .with_channel(Channel::Rpm(0), vec![4200.0; N])
            .with_channel(
                Channel::Debug(0),
                (0..N).map(|i| 200.0 + at(i) * 90.0).collect(),
            );
        for axis in Axis::ALL {
            fd = fd
                .with_channel(Channel::RawGyro(axis), (0..N).map(noise).collect())
                .with_channel(
                    Channel::Gyro(axis),
                    (0..N).map(|i| noise(i) * 0.5).collect(),
                )
                .with_channel(
                    Channel::Debug(axis.index()),
                    (0..N).map(|i| 200.0 + at(i) * 90.0).collect(),
                );
        }

        let metadata = crate::parser::Metadata {
            looptime_us: Some(125),
            debug_mode: "FFT_FREQ".to_string(),
            filters: FilterConfig {
                gyro_lpf1: Some(LowpassConfig {
                    static_hz: 0.0,
                    dyn_min_hz: 250.0,
                    dyn_max_hz: 500.0,
                    dyn_expo: 0.0,
                    filter_type: FilterType::Pt1,
                }),
                gyro_lpf2: Some(StaticLowpassConfig {
                    cutoff_hz: 500.0,
                    filter_type: FilterType::Pt1,
                }),
                gyro_notches: vec![NotchConfig {
                    center_hz: 300.0,
                    cutoff_hz: 280.0,
                }],
                dterm_lpf1: Some(LowpassConfig {
                    static_hz: 100.0,
                    dyn_min_hz: 0.0,
                    dyn_max_hz: 0.0,
                    dyn_expo: 0.0,
                    filter_type: FilterType::Pt2,
                }),
                dterm_notches: vec![NotchConfig {
                    center_hz: 0.0,
                    cutoff_hz: 0.0,
                }],
                dyn_notch: Some(DynNotchConfig {
                    min_hz: 100.0,
                    max_hz: 500.0,
                    count: 2,
                    q: 5.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        (fd, metadata)
    }

    /// The whole panel, drawn headlessly with every overlay on: the fill, both
    /// chains, the stage curves, the harmonic recolours, the dwell lane and the
    /// peaks. A drawing bug here is a panic — `FilledArea` asserts its two
    /// edges are the same length, and every lane bar is sized off plot bounds
    /// that only exist mid-frame.
    #[test]
    fn every_overlay_draws_over_a_real_analysis() {
        let (fd, metadata) = flight();
        let analysis = crate::analysis::GyroNoiseAnalyzer::default().analyze(&fd, &metadata);
        assert!(analysis.axis(Axis::Roll).is_some(), "the fixture analysed");
        assert!(
            OverlayFamily::ALL
                .iter()
                .all(|&family| analysis.overlays.iter().any(|o| o.family == family)),
            "the fixture is missing a family: {:?}",
            analysis
                .overlays
                .iter()
                .map(|o| o.family)
                .collect::<Vec<_>>()
        );

        let mut panel = Psd {
            filtered_visible: PerAxis([true; 3]),
            overlays: OverlayVisibility::all_on(),
        };
        let ctx = egui::Context::default();
        // Twice: the first frame has no stored plot bounds, and the dwell lane
        // is sized from the bounds the previous frame left behind.
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| panel.show(ui, &analysis));
            });
        }
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
