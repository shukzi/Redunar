use evdev::{Device, EventType, KeyCode, RelativeAxisCode};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const SESSION_ARGUMENT: &str = "--monitor-v1";
const TRUSTED_PARENTS: &[&str] = &["/usr/bin/redunar-tauri"];
const MAX_BINDINGS: usize = 9;
const MAX_KEYBOARDS: usize = 16;
const MAX_MICE: usize = 8;
const MAX_DEVICE_LINKS_SCANNED: usize = 64;
const INPUT_EVENT_BYTES: usize = 24;
const EV_KEY: u16 = 1;
const KEY_ESC: u16 = 1;
const BTN_LEFT: u16 = 272;
const MOTION_LIMIT: i32 = 4_096;
const POINTER_BASE_GAIN_MILLI: i64 = 8_500;
const POINTER_SPEED_GAIN_MILLI: i64 = 300;
const POINTER_SPEED_LIMIT: i64 = 32;
const POINTER_VERTICAL_RATIO_MILLI: i64 = 1_568;
const EVENT_TICK: Duration = Duration::from_millis(8);
const MOTION_INTERVAL: Duration = Duration::from_millis(8);
const MENU_PING_INTERVAL: Duration = Duration::from_millis(500);
const MENU_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);
const CONTROL_TIMEOUT: Duration = Duration::from_millis(500);
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTALT: u16 = 56;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_RIGHTALT: u16 = 100;
const KEY_RIGHTMETA: u16 = 126;
const MOD_CONTROL: u8 = 1 << 0;
const MOD_SHIFT: u8 = 1 << 1;
const MOD_ALT: u8 = 1 << 2;
const MOD_META: u8 = 1 << 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeyChord {
    key: u16,
    modifiers: u8,
    action: HotkeyAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HotkeyAction {
    ToggleOverlay,
    Save(u16),
}

#[derive(Clone, Copy)]
struct KeyEvent {
    device: usize,
    code: u16,
    value: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PointerEvent {
    MotionX(i32),
    MotionY(i32),
    LeftButton(bool),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PendingMotion {
    x: i32,
    y: i32,
}

impl PendingMotion {
    fn add(&mut self, event: PointerEvent) {
        match event {
            PointerEvent::MotionX(value) => self.x = bounded_motion(self.x, value),
            PointerEvent::MotionY(value) => self.y = bounded_motion(self.y, value),
            PointerEvent::LeftButton(_) => {}
        }
    }

    fn take(&mut self) -> Option<(i32, i32)> {
        let motion = (self.x, self.y);
        *self = Self::default();
        (motion != (0, 0)).then_some(motion)
    }
}

struct MenuCapture {
    open: bool,
    control: Option<UnixStream>,
    pending_motion: PendingMotion,
    last_motion_sent: Instant,
    last_input: Instant,
    last_ping: Instant,
}

impl MenuCapture {
    fn closed(now: Instant) -> Self {
        Self {
            open: false,
            control: None,
            pending_motion: PendingMotion::default(),
            last_motion_sent: now,
            last_input: now,
            last_ping: now,
        }
    }

    #[allow(
        dead_code,
        reason = "menu mouse capture remains dormant in the restored helper path"
    )]
    fn opened(&mut self, control: UnixStream, now: Instant) {
        self.open = true;
        self.control = Some(control);
        self.pending_motion = PendingMotion::default();
        self.last_motion_sent = now;
        self.last_input = now;
        self.last_ping = now;
    }

    fn closed_by_release(&mut self) {
        self.open = false;
        self.control = None;
        self.pending_motion = PendingMotion::default();
    }

    fn request(&mut self, command: &[u8]) -> Result<String, String> {
        let stream = self
            .control
            .as_mut()
            .ok_or_else(|| "Replay menu control session is unavailable".to_owned())?;
        request_on_stream(stream, command)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Redunar hotkey helper refused to start: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(SESSION_ARGUMENT))
        || arguments.next().is_some()
    {
        return Err("an authenticated monitor argument is required".to_owned());
    }
    let uid = authenticated_parent_uid()?;
    let mut input = BufReader::new(io::stdin());
    let bindings = read_bindings(&mut input)?;
    let devices = keyboard_devices();
    if devices.is_empty() {
        return Err("no readable keyboard input device was found".to_owned());
    }
    let mut mice = open_mouse_devices(&mouse_devices());

    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::sync_channel(128);
    let mut workers = Vec::new();
    for (device, path) in devices.into_iter().enumerate() {
        let sender = sender.clone();
        let stop = Arc::clone(&stop);
        workers.push(thread::spawn(move || {
            read_keyboard(device, &path, &stop, &sender);
        }));
    }
    drop(sender);
    let input_stop = Arc::clone(&stop);
    let input_worker = thread::spawn(move || {
        let mut sink = [0_u8; 64];
        while input.read(&mut sink).is_ok_and(|read| read != 0) {}
        input_stop.store(true, Ordering::Release);
    });

    println!("READY {}", workers.len());
    io::stdout().flush().map_err(|error| error.to_string())?;
    let control_path = PathBuf::from(format!("/run/user/{uid}/redunar/replay-control-v1.sock"));
    monitor_events(&receiver, &stop, &bindings, &control_path, &mut mice)?;
    release_all(&mut mice);
    stop.store(true, Ordering::Release);
    drop(workers);
    let _ = input_worker.join();
    Ok(())
}

fn authenticated_parent_uid() -> Result<u32, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|_| "process credentials are unavailable".to_owned())?;
    let (effective_uid, parent_pid) = parse_process_identity(&status)?;
    if effective_uid == 0 {
        return Err("the shortcut helper must run as the logged-in user".to_owned());
    }
    let parent_status = fs::read_to_string(format!("/proc/{parent_pid}/status"))
        .map_err(|_| "parent process credentials are unavailable".to_owned())?;
    let (caller_uid, _) = parse_process_identity(&parent_status)?;
    if caller_uid != effective_uid {
        return Err("the shortcut helper and Redunar app must use the same user".to_owned());
    }
    let parent_executable = fs::read_link(format!("/proc/{parent_pid}/exe"))
        .map_err(|_| "parent executable is unavailable".to_owned())?;
    let trusted_parent = trusted_parent(&parent_executable);
    if !trusted_parent {
        return Err("the helper was not started by the installed Redunar app".to_owned());
    }
    Ok(effective_uid)
}

fn trusted_parent(parent_executable: &Path) -> bool {
    if TRUSTED_PARENTS
        .iter()
        .map(Path::new)
        .any(|path| parent_executable == path)
    {
        return true;
    }

    // Checkout-only release runs keep the helper beside the Tauri binary.
    // Require both the exact binary name and its adjacent helper so this
    // development exception cannot trust an arbitrary parent executable.
    parent_executable
        .file_name()
        .is_some_and(|name| name == "redunar-tauri")
        && parent_executable
            .parent()
            .is_some_and(|directory| directory.join("redunar-hotkey-helper").is_file())
}

fn parse_process_identity(status: &str) -> Result<(u32, u32), String> {
    let effective_uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|values| values.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "effective user identity is unavailable".to_owned())?;
    let parent_pid = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid != 0)
        .ok_or_else(|| "parent process identity is unavailable".to_owned())?;
    Ok((effective_uid, parent_pid))
}

fn read_bindings(input: &mut impl BufRead) -> Result<Vec<KeyChord>, String> {
    let mut bindings = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let read = input
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("shortcut configuration ended before START".to_owned());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "START" {
            break;
        }
        if bindings.len() >= MAX_BINDINGS {
            return Err("too many shortcut bindings".to_owned());
        }
        let (shortcut, action) = if let Some(shortcut) = line.strip_prefix("TOGGLE ") {
            (shortcut, HotkeyAction::ToggleOverlay)
        } else {
            let (shortcut, seconds) = line
                .strip_prefix("BIND ")
                .and_then(|value| value.rsplit_once(' '))
                .ok_or_else(|| "invalid shortcut binding".to_owned())?;
            let seconds = seconds
                .parse::<u16>()
                .ok()
                .filter(|seconds| matches!(seconds, 15 | 30 | 60 | 120 | 180 | 300 | 600 | 900))
                .ok_or_else(|| "unsupported Replay duration".to_owned())?;
            (shortcut, HotkeyAction::Save(seconds))
        };
        let mut chord = parse_chord(shortcut)?;
        chord.action = action;
        if bindings.iter().any(|existing: &KeyChord| {
            existing.key == chord.key && existing.modifiers == chord.modifiers
        }) {
            return Err("duplicate shortcut binding".to_owned());
        }
        bindings.push(chord);
    }
    if bindings.is_empty() {
        return Err("at least one shortcut binding is required".to_owned());
    }
    if bindings
        .iter()
        .filter(|binding| binding.action == HotkeyAction::ToggleOverlay)
        .count()
        > 1
    {
        return Err("at most one Replay overlay shortcut is allowed".to_owned());
    }
    Ok(bindings)
}

fn parse_chord(shortcut: &str) -> Result<KeyChord, String> {
    let normalized = shortcut.replace(['<', '>'], "+").replace(' ', "");
    let mut chord = KeyChord {
        key: 0,
        modifiers: 0,
        action: HotkeyAction::ToggleOverlay,
    };
    for part in normalized.split('+').filter(|part| !part.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => chord.modifiers |= MOD_CONTROL,
            "shift" => chord.modifiers |= MOD_SHIFT,
            "alt" => chord.modifiers |= MOD_ALT,
            "super" | "meta" | "win" => chord.modifiers |= MOD_META,
            key if chord.key == 0 => {
                chord.key = key_code(key).ok_or_else(|| "unsupported shortcut key".to_owned())?;
            }
            _ => return Err("shortcut contains more than one non-modifier key".to_owned()),
        }
    }
    if chord.key == 0 {
        return Err("shortcut has no key".to_owned());
    }
    Ok(chord)
}

fn key_code(key: &str) -> Option<u16> {
    let code = match key {
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "tab" => 15,
        "q" => 16,
        "w" => 17,
        "e" => 18,
        "r" => 19,
        "t" => 20,
        "y" => 21,
        "u" => 22,
        "i" => 23,
        "o" => 24,
        "p" => 25,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "z" => 44,
        "x" => 45,
        "c" => 46,
        "v" => 47,
        "b" => 48,
        "n" => 49,
        "m" => 50,
        "space" => 57,
        "enter" | "return" => 28,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        _ => return None,
    };
    Some(code)
}

fn keyboard_devices() -> Vec<PathBuf> {
    let mut devices = HashSet::new();
    for directory in ["/dev/input/by-id", "/dev/input/by-path"] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(MAX_DEVICE_LINKS_SCANNED) {
            let name = entry.file_name();
            if !name.to_string_lossy().ends_with("-event-kbd") {
                continue;
            }
            if let Ok(path) = fs::canonicalize(entry.path())
                && path.starts_with("/dev/input")
                // The directory entries can exist even when the logged-in
                // user has no ACL/group access to the evdev node. Filter
                // those nodes before announcing READY so the UI cannot show
                // an active monitor that will never receive key events.
                && File::open(&path).is_ok()
            {
                devices.insert(path);
            }
            if devices.len() >= MAX_KEYBOARDS {
                break;
            }
        }
    }
    let mut devices = devices.into_iter().collect::<Vec<_>>();
    devices.sort();
    devices
}

fn mouse_devices() -> Vec<PathBuf> {
    input_devices_with_suffix("-event-mouse", MAX_MICE)
}

fn input_devices_with_suffix(suffix: &str, limit: usize) -> Vec<PathBuf> {
    let mut devices = HashSet::new();
    for directory in ["/dev/input/by-id", "/dev/input/by-path"] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(MAX_DEVICE_LINKS_SCANNED) {
            if !device_link_name_matches(&entry.file_name().to_string_lossy(), suffix) {
                continue;
            }
            if let Ok(path) = fs::canonicalize(entry.path())
                && path.starts_with("/dev/input")
            {
                devices.insert(path);
            }
            if devices.len() >= limit {
                break;
            }
        }
    }
    let mut devices = devices.into_iter().collect::<Vec<_>>();
    devices.sort();
    devices.truncate(limit);
    devices
}

fn device_link_name_matches(name: &str, suffix: &str) -> bool {
    !name.contains('/') && name.ends_with(suffix)
}

fn open_mouse_devices(paths: &[PathBuf]) -> Vec<Device> {
    paths
        .iter()
        .filter_map(|path| {
            let device = Device::open(path).ok()?;
            let has_motion = device.supported_relative_axes().is_some_and(|axes| {
                axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y)
            });
            let has_left = device
                .supported_keys()
                .is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT));
            (has_motion && has_left && device.set_nonblocking(true).is_ok()).then_some(device)
        })
        .take(MAX_MICE)
        .collect()
}

fn read_keyboard(
    device: usize,
    path: &Path,
    stop: &AtomicBool,
    sender: &mpsc::SyncSender<KeyEvent>,
) {
    let Ok(mut input) = File::open(path) else {
        return;
    };
    let mut bytes = [0_u8; INPUT_EVENT_BYTES];
    while !stop.load(Ordering::Acquire) && input.read_exact(&mut bytes).is_ok() {
        let event_type = u16::from_ne_bytes([bytes[16], bytes[17]]);
        if event_type != EV_KEY {
            continue;
        }
        let event = KeyEvent {
            device,
            code: u16::from_ne_bytes([bytes[18], bytes[19]]),
            value: i32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
        };
        if sender.send(event).is_err() {
            break;
        }
    }
}

fn monitor_events(
    receiver: &mpsc::Receiver<KeyEvent>,
    stop: &AtomicBool,
    bindings: &[KeyChord],
    control_path: &Path,
    mice: &mut [Device],
) -> Result<(), String> {
    let mut pressed = HashSet::new();
    let mut menu = MenuCapture::closed(Instant::now());
    while !stop.load(Ordering::Acquire) {
        match receiver.recv_timeout(EVENT_TICK) {
            Ok(event) => {
                let identity = (event.device, event.code);
                match event.value {
                    0 => {
                        pressed.remove(&identity);
                    }
                    1 => {
                        let first_logical_press = logical_press_is_new(&pressed, event.code);
                        pressed.insert(identity);
                        // Composite USB keyboards can expose the same physical
                        // keys through more than one `-event-kbd` interface.
                        // Treat those duplicate edges as one logical press so
                        // an overlay toggle cannot immediately toggle itself
                        // closed and a save cannot be submitted twice.
                        if !first_logical_press {
                            continue;
                        }
                        if menu.open && event.code == KEY_ESC {
                            close_menu(control_path, mice, &mut menu, b"MENU ESCAPE\n");
                            continue;
                        }
                        let modifiers = active_modifiers(&pressed);
                        let action = bindings
                            .iter()
                            .find(|binding| {
                                binding.key == event.code && binding.modifiers == modifiers
                            })
                            .map(|binding| binding.action);
                        // The Replay menu is mouse-only. Any unrelated
                        // non-modifier key closes it before that same event is
                        // handled by the game or desktop (notably Alt+Tab), so
                        // an evdev mouse grab can never strand app switching.
                        if menu.open
                            && action != Some(HotkeyAction::ToggleOverlay)
                            && !is_modifier_key(event.code)
                        {
                            close_menu(control_path, mice, &mut menu, b"MENU ESCAPE\n");
                        }
                        if let Some(action) = action {
                            match action {
                                HotkeyAction::ToggleOverlay => {
                                    println!("OVERLAY");
                                }
                                HotkeyAction::Save(seconds) => {
                                    let command = format!("SAVE {seconds}\n");
                                    match request_save(control_path, command.as_bytes()) {
                                        Ok(()) => println!("ACTIVATED {seconds}"),
                                        Err(error) => println!("REJECTED {error}"),
                                    }
                                }
                            }
                            io::stdout().flush().map_err(|error| error.to_string())?;
                        }
                    }
                    _ => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if menu.open && !process_mouse_events(mice, &mut menu) {
            release_all(mice);
            menu.closed_by_release();
        }
        if menu.open && watchdog_action(&menu, Instant::now()) != WatchdogAction::None {
            close_menu(control_path, mice, &mut menu, b"MENU ESCAPE\n");
        }
    }
    release_all(mice);
    Ok(())
}

fn request_control(control_path: &Path, command: &[u8]) -> Result<String, String> {
    let mut socket = UnixStream::connect(control_path)
        .map_err(|error| format!("Replay control connection failed: {error}"))?;
    request_on_stream(&mut socket, command)
}

fn request_on_stream(socket: &mut UnixStream, command: &[u8]) -> Result<String, String> {
    socket
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|error| error.to_string())?;
    socket
        .write_all(command)
        .map_err(|error| format!("Replay control request failed: {error}"))?;
    let mut response = String::new();
    BufReader::new(socket)
        .read_line(&mut response)
        .map_err(|error| format!("Replay control response failed: {error}"))?;
    if let Some(error) = response.strip_prefix("ERROR ") {
        return Err(error.trim_end().to_owned());
    }
    if response != "OK\n" {
        if response == "OK GRAB\n" || response == "OK RELEASE\n" {
            return Ok(response);
        }
        return Err("Replay control returned an invalid response".to_owned());
    }
    Ok(response)
}

fn request_save(control_path: &Path, command: &[u8]) -> Result<(), String> {
    match request_control(control_path, command)? {
        response if response == "OK\n" => Ok(()),
        _ => Err("Replay control returned an invalid save response".to_owned()),
    }
}

#[allow(
    dead_code,
    reason = "menu mouse capture remains dormant in the restored helper path"
)]
fn toggle_menu(control_path: &Path, mice: &mut [Device], menu: &mut MenuCapture) {
    if menu.open {
        close_menu(control_path, mice, menu, b"MENU TOGGLE\n");
        return;
    }
    if mice.is_empty() {
        println!("REJECTED no bounded mouse input device is available");
        return;
    }
    let mut control = match UnixStream::connect(control_path) {
        Ok(control) => control,
        Err(error) => {
            println!("REJECTED Replay control connection failed: {error}");
            return;
        }
    };
    match request_on_stream(&mut control, b"MENU TOGGLE\n") {
        Ok(response) if response == "OK GRAB\n" => match grab_all(mice) {
            Ok(()) => menu.opened(control, Instant::now()),
            Err(error) => {
                release_all(mice);
                let _ = request_on_stream(&mut control, b"MENU ESCAPE\n");
                println!("REJECTED mouse capture failed: {error}");
            }
        },
        Ok(_) => println!("REJECTED Replay menu returned an invalid open response"),
        Err(error) => println!("REJECTED {error}"),
    }
}

fn close_menu(control_path: &Path, mice: &mut [Device], menu: &mut MenuCapture, command: &[u8]) {
    // Release locally even when the UI/control owner has disappeared. The
    // kernel also releases EVIOCGRAB when these file descriptors are dropped.
    let _ = menu
        .request(command)
        .or_else(|_| request_control(control_path, command));
    release_all(mice);
    menu.closed_by_release();
}

#[allow(
    dead_code,
    reason = "menu mouse capture remains dormant in the restored helper path"
)]
fn grab_all(devices: &mut [Device]) -> Result<(), String> {
    for grabbed in 0..devices.len() {
        let device = &mut devices[grabbed];
        if let Err(error) = device.grab() {
            for rollback in devices.iter_mut().take(grabbed) {
                let _ = rollback.ungrab();
            }
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn release_all(devices: &mut [Device]) {
    for device in devices {
        let _ = device.ungrab();
    }
}

fn process_mouse_events(mice: &mut [Device], menu: &mut MenuCapture) -> bool {
    let mut pointer_events = Vec::new();
    for mouse in mice {
        match mouse.fetch_events() {
            Ok(events) => pointer_events.extend(events.filter_map(|event| {
                parse_pointer_event(event.event_type().0, event.code(), event.value())
            })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => return false,
        }
    }
    for event in pointer_events {
        menu.last_input = Instant::now();
        match event {
            PointerEvent::MotionX(_) | PointerEvent::MotionY(_) => menu.pending_motion.add(event),
            PointerEvent::LeftButton(pressed) => {
                if !send_menu_button(menu, pressed).unwrap_or(false) {
                    return false;
                }
            }
        }
    }
    let now = Instant::now();
    if now.duration_since(menu.last_motion_sent) >= MOTION_INTERVAL
        && let Some((x, y)) = menu.pending_motion.take()
    {
        let (x, y) = accelerated_pointer_motion(x, y);
        let command = format!("MENU MOVE {x} {y}\n");
        if !request_keeps_menu_open(menu, command.as_bytes()).unwrap_or(false) {
            return false;
        }
        menu.last_motion_sent = now;
    }
    if now.duration_since(menu.last_ping) >= MENU_PING_INTERVAL {
        if !request_keeps_menu_open(menu, b"MENU PING\n").unwrap_or(false) {
            return false;
        }
        menu.last_ping = now;
    }
    true
}

fn send_menu_button(menu: &mut MenuCapture, pressed: bool) -> Result<bool, String> {
    request_keeps_menu_open(
        menu,
        if pressed {
            b"MENU BUTTON 1\n"
        } else {
            b"MENU BUTTON 0\n"
        },
    )
}

fn request_keeps_menu_open(menu: &mut MenuCapture, command: &[u8]) -> Result<bool, String> {
    menu.request(command)
        .map(|response| response != "OK RELEASE\n")
}

fn parse_pointer_event(event_type: u16, code: u16, value: i32) -> Option<PointerEvent> {
    match (event_type, code, value) {
        (kind, 0, value) if kind == EventType::RELATIVE.0 => Some(PointerEvent::MotionX(value)),
        (kind, 1, value) if kind == EventType::RELATIVE.0 => Some(PointerEvent::MotionY(value)),
        (EV_KEY, BTN_LEFT, 0) => Some(PointerEvent::LeftButton(false)),
        (EV_KEY, BTN_LEFT, 1) => Some(PointerEvent::LeftButton(true)),
        _ => None,
    }
}

fn bounded_motion(current: i32, delta: i32) -> i32 {
    current
        .saturating_add(delta)
        .clamp(-MOTION_LIMIT, MOTION_LIMIT)
}

fn accelerated_pointer_motion(x: i32, y: i32) -> (i32, i32) {
    let speed = i64::from(x.unsigned_abs().max(y.unsigned_abs())).min(POINTER_SPEED_LIMIT);
    let gain = POINTER_BASE_GAIN_MILLI + speed * POINTER_SPEED_GAIN_MILLI;
    let scaled = |value: i32, axis_ratio: i64| {
        let numerator = i64::from(value)
            .saturating_mul(gain)
            .saturating_mul(axis_ratio);
        let denominator = 1_000_000_i64;
        let rounded = if numerator >= 0 {
            numerator.saturating_add(denominator / 2)
        } else {
            numerator.saturating_sub(denominator / 2)
        } / denominator;
        i32::try_from(rounded)
            .unwrap_or(if rounded < 0 { i32::MIN } else { i32::MAX })
            .clamp(-MOTION_LIMIT, MOTION_LIMIT)
    };
    (scaled(x, 1_000), scaled(y, POINTER_VERTICAL_RATIO_MILLI))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchdogAction {
    None,
    Inactive,
}

fn watchdog_action(menu: &MenuCapture, now: Instant) -> WatchdogAction {
    if menu.open && now.duration_since(menu.last_input) >= MENU_INACTIVITY_TIMEOUT {
        WatchdogAction::Inactive
    } else {
        WatchdogAction::None
    }
}

fn active_modifiers(pressed: &HashSet<(usize, u16)>) -> u8 {
    let mut active = HashMap::new();
    for &(_, code) in pressed {
        active.insert(code, true);
    }
    let mut modifiers = 0;
    if active.contains_key(&KEY_LEFTCTRL) || active.contains_key(&KEY_RIGHTCTRL) {
        modifiers |= MOD_CONTROL;
    }
    if active.contains_key(&KEY_LEFTSHIFT) || active.contains_key(&KEY_RIGHTSHIFT) {
        modifiers |= MOD_SHIFT;
    }
    if active.contains_key(&KEY_LEFTALT) || active.contains_key(&KEY_RIGHTALT) {
        modifiers |= MOD_ALT;
    }
    if active.contains_key(&KEY_LEFTMETA) || active.contains_key(&KEY_RIGHTMETA) {
        modifiers |= MOD_META;
    }
    modifiers
}

fn logical_press_is_new(pressed: &HashSet<(usize, u16)>, code: u16) -> bool {
    !pressed
        .iter()
        .any(|&(_, pressed_code)| pressed_code == code)
}

fn is_modifier_key(code: u16) -> bool {
    matches!(
        code,
        KEY_LEFTCTRL
            | KEY_RIGHTCTRL
            | KEY_LEFTSHIFT
            | KEY_RIGHTSHIFT
            | KEY_LEFTALT
            | KEY_RIGHTALT
            | KEY_LEFTMETA
            | KEY_RIGHTMETA
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_parser_is_bounded_and_supports_function_key_chords() {
        assert_eq!(parse_chord("F8").expect("F8").key, 66);
        assert_eq!(parse_chord("Shift+Tab").expect("Shift+Tab").key, 15);
        let chord = parse_chord("Ctrl+Shift+F10").expect("modified key");
        assert_eq!(chord.modifiers, MOD_CONTROL | MOD_SHIFT);
        assert_eq!(chord.key, 68);
        assert!(parse_chord("VolumeUp").is_err());
        assert!(parse_chord("F8+F9").is_err());
    }

    #[test]
    fn configuration_accepts_only_known_durations() {
        let mut valid = BufReader::new(&b"TOGGLE Shift+Tab\nBIND F8 30\nSTART\n"[..]);
        assert_eq!(
            read_bindings(&mut valid).expect("valid")[1].action,
            HotkeyAction::Save(30)
        );
        let mut invalid = BufReader::new(&b"TOGGLE Shift+Tab\nBIND F8 31\nSTART\n"[..]);
        assert!(read_bindings(&mut invalid).is_err());
        let mut missing_toggle = BufReader::new(&b"BIND F8 30\nSTART\n"[..]);
        assert_eq!(
            read_bindings(&mut missing_toggle).unwrap()[0].action,
            HotkeyAction::Save(30)
        );
        let mut menu_only = BufReader::new(&b"TOGGLE Shift+Tab\nSTART\n"[..]);
        assert_eq!(read_bindings(&mut menu_only).unwrap().len(), 1);
    }

    #[test]
    fn process_identity_reads_effective_uid_and_nonzero_parent() {
        let status = "Name:\tredunar-hotkey\nPPid:\t4321\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_process_identity(status), Ok((1000, 4_321)));
        assert_eq!(
            parse_process_identity("Uid:\t0\t0\t0\t0\nPPid:\t1\n"),
            Ok((0, 1))
        );
        assert!(parse_process_identity("PPid:\t1\n").is_err());
    }

    #[test]
    fn checkout_release_parent_is_trusted_only_with_adjacent_helper() {
        let directory =
            std::env::temp_dir().join(format!("redunar-hotkey-trust-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("trust fixture directory");
        let parent = directory.join("redunar-tauri");
        let helper = directory.join("redunar-hotkey-helper");
        std::fs::write(&helper, b"fixture").expect("trust fixture helper");
        assert!(trusted_parent(&parent));
        std::fs::remove_file(helper).expect("remove trust fixture helper");
        assert!(!trusted_parent(&parent));
        std::fs::remove_dir(directory).expect("remove trust fixture directory");
    }

    #[test]
    fn mouse_device_links_are_bounded_to_kernel_mouse_event_aliases() {
        assert!(device_link_name_matches(
            "usb-Example_Mouse-event-mouse",
            "-event-mouse"
        ));
        assert!(device_link_name_matches(
            "pci-0000:00:14.0-usb-0:2:1.0-event-mouse",
            "-event-mouse"
        ));
        assert!(!device_link_name_matches("mouse0", "-event-mouse"));
        assert!(!device_link_name_matches(
            "nested/device-event-mouse",
            "-event-mouse"
        ));
    }

    #[test]
    fn pointer_parser_accepts_only_relative_motion_and_left_button_edges() {
        assert_eq!(
            parse_pointer_event(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 17),
            Some(PointerEvent::MotionX(17))
        );
        assert_eq!(
            parse_pointer_event(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, -9),
            Some(PointerEvent::MotionY(-9))
        );
        assert_eq!(
            parse_pointer_event(EV_KEY, BTN_LEFT, 1),
            Some(PointerEvent::LeftButton(true))
        );
        assert_eq!(parse_pointer_event(EV_KEY, BTN_LEFT, 2), None);
        assert_eq!(parse_pointer_event(EV_KEY, 273, 1), None);
    }

    #[test]
    fn motion_is_coalesced_and_bounded_before_crossing_the_socket() {
        let mut pending = PendingMotion::default();
        pending.add(PointerEvent::MotionX(3_000));
        pending.add(PointerEvent::MotionX(3_000));
        pending.add(PointerEvent::MotionY(-5_000));
        assert_eq!(pending.take(), Some((4_096, -4_096)));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn raw_mouse_motion_gets_continuous_aspect_corrected_acceleration() {
        let slow = accelerated_pointer_motion(1, 1);
        let medium = accelerated_pointer_motion(6, 6);
        let fast = accelerated_pointer_motion(12, 12);
        assert!(slow.0 > 0);
        assert!(slow.1 > slow.0);
        assert!(medium.0 > slow.0 * 6);
        assert!(fast.0 > medium.0 * 2);
        assert_eq!(accelerated_pointer_motion(i32::MAX, 0).0, MOTION_LIMIT);
    }

    #[test]
    fn menu_state_clears_pending_input_on_every_release() {
        let now = Instant::now();
        let mut menu = MenuCapture::closed(now);
        let (control, _peer) = UnixStream::pair().expect("local control pair");
        menu.opened(control, now);
        menu.pending_motion.add(PointerEvent::MotionX(4));
        menu.closed_by_release();
        assert!(!menu.open);
        assert_eq!(menu.pending_motion.take(), None);
    }

    #[test]
    fn duplicate_composite_keyboard_edges_trigger_once() {
        let mut pressed = HashSet::new();
        assert!(logical_press_is_new(&pressed, 66));
        pressed.insert((0, 66));
        assert!(!logical_press_is_new(&pressed, 66));
        pressed.insert((1, 66));
        pressed.remove(&(0, 66));
        assert!(!logical_press_is_new(&pressed, 66));
        pressed.remove(&(1, 66));
        assert!(logical_press_is_new(&pressed, 66));
    }

    #[test]
    fn watchdog_closes_only_an_open_inactive_menu() {
        let now = Instant::now();
        let mut menu = MenuCapture::closed(now);
        assert_eq!(
            watchdog_action(&menu, now + MENU_INACTIVITY_TIMEOUT),
            WatchdogAction::None
        );
        let (control, _peer) = UnixStream::pair().expect("local control pair");
        menu.opened(control, now);
        assert_eq!(
            watchdog_action(
                &menu,
                (now + MENU_INACTIVITY_TIMEOUT)
                    .checked_sub(Duration::from_millis(1))
                    .expect("one millisecond is below the watchdog duration"),
            ),
            WatchdogAction::None
        );
        assert_eq!(
            watchdog_action(&menu, now + MENU_INACTIVITY_TIMEOUT),
            WatchdogAction::Inactive
        );
    }

    #[test]
    fn only_modifier_keys_may_leave_the_mouse_menu_open_for_a_chord() {
        assert!(is_modifier_key(KEY_LEFTALT));
        assert!(is_modifier_key(KEY_RIGHTSHIFT));
        assert!(!is_modifier_key(15));
        assert!(!is_modifier_key(KEY_ESC));
    }
}
