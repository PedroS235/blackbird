# 09 — Correct the stale type names in `CLAUDE.md`

Status: done

`HeaderData`, `SpectralResult` and `NotchFilter` do not exist. The real types
are `Metadata`, `AxisSpectral` and `NotchConfig`, and PIDs are not parsed at
all. Document the overlay geometry while there.
