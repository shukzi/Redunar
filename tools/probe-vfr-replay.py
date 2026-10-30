#!/usr/bin/env python3
"""Isolated VFR media proof; does not start Redunar or access game hardware.

Generate one continuous moving H.264 clip with one-second 156/120/136 FPS
segments, remux it to MKV, then check decoded frame counts and timestamps.
The output argument is the MP4 path; the MKV uses the same stem.
"""

import argparse
import json
import statistics
import subprocess
from pathlib import Path


SEGMENTS = ((156, 0), (120, 1), (136, 2))
EXPECTED_FRAMES = sum(rate for rate, _ in SEGMENTS)


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def frame_times(path):
    result = json.loads(
        run(
            "ffprobe", "-v", "error", "-select_streams", "v:0",
            "-show_entries", "frame=best_effort_timestamp_time",
            "-of", "json", str(path),
        )
    )
    return [float(frame["best_effort_timestamp_time"]) for frame in result["frames"]]


def inspect(path, tolerance):
    times = frame_times(path)
    if len(times) != EXPECTED_FRAMES:
        raise RuntimeError(f"{path}: expected {EXPECTED_FRAMES} frames, got {len(times)}")
    if any(later <= earlier for earlier, later in zip(times, times[1:])):
        raise RuntimeError(f"{path}: video timestamps are not strictly increasing")

    results = []
    for rate, start in SEGMENTS:
        section = [time for time in times if start - tolerance <= time < start + 1 - tolerance]
        if len(section) != rate:
            raise RuntimeError(f"{path}: {rate} FPS section has {len(section)} frames")
        intervals = [later - earlier for earlier, later in zip(section, section[1:])]
        expected = 1 / rate
        worst_error = max(abs(interval - expected) for interval in intervals)
        if worst_error > tolerance:
            raise RuntimeError(
                f"{path}: {rate} FPS section has {worst_error:.6f}s timing error"
            )
        results.append(
            f"{rate} frames; median interval {statistics.median(intervals) * 1000:.3f} ms; "
            f"worst error {worst_error * 1000:.3f} ms"
        )
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="MP4 output; MKV uses the same stem")
    args = parser.parse_args()
    mp4 = args.output.resolve()
    mkv = mp4.with_suffix(".mkv")
    prepared = mp4.with_name(f"{mp4.stem}-prepared.mp4")
    fixed_60 = mp4.with_name(f"{mp4.stem}-60fps.mp4")
    mp4.parent.mkdir(parents=True, exist_ok=True)

    inputs = []
    for rate, _ in SEGMENTS:
        inputs += [
            "-f", "lavfi", "-i",
            f"color=c=0x0b1019:s=640x360:rate={rate}:duration=1",
        ]
    run(
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *inputs,
        "-filter_complex",
        "[0:v]setpts=PTS-STARTPTS[v0];"
        "[1:v]setpts=PTS-STARTPTS[v1];"
        "[2:v]setpts=PTS-STARTPTS[v2];"
        "[v0][v1][v2]concat=n=3:v=1:a=0[base];"
        "[base]drawtext=text=REDUNAR:fontsize=56:fontcolor=white:"
        "x='(w-text_w)/2+((w-text_w)/2)*sin(2*PI*t/1.6)':"
        "y='(h-text_h)/2'[v]",
        "-map", "[v]", "-fps_mode:v", "passthrough",
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "25",
        "-pix_fmt", "yuv420p", "-bf", "0",
        "-video_track_timescale", "1000000", "-movflags", "+faststart",
        str(mp4),
    )
    run(
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-i", str(mp4), "-map", "0:v:0", "-c", "copy", str(mkv),
    )
    # Match the video-copy/audio-conversion preparation used by the Tauri player.
    run(
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-i", str(mkv), "-map", "0:v:0", "-map", "0:a?",
        "-c:v", "copy", "-af", "aresample=async=1:first_pts=0",
        "-c:a", "aac", "-b:a", "192k", "-map_metadata", "-1",
        "-movflags", "+faststart", "-f", "mp4", str(prepared),
    )
    run(
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-i", str(mp4), "-map", "0:v:0", "-vf", "fps=60",
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "25",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(fixed_60),
    )

    for path, tolerance in ((mp4, 0.000003), (mkv, 0.0015), (prepared, 0.0015)):
        results = inspect(path, tolerance)
        decoded = json.loads(
            run(
                "ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0",
                "-show_entries", "stream=nb_read_frames", "-of", "json", str(path),
            )
        )["streams"][0]["nb_read_frames"]
        if int(decoded) != EXPECTED_FRAMES:
            raise RuntimeError(f"{path}: decoded {decoded} instead of {EXPECTED_FRAMES} frames")
        print(f"{path}: {decoded} decoded frames")
        for (_, second), result in zip(SEGMENTS, results):
            print(f"  second {second}: {result}")

    baseline_count = len(frame_times(fixed_60))
    if baseline_count != 180:
        raise RuntimeError(f"{fixed_60}: expected 180 frames, got {baseline_count}")
    print(f"{fixed_60}: {baseline_count} frames at fixed 60 FPS for visual comparison")


if __name__ == "__main__":
    main()
