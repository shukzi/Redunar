"""Build this local Tauri app with matching Redunar capture components."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import re

native = Path(__file__).resolve().parent
workspace = native.parent.parent
runtime_target = native / "capture-runtime-build"

def run(arguments, directory, environment=None):
    subprocess.run(arguments, cwd=directory, check=True, env=environment)

spec = workspace / "packaging/redunar-app.spec"
spec_text = spec.read_text()
version_match = re.search(r"^Version:\s+([^%\s]+)", spec_text, re.MULTILINE)
if version_match is None:
    raise RuntimeError(f"Could not determine Redunar product version from {spec}")
version_label = version_match.group(1)
index = native / "ui/index.html"
index_source = index.read_text()
rendered_index = index_source.replace("__REDUNAR_VERSION__", version_label)
if rendered_index == index_source:
    raise RuntimeError("ui/index.html is missing the __REDUNAR_VERSION__ placeholder")
index.write_text(rendered_index)
try:
    run(["npm", "run", "build"], native)
finally:
    index.write_text(index_source)
run(["cargo", "build", "--release", "--offline", "-p", "redunar-capture-vulkan", "--lib", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--offline", "-p", "redunar-capture-opengl", "--lib", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--offline", "-p", "redunar-platform", "--bin", "redunar-steam-launch", "--target-dir", str(runtime_target)], workspace)
run(["cargo", "build", "--release", "--offline", "-p", "redunar-hotkeys", "--bin", "redunar-hotkey-helper", "--target-dir", str(runtime_target)], workspace)
release_environment = os.environ.copy()
release_environment.setdefault(
    "REDUNAR_UPDATE_SOURCE_URL",
    "https://github.com/shukzi/Redunar/releases/latest/download",
)
run(
    ["cargo", "build", "--release", "--offline", "--manifest-path", "src-tauri/Cargo.toml"],
    native,
    release_environment,
)

destination = native / "src-tauri/target/release"
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
print("Built Tauri with its local capture libraries, Steam wrapper, and shortcut helper.")
