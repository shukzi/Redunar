#!/usr/bin/env python3
"""Verify native display selection on private Xvfb and headless Wayland servers.

Build display_backend_probe first. No production app, real game, installed
configuration, keyboard device or visible desktop is used.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]


def inside(directory):
    environment = dict(os.environ, GTK_A11Y="none", GTK_USE_PORTAL="0", GIO_USE_VFS="local")
    environment.pop("GDK_BACKEND", None)
    environment.pop("WAYLAND_DISPLAY", None)
    compositor_log = (directory / "wayland-compositor.log").open("w")
    compositor = subprocess.Popen([
        "mutter", "--headless", "--wayland", "--no-x11", "--wayland-display=redunar-test-wayland",
        "--virtual-monitor=1280x800",
    ], env=environment, stdout=compositor_log, stderr=subprocess.STDOUT)
    results = []
    try:
        for _ in range(100):
            if (Path(environment["XDG_RUNTIME_DIR"]) / "redunar-test-wayland").is_socket():
                break
            if compositor.poll() is not None:
                raise RuntimeError("Headless Wayland server failed; inspect its fixture log")
            time.sleep(.1)
        else:
            raise RuntimeError("Headless Wayland server did not become ready")
        cases = [
            ("wayland-session", "wayland", "redunar-test-wayland", None, "GdkWaylandDisplay"),
            ("launcher-without-session-type", None, "redunar-test-wayland", None, "GdkWaylandDisplay"),
            ("unreachable-wayland-falls-back", "wayland", str(Path(environment["XDG_RUNTIME_DIR"]) / "absent-wayland"), None, "GdkX11Display"),
            ("wayland-session-without-socket-name", "wayland", None, None, "GdkX11Display"),
            ("x11-session", "x11", None, None, "GdkX11Display"),
            ("x11-session-with-nested-wayland", "x11", "redunar-test-wayland", None, "GdkX11Display"),
            ("explicit-x11-on-wayland", "wayland", "redunar-test-wayland", "x11", "GdkX11Display"),
            ("explicit-wayland-on-x11", "x11", "redunar-test-wayland", "wayland", "GdkWaylandDisplay"),
            ("explicit-reverse-order", "wayland", "redunar-test-wayland", "x11,wayland", "GdkX11Display"),
        ]
        binary = ROOT / ".redunar-build/checks/tauri/debug/examples/display_backend_probe"
        for name, session, wayland, override, expected in cases:
            env = dict(environment, REDUNAR_PRIVATE_DESKTOP_FIXTURE="1", REDUNAR_EXPECT_DISPLAY_TYPE=expected)
            for key, value in [("XDG_SESSION_TYPE", session), ("WAYLAND_DISPLAY", wayland), ("GDK_BACKEND", override)]:
                env.pop(key, None)
                if value is not None:
                    env[key] = value
            result = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=10)
            (directory / f"{name}.log").write_text(result.stdout + result.stderr)
            assert result.returncode == 0, f"{name} failed; inspect its fixture log"
            results.append({"case": name, "display_type": expected, "passed": True})
            print(f"PASS: {name}: {expected}", flush=True)
        env = dict(environment, REDUNAR_PRIVATE_DESKTOP_FIXTURE="1", XDG_SESSION_TYPE="wayland",
            WAYLAND_DISPLAY="redunar-test-wayland", REDUNAR_EXPECT_DISPLAY_TYPE="GdkWaylandDisplay")
        for name, display, expected in [
            ("steam-wayland", "redunar-test-wayland", "GdkWaylandDisplay"),
            ("steam-x11-fallback", str(Path(environment["XDG_RUNTIME_DIR"]) / "absent-wayland"), "GdkX11Display"),
        ]:
            with (directory / f"{name}.log").open("w") as log:
                result = subprocess.run([str(binary.with_name("steam_background_probe")), "--tray-off-host", "--display-startup"],
                    env=dict(env, WAYLAND_DISPLAY=display, REDUNAR_EXPECT_DISPLAY_TYPE=expected),
                    stdout=log, stderr=subprocess.STDOUT, timeout=20)
            assert result.returncode == 0, f"{name} failed; inspect fixture log"
            results.append({"case": name, "display_type": expected, "passed": True})
            print(f"PASS: {name}: {expected}", flush=True)
        foreground = directory / "tauri-foreground.json"
        with (directory / "tauri-foreground.log").open("w") as log:
            result = subprocess.run(["python3", str(ROOT / "tools/check-native-rendering.py"),
                str(binary.with_name("rendering_probe")), "--output", str(foreground)],
                env=env, stdout=log, stderr=subprocess.STDOUT, timeout=75)
        assert result.returncode == 0, "Native Tauri foreground startup failed; inspect fixture log"
        assert all(phase["display_backend"] == "GdkWaylandDisplay" for phase in json.loads(foreground.read_text())["phases"])
        results.append({"case": "tauri-foreground", "display_type": "GdkWaylandDisplay", "passed": True})
        print("PASS: Tauri foreground startup uses native Wayland", flush=True)
        (directory / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    finally:
        if compositor.poll() is None:
            compositor.terminate()
            compositor.wait(timeout=10)
        compositor_log.close()


def main():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", type=Path)
    args = parser.parse_args()
    if args.inside:
        assert os.environ.get("REDUNAR_PRIVATE_DESKTOP_FIXTURE") == "1"
        inside(args.inside)
        return
    reports = ROOT / ".redunar-build/reports/wayland-startup"
    reports.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="private-display-", dir=reports))
    for name in ["config", "cache", "data", "state"]:
        (directory / name).mkdir(mode=0o700)
    config = directory / "session-bus.conf"
    config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>'
        '<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/>'
        '<allow own="*"/></policy></busconfig>')
    # Wayland uses AF_UNIX: keep runtime socket paths below its 108-byte limit,
    # independently of the checkout/report path. Only this owned fixture is removed.
    with tempfile.TemporaryDirectory(prefix="rdr-display-") as runtime:
        env = dict(os.environ, REDUNAR_PRIVATE_DESKTOP_FIXTURE="1", XDG_RUNTIME_DIR=runtime,
            XDG_CONFIG_HOME=str(directory / "config"), XDG_DATA_HOME=str(directory / "data"),
            XDG_STATE_HOME=str(directory / "state"), XDG_CACHE_HOME=str(directory / "cache"))
        subprocess.run(["dbus-run-session", "--config-file", str(config), "--", "xvfb-run", "-a",
            "python3", str(Path(__file__).resolve()), "--inside", str(directory)], env=env, check=True, timeout=120)
    print(f"Reports: {directory}")


if __name__ == "__main__":
    main()
