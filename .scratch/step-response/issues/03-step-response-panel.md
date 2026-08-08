Status: done
Type: task

# Step Response panel

Third of three for `../spec.md`. Blocked by 02.

## Files

- `src/app/tabs/pid_analysis/step_response.rs` — new
- `src/app/tabs/pid_analysis/mod.rs` — enable the subtab

## Problem

The subtab exists as a hardcoded `enabled: false` button and a "coming soon"
label.

## Solution

Three stacked plots, `stacked_plot_height(ui, 3)`, one per axis, x in ms.

- Individual responses in `GYRO_AXIS_COLORS[axis]` at ~40 alpha, the mean in
  the same colour at full opacity and thicker, drawn last so it sits on top.
- Checkbox "show individual responses", default on, as panel state — following
  `Psd`/`Frequency`, whose toggles are deliberately *not* shared.
- Readout: "mean of N responses", where N counts every surviving trace.
- At most 100 evenly-spaced traces are drawn. The cap is a rendering decision;
  the mean always comes from all of them.

## Empty states

Distinct messages, in FPV language:

- No `setpoint` field in the log — name the field and how to enable it. This is
  the same class of defect as `logs-without-gyrounfilt/01`; do not ship a second
  silently-disabled tab.
- Setpoint present but no window passed the mask — say the sticks never moved
  enough, naming the 20 deg/s threshold.

The subtab button is enabled unconditionally. Presence is checked per axis, so a
partly-logged craft still gets the axes it has.
