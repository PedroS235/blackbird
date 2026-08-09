# Spec: trim the ends of a log before analysing it

Status: done

## Problem

The first and last couple of seconds of a blackbox log are not flight. They are
arming, the craft sat on the ground with the props spinning, the hand launch,
and at the other end the landing or the crash. They carry motor noise the
spectral analysis reports as if it were in-flight noise, and stick movement the
deconvolution turns into responses of a craft that is not airborne.

## Change

Analysis runs over the log's middle, not the whole log:

- `FlightData::trimmed(trim_s)` returns a **view** — a sample span, no copy, a
  five-minute log at 8 kHz being tens of megabytes per channel.
- `GyroNoiseAnalyzer` and `StepResponseAnalyzer` each gain a `trim_s` knob,
  default 2.0 s, and analyse `fd.trimmed(self.trim_s)`.
- Trimming applies only when it leaves at least half the log. Otherwise the log
  is too short for its ends to be a distinguishable part of it and it is
  analysed whole — a 4 s bench test must not become a 0 s one.
- Time values stay relative to the untrimmed start, so the spectrogram's x axis
  still lines up with the timeseries plots.
- The step response panel exposes the knob; the timeseries plots are untouched,
  the pilot still sees every sample they logged.

## Out of scope

Per-log trim overrides, or detecting take-off/landing from the data.
