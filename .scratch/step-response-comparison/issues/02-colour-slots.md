# 02 — Colour slots, and the theme fix they share with the axes

Status: done

Spec: `.scratch/step-response-comparison/spec.md`

## What

Comparison needs up to four colours that a pilot can tell apart and name. It
must not inherit the defect already in `get_axis_color`, which returns
`elegance::Palette::charcoal()` regardless of theme under its own
`// TODO: retrive from current theme and not fixed pallete` — the axis colours
are wrong in light mode today, and five panels call it.

Colour is the *slot*, not the flight: the base is always slot 0, so switching
the sidepanel changes the curve under a colour without changing the colour.
Identity lives on the chip label, and slot 0 should read as visually dominant —
a ramp of four equals says nothing about which flight is the pilot's own.

## Scope

- A slot colour function: fixed hue per slot, lightness and saturation resolved
  from `Theme::current(ui.ctx()).palette`. Fixed hues so "log 3 is the teal one"
  survives a theme toggle mid-session; palette-derived luminance so every stop
  is legible on the background it is actually drawn on.
- Four slots. Slot 0 carries the most weight — the others are visibly
  secondary.
- `get_axis_color` reads the current theme in the same pass, and the TODO goes.
  Same defect, same call-site shape, and leaving one of them wrong makes the
  next contributor copy the wrong one.
- Both live together in `app/tabs/mod.rs` or a small colour module next to it —
  one place answers "what colour is this line".

## Tests

- Every slot colour differs from every other, in both themes.
- Slot colours and axis colours are distinguishable from the panel background in
  both themes — a minimum contrast assertion, not an exact-value one, so a
  palette tweak in `elegance` does not fail the suite spuriously.
- `get_axis_color` returns different values under the light and dark palettes.
  This fails on `main` today, which is the point.

## Done when

Four nameable comparison colours exist, and the axis colours are finally correct
in light mode.
