# 02 — Draw the gyro chain total on the raw spectrum, with the fill

Status: done

- Per frame, per axis: product of the gain arrays (01) of the *visible* gyro
  stages, giving one chain total; drawn at `raw_db − chain_gain_db`.
- The region between the raw trace and that curve is filled, chain hue, low
  alpha. No threshold — everywhere the two differ, however little.
- Per-stage gyro curves stay, thin and dimmed, anchored the same way. The fine
  512-point `FilterResponse` is what they draw, so a notch is still a V;
  anchoring it at its own frequencies uses `ui::hover::y_at` against the raw
  PSD.
- `anchor_db`'s shared `hline` goes; only D-term keeps a reference line (03).
- Corner labels keep their job of naming the stage, now sitting on the data.

Tests: the total of two visible stages is their product; hiding a family drops
it from the total; the fill's two edges have identical x.
