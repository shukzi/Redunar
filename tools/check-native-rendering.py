#!/usr/bin/env python3
"""Compare real Tauri renderers using generated clips and private command data.

Build the rendering_probe example first. Use a private D-Bus/Xvfb session for
ordinary checks; --desktop explicitly permits a short visible comparison.
"""
import argparse
import ast
import base64
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--desktop", action="store_true")
    parser.add_argument("--snapshot", action="store_true", help="Capture only the private X display")
    args = parser.parse_args()
    if not args.desktop and os.environ.get("REDUNAR_PRIVATE_DESKTOP_FIXTURE") != "1":
        parser.error("Use private D-Bus/Xvfb with REDUNAR_PRIVATE_DESKTOP_FIXTURE=1, or --desktop")
    if args.snapshot and args.desktop:
        parser.error("Snapshots are restricted to the private X fixture")
    root = Path(__file__).resolve().parents[1]
    native = root / "output/tauri-redunar"
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix=output.stem + "-", dir=output.parent))
    (directory / "fixture-v1").write_bytes(b"redunar-render-fixture-v1\n")
    media = directory / "clip.mp4"
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
        "testsrc2=s=640x360:r=30", "-t", "5", "-c:v", "libx264", "-preset", "ultrafast",
        "-threads", "1", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(media)], check=True)
    thumb = directory / "thumb.jpg"
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-i", str(media),
        "-frames:v", "1", "-threads", "1", str(thumb)], check=True)

    class Handler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            if self.path.split("?")[0] != "/clip.mp4":
                self.send_error(404)
                return
            body = media.read_bytes()
            start, end = 0, len(body) - 1
            requested = self.headers.get("Range")
            if requested:
                first, last = requested.removeprefix("bytes=").split("-", 1)
                start, end = int(first or 0), min(int(last) if last else end, end)
            self.send_response(206 if requested else 200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            if requested:
                self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
            self.end_headers()
            try:
                self.wfile.write(body[start:end + 1])
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    tree = ast.parse((native / "tests/webview_fixture.py").read_text())
    fixture = next(ast.literal_eval(node.value) for node in tree.body if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "fixture" for target in node.targets))
    # Evaluate the existing data fixture in Node, then serve its plain results
    # through the native fixture handler. Tauri's real IPC remains untouched.
    jpeg = base64.b64encode(thumb.read_bytes()).decode()
    generator = """
global.window=global;global.location={search:''};global.addEventListener=()=>{};
global.document={createElement:()=>({getContext:()=>({createLinearGradient:()=>({addColorStop(){}}),fillRect(){},fillText(){}}),toDataURL:()=> 'data:image/jpeg;base64,'+JPEG})};
const fixtureResolutionRejection='Fixture display unavailable';window.fixturePalettes=PALETTES;
""".replace("JPEG", json.dumps(jpeg)).replace("PALETTES", (native / "ui/overlay-palettes.json").read_text())
    generator += fixture + "\n" + """
bridge.idle=true;bridge.replayInactive=true;bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
bridge.clips=Array.from({length:40},(_,i)=>({file_name:`fixture-${i}.mp4`,title:`Generated clip ${i+1}`,bytes:800000,duration_seconds:5,modified_unix_ns:String(i+1)}));
const commands=['daemon_status','session_status','module_statuses','catalog_games','replay_clips','replay_storage_status','app_preferences','shortcut_status','replay_preferences','session_history','microphone_inputs','replay_display_capability','diagnostics_snapshot','global_settings','check_for_updates','game_installation_statuses','game_poster','game_banner'];
(async()=>{const responses={};for(const command of commands)responses[command]=await window.__TAURI_INTERNALS__.invoke(command,{});
responses.clip_playback_path=MEDIA;responses.clip_metadata={duration_seconds:5,width:640,height:360,fps:30};
responses.clip_thumbnail=Array.from(atob(JPEG),c=>c.charCodeAt(0));process.stdout.write(JSON.stringify(responses));})();
""".replace("MEDIA", json.dumps(f"http://127.0.0.1:{server.server_port}/clip.mp4")).replace("JPEG", json.dumps(jpeg))
    responses = subprocess.check_output(["node", "--input-type=commonjs", "-e", generator], text=True)
    (directory / "responses.json").write_text(responses)
    initialization = "const nativeRenderInvoke=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);window.bridge={errors:[]};\n"
    initialization += "window.addEventListener('error',e=>bridge.errors.push(e.message));window.addEventListener('unhandledrejection',e=>bridge.errors.push(String(e.reason)));\n"
    initialization += (native / "tests/rendering-probe.js").read_text()
    (directory / "init.js").write_text(initialization)
    env = dict(os.environ, REDUNAR_RENDER_FIXTURE="1", REDUNAR_RENDER_FIXTURE_DIR=str(directory))
    log = (directory / "native.log").open("w")
    process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        if args.snapshot:
            time.sleep(5)
            subprocess.run(["import", "-window", "root", str(directory / "native.png")], check=True)
        if args.desktop and shutil.which("niri"):
            for _ in range(100):
                if process.poll() is not None:
                    break
                windows = json.loads(subprocess.check_output(["niri", "msg", "--json", "windows"]))
                window = next((window for window in windows if window["title"] ==
                    "Redunar rendering comparison — generated fixture"), None)
                if window:
                    subprocess.run(["niri", "msg", "action", "focus-window", "--id", str(window["id"])],
                        stdout=subprocess.DEVNULL, check=True)
                    break
                time.sleep(.1)
        code = process.wait(timeout=65)
        result = json.loads((directory / "result.json").read_text())
        result.update(exit_code=code, backend=env.get("GDK_BACKEND"),
            dmabuf_override=env.get("WEBKIT_DISABLE_DMABUF_RENDERER", "1"), artifacts=str(directory))
        output.write_text(json.dumps(result, indent=2) + "\n")
        assert code == 0 and not result.get("errors"), result
        for phase in result["phases"]:
            print(json.dumps({key: phase[key] for key in ["label", "focused", "raf_per_second", "paint_ms"]}))
    finally:
        if process.poll() is None:
            process.terminate()  # Only the fixture child owned by this runner.
            process.wait(timeout=5)
        log.close()
        server.shutdown()


if __name__ == "__main__":
    main()
