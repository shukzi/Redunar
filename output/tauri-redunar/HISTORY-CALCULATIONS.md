# Session History calculation contract

Reviewed against `ui/history-timeline.mjs` on September 13, 2026. This file owns
current graph/inspector behavior.

## Recorded data

Plot every retained journal observation at its original elapsed time. Do not
median-smooth values, move timestamps into buckets, invent start/end readings,
or fill missing metrics with zero. Invalid/missing values break their series;
an isolated valid observation remains visible.

New journal samples contain a capture-window average FPS and latest captured
frame interval. They are not necessarily reciprocals. Session summaries come
from native data rather than being recomputed from the rendered line. The bounded
journal can reduce retention resolution on long sessions; the UI cannot recover
unrecorded or discarded samples.

Legacy frame-only records reconstruct the retained tail ending at session end:
FPS = 1000 / frame interval in milliseconds. Use the retained duration when a
subsecond record has a zero-second summary. There is no arbitrary 1000 FPS cap,
and no invented temperature or load for legacy records.

## Axis calculation

The displayed metric's valid observations determine one shared scale; CPU and
GPU use the combined extrema. Finite temperatures are valid, utilization must
be in 0–100%, frame times must be positive, and FPS may be zero.

- FPS and frame time use zero as the lower bound and rounded ticks covering the
  raw maximum. FPS tick spacing is at least one FPS.
- Temperature pads the recorded minimum and maximum by 5°C before rounding.
- Load starts at zero and rounds up from the recorded maximum, capped at 100%.
  It is **not** always the fixed 0, 25, 50, 75, 100 scale. Near-zero data still
  receives a positive range.
- Tick spacing uses the first suitable 1, 2, 2.5, 5, or 10 multiple of the
  magnitude derived from roughly four intervals. Labels use precision matching
  that spacing and explicit FPS, ms, °C, or % units.
- Labels, grid, traces, and cursor values share `historyScale`/`historyY`.
  The SVG mapping is `y = 210 - (value - floor) / (ceiling - floor) * 190`.
  An 80 reading must plot above a 75 gridline on that same scale.

The upper tick is a readable bound, not necessarily the exact recorded peak.
Session-specific scaling helps reveal variation but does not normalize load or
permit percentages above 100. Missing data must not manufacture a scale/readout.

## Time and selection

The chart, time labels, and slider share one horizontal plot extent, including
thumb geometry. Duration covers the session and any retained last timestamp.
Within recorded coverage, selection snaps to the nearest actual observation,
choosing the earlier one on a tie. The marker, thumb, time label, accessible
value, and inspector must all use that same timestamp. Outside coverage, retain
the requested time and show unavailable readings.

Arrow keys visit adjacent observations, Home/End reach session endpoints, and
PageUp/PageDown move through the duration. Fractional timestamps and valid zero
values remain representable. Map data once; pointer inspection must not rebuild
all traces or run expensive native queries.

## Presentation and verification

History CPU traces/legends use `#e7474f`; GPU uses `#ff9297`. Selected measurement
cards retain neutral text. This scope does not recolor unrelated CPU/GPU UI.

`tests/history-timeline.test.mjs` exercises the calculation helpers.
`tests/workspace-webview.py` checks built chart geometry, trace/axis consistency,
slider alignment, missing samples, and CPU/GPU temperature/load fixtures. Use
independent expected values and coordinates; asserting one helper against itself
cannot prove correctness. See [TESTING.md](../../TESTING.md) for commands.

Visual and numeric checks do not validate a real sensor's accuracy or fill gaps
in an old session. Overview has its own live graph behavior; changes here must
not silently alter it.
