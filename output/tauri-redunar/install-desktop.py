"""Register this local Tauri build with the production desktop identity."""
import hashlib
import os
import shutil
import subprocess
from pathlib import Path

APP_ID = "com.redunar.Redunar"
MARKER = "X-Redunar-Local-Tauri=true"
root = Path(__file__).resolve().parent
build_binary = root / "src-tauri/target/release/redunar-tauri"
binary = Path("/usr/bin/redunar-tauri")
icon = root / "src-tauri/icons/icon.png"
release_root = build_binary.parent
if not build_binary.is_file() or not icon.is_file():
    raise SystemExit("Build the native Tauri app before registering its launcher.")
if not binary.is_file():
    raise SystemExit("Install the Tauri binary at /usr/bin/redunar-tauri before registering it.")
if hashlib.sha256(build_binary.read_bytes()).digest() != hashlib.sha256(binary.read_bytes()).digest():
    raise SystemExit("Install the latest Tauri build at /usr/bin/redunar-tauri before registering it.")

# Capture and shortcut support are adjacent, versioned sidecars. Refuse to
# register a launcher that would look healthy but silently lose those paths.
for name, installed in (
    ("libredunar_capture_vulkan.so", Path("/usr/bin/libredunar_capture_vulkan.so")),
    ("redunar-steam-launch", Path("/usr/bin/redunar-steam-launch")),
    ("redunar-hotkey-helper", Path("/usr/libexec/redunar-hotkey-helper")),
):
    built = release_root / name
    if not built.is_file() or not installed.is_file():
        raise SystemExit(f"Install the Tauri runtime sidecar before registering its launcher: {name}")
    if hashlib.sha256(built.read_bytes()).digest() != hashlib.sha256(installed.read_bytes()).digest():
        raise SystemExit(f"Install the latest Tauri runtime sidecar before registering its launcher: {name}")

data = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
entry = data / "applications" / f"{APP_ID}.desktop"
icon_dir = data / "icons" / "hicolor" / "256x256" / "apps"
installed_icon = icon_dir / f"{APP_ID}.png"
if entry.exists() and MARKER not in entry.read_text():
    raise SystemExit(f"Preserving an existing unmanaged launcher: {entry}")

# Desktop Entry Exec quoting is distinct from shell quoting; percent introduces
# field codes even inside quotes, so literal percent characters must be doubled.
executable = str(binary).replace("%", "%%")
for character in ("\\", '"', "`", "$"):
    executable = executable.replace(character, "\\" + character)
if any(character in str(root) for character in ("\n", "\r")):
    raise SystemExit("The checkout path cannot contain a newline.")
entry.parent.mkdir(parents=True, exist_ok=True)
icon_dir.mkdir(parents=True, exist_ok=True)
installed_icon.write_bytes(icon.read_bytes())
entry_contents = (
    "[Desktop Entry]\nType=Application\nName=Redunar\n"
    "Comment=App for instant replay and in-game metrics.\n"
    f'Exec="{executable}"\nIcon={APP_ID}\n'
    f"StartupWMClass={APP_ID}\nTerminal=false\nCategories=Game;\n"
    f"StartupNotify=true\n{MARKER}\n"
)
temporary_entry = entry.with_suffix(".desktop.stage")
temporary_entry.write_text(entry_contents)
os.replace(temporary_entry, entry)

# KDE and GTK both cache desktop/icon discovery. Refresh those caches after
# replacing a local build so the production app ID keeps resolving to the
# Redunar icon instead of a generic Wayland fallback.
for command in (
    ["gtk-update-icon-cache", "-f", "-t", str(data / "icons" / "hicolor")],
    ["kbuildsycoca6", "--noincremental"],
):
    if shutil.which(command[0]):
        subprocess.run(command, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print(f"Registered the local Tauri launcher: {entry}")
