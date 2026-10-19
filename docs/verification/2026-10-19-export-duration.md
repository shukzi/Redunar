# Sparse-frame export duration, October 19, 2026

Follow-up to the UI/Wayland task at
`b56ab1c3d71bf4e360dc6c382005d38d477fa098`, before PR #15 integration.
The owner reported the released app rejecting an export because the rendered
duration differed from the selected range. Choosing another range succeeded.

## Reproduction and cause

Read-only timing inspection of the identified source found adjacent video
frame gaps of approximately 2.88 and 0.93 seconds. Private exports of a
five-second range near those gaps reproduced the failure. The released command
produced 8.762 seconds; the current task's encoder produced 8.778 seconds in MP4
and 8.813 seconds in MKV. Nothing was saved into the recording inventory.

The shared cause was `-avoid_negative_ts make_zero`. Sparse frames can give
B-frame decoding a multi-second negative preroll. Shifting that decode timeline
also shifts presentation and audio forward, inflating the container duration.
[FFmpeg documents this shared timestamp shift](https://ffmpeg.org/ffmpeg-formats.html#Format-Options).
The duration guard correctly rejected the enlarged output.

Export now uses `-avoid_negative_ts disabled` for every encoding pass, retaining
the muxers' preroll handling and the selected presentation timeline. The same
private five-second range measured 5.032 seconds with the released command's
other settings, 5.028 seconds in the current MP4 path and 5.021 seconds in MKV.
This changes no selection boundaries, size budgets, frame-rate policy or
per-track audio offsets. Duration admission and final duration/audio/size checks
are unchanged. Cancellation, descriptor ownership and cleanup retain their owners.

## Regression and qualification

The generated regression failed before the fix: a four-second selection became
5.967 seconds. Afterward, all thirteen native export tests passed, including
twelve sparse-media combinations: MP4/MKV input, MP4/MKV output and normal trim,
two-pass limited trim or whole-clip compression. Two audio tracks include a
deliberate offset. Assertions cover duration, leading timestamps, the retained
two-second frame hold, relative audio timing, complete decoding, the size cap,
unchanged source bytes and private output/pass-file removal. Automatic frame-rate
selection is unchanged; this is not a new guarantee of one output packet per
source frame.

Exact source manifests, before/after logs, private timing reports and final
build/package qualification are retained under
`.redunar-build/reports/export-duration/`. Original recordings are excluded from
source, tests and uploads. The native build is prepared separately from the
retained owner-tested build and official installed release. No app restart,
installation, game launch or publication is part of these checks.
