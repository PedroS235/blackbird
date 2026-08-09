# 03 — Parse and show the craft's rates

Status: done

Spec: `.scratch/step-response-metrics/spec.md`

Independent of 01 and 02; ordered last because it is the smallest. It is the
prerequisite for deriving the stick-input presets from the craft's own rates
later.

## What

The presets ask a pilot to choose between 25, 70 and 120 deg/s with nothing on
screen saying what those mean on their quad — a pilot on 670 deg/s rates and one
on 1200 are not describing the same manoeuvre when they pick "Freestyle". Every
Betaflight log header records the answer and none of it is parsed: the fixture
carries `rates_type:3`, `rc_rates:7,7,7`, `rates:67,67,67`, `rc_expo:0,0,0`.

## Scope

- `RateConfig` holding the rate type, per-axis RC rates, per-axis rates and
  per-axis expo, built from the raw headers by a `parse_rate_config` step
  alongside the existing filter-config parsing.
- `RateType` decoded from the Betaflight code, mirroring the existing filter-type
  decode, with an `Unknown(code)` variant that renders as the raw code rather
  than guessing at a conversion.
- Raw values and the type name only. No centre-sensitivity or maximum-rate maths
  — that needs a different formula per rate type and none are verified yet.
- Two readers: a rates line on the log card, beside the craft name, firmware and
  loop rate it already shows; and a short echo beside the stick-input presets,
  where the choice is actually made.
- Header string parsing stays in the parser. Neither panel touches raw headers.

## Tests

Integration, over the load pipeline, with the existing metadata parser test as
prior art: a fixture's headers decode to their recorded rate type and values.

## Done when

The log card and the preset row both read `Actual 67/67/67` for the fixture, an
unrecognised rate type renders as its code, and no panel touches a raw header
string.
