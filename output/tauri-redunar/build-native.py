"""Build this local Tauri app with matching Redunar capture components."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import fcntl

native = Path(__file__).resolve().parent
workspace = native.parent.parent
build_root = Path(os.environ.get("REDUNAR_NATIVE_BUILD_ROOT", workspace / ".redunar-build/native"))
if not build_root.is_absolute():
    raise ValueError("REDUNAR_NATIVE_BUILD_ROOT must be absolute")
build_root.mkdir(parents=True, exist_ok=True)
build_lock = (build_root / "build.lock").open("w")
fcntl.flock(build_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
runtime_target = build_root / "capture"
tauri_target = build_root / "tauri"

def run(arguments, directory, environment=None):
    subprocess.run(arguments, cwd=directory, check=True, env=environment)

run(["npm", "run", "build"], native)
run(["cargo", "build", "--release", "--locked", "--offline", "-p", "redunar-capture-vulkan", "--lib", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--locked", "--offline", "-p", "redunar-capture-opengl", "--lib", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--locked", "--offline", "-p", "redunar-platform", "--bin", "redunar-steam-launch", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--locked", "--offline", "-p", "redunar-hotkeys", "--bin", "redunar-hotkey-helper", "--target-dir", str(runtime_target)], workspace)
release_environment = os.environ.copy()
release_environment.setdefault(
    "REDUNAR_UPDATE_SOURCE_URL",
    "https://github.com/shukzi/Redunar/releases/latest/download",
)
run(
    ["cargo", "build", "--release", "--locked", "--offline", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", str(tauri_target)],
    native,
    release_environment,
)

destination = tauri_target / "release"
hashes = {}
for name in ("libredunar_capture_vulkan.so", "libredunar_capture_opengl.so", "redunar-steam-launch", "redunar-hotkey-helper"):
    source = runtime_target / "release" / name
    temporary = destination / f".{name}.stage"
    # Atomic replacement preserves the old inode if a running game still has
    # the previous library mapped. No global layer or Steam setting is changed.
    shutil.copy2(source, temporary)
    os.replace(temporary, destination / name)
    hashes[name] = hashlib.sha256(source.read_bytes()).hexdigest()
(destination / "capture-components.json").write_text(json.dumps(hashes, indent=2) + "\n")
print("Built Tauri with its capture libraries, Steam wrapper, shortcut helper, and update helper.")
