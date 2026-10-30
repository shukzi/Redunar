//! Actual exported objects against a private session bus and fake watcher/host.
//! No environment override, desktop app, input device or GPU is opened.
use super::*;
use gio::prelude::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static NEXT_BUS: AtomicU64 = AtomicU64::new(1);
const WATCHER_XML: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
</interface></node>"#;

struct Bus {
    child: Child,
    directory: std::path::PathBuf,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rdtray-{}-{}",
            std::process::id(),
            NEXT_BUS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                directory.join("bus").display()
            ))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(!address.trim().is_empty());
        Self {
            child,
            directory,
            address: address.trim().to_owned(),
        }
    }
    fn connect(&self) -> gio::DBusConnection {
        gio::DBusConnection::for_address_sync(
            &self.address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None,
            gio::Cancellable::NONE,
        )
        .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Accept,
    Reject,
    AcknowledgeOnly,
    Hold,
}
struct HostState {
    mode: Mode,
    host: bool,
    items: Vec<String>,
    pending: usize,
    release_pending: bool,
    observed: Vec<Variant>,
    registrations: usize,
}
struct Host {
    bus: gio::DBusConnection,
    state: Arc<Mutex<HostState>>,
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}
impl Host {
    fn new(bus: &Bus, mode: Mode, host: bool) -> Self {
        let context = glib::MainContext::new();
        let main_loop = glib::MainLoop::new(Some(&context), false);
        let run_loop = main_loop.clone();
        let state = Arc::new(Mutex::new(HostState {
            mode,
            host,
            items: Vec::new(),
            pending: 0,
            release_pending: false,
            observed: Vec::new(),
            registrations: 0,
        }));
        let run_state = state.clone();
        let address = bus.address.clone();
        let (ready, receive) = mpsc::channel();
        let thread = thread::spawn(move || {
            context
                .with_thread_default(|| {
                    let bus = gio::DBusConnection::for_address_sync(
                        &address,
                        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                        None,
                        gio::Cancellable::NONE,
                    )
                    .unwrap();
                    bus.set_exit_on_close(false);
                    let methods = run_state.clone();
                    let props = run_state.clone();
                    let info = gio::DBusNodeInfo::for_xml(WATCHER_XML)
                        .unwrap()
                        .lookup_interface(WATCHER)
                        .unwrap();
                    let registration = bus
                        .register_object(
                            WATCHER_PATH,
                            &info,
                            move |connection, _, _, _, method, params, invocation| {
                                assert_eq!(method, "RegisterStatusNotifierItem");
                                let (name,) = params.get::<(String,)>().unwrap();
                                let mut state = methods.lock().unwrap();
                                state.registrations += 1;
                                match state.mode {
                                    Mode::Reject => invocation.return_dbus_error(
                                        "org.freedesktop.DBus.Error.AccessDenied",
                                        "fixture denial",
                                    ),
                                    Mode::AcknowledgeOnly => {
                                        invocation.return_value(Some(&().to_variant()))
                                    }
                                    Mode::Hold => {
                                        state.pending += 1;
                                        drop(state);
                                        let held = methods.clone();
                                        glib::MainContext::ref_thread_default().spawn_local(
                                            async move {
                                                while !held.lock().unwrap().release_pending {
                                                    glib::timeout_future(Duration::from_millis(5))
                                                        .await;
                                                }
                                                invocation.return_value(Some(&().to_variant()));
                                            },
                                        );
                                    }
                                    Mode::Accept => {
                                        drop(state);
                                        let state = methods.clone();
                                        let connection_reply = connection.clone();
                                        let target = name.clone();
                                        // A real watcher/host can read the item before acknowledging.
                                        connection.call(
                                            Some(&name),
                                            ITEM_PATH,
                                            "org.freedesktop.DBus.Properties",
                                            "GetAll",
                                            Some(&(ITEM_INTERFACE,).to_variant()),
                                            None,
                                            gio::DBusCallFlags::NONE,
                                            500,
                                            gio::Cancellable::NONE,
                                            move |reply| {
                                                let reply = reply.unwrap();
                                                let item = format!("{target}{ITEM_PATH}");
                                                let mut state = state.lock().unwrap();
                                                state.observed.push(reply);
                                                if !state.items.contains(&item) {
                                                    state.items.push(item.clone());
                                                }
                                                drop(state);
                                                let _ = connection_reply.emit_signal(
                                                    None,
                                                    WATCHER_PATH,
                                                    WATCHER,
                                                    "StatusNotifierItemRegistered",
                                                    Some(&(item,).to_variant()),
                                                );
                                                invocation.return_value(Some(&().to_variant()));
                                            },
                                        );
                                    }
                                }
                            },
                            move |_, _, _, _, property| {
                                let state = props.lock().unwrap();
                                match property {
                                    "RegisteredStatusNotifierItems" => state.items.to_variant(),
                                    "IsStatusNotifierHostRegistered" => state.host.to_variant(),
                                    _ => 0_i32.to_variant(),
                                }
                            },
                            |_, _, _, _, _, _| false,
                        )
                        .unwrap();
                    let result = bus
                        .call_sync(
                            Some("org.freedesktop.DBus"),
                            "/org/freedesktop/DBus",
                            "org.freedesktop.DBus",
                            "RequestName",
                            Some(&(WATCHER, 4_u32).to_variant()),
                            None,
                            gio::DBusCallFlags::NONE,
                            500,
                            gio::Cancellable::NONE,
                        )
                        .unwrap();
                    assert_eq!(result.get::<(u32,)>(), Some((1,)));
                    ready.send(bus.clone()).unwrap();
                    run_loop.run();
                    let _ = bus.unregister_object(registration);
                    let _ = bus.close_sync(gio::Cancellable::NONE);
                })
                .unwrap();
        });
        Self {
            bus: receive.recv_timeout(Duration::from_secs(2)).unwrap(),
            state,
            main_loop,
            thread: Some(thread),
        }
    }
    fn changed(&self) {
        let changed = BTreeMap::<String, Variant>::new();
        self.bus
            .emit_signal(
                None,
                WATCHER_PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                Some(
                    &(
                        WATCHER,
                        changed,
                        vec![
                            "IsStatusNotifierHostRegistered",
                            "RegisteredStatusNotifierItems",
                        ],
                    )
                        .to_variant(),
                ),
            )
            .unwrap();
    }
    fn release_name(&self) {
        self.bus
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ReleaseName",
                Some(&(WATCHER,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                500,
                gio::Cancellable::NONE,
            )
            .unwrap();
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let loop_ = self.main_loop.clone();
        self.main_loop.context().invoke(move || loop_.quit());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn icon() -> Icon {
    Icon::from_rgba(1, 1, &[10, 20, 30, 40]).unwrap()
}
fn client(bus: &Bus) -> (NativeTray, Arc<Mutex<Vec<Action>>>) {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let recorded = actions.clone();
    let tray = NativeTray::start_at(
        Some(bus.address.clone()),
        icon(),
        Arc::new(move |action| recorded.lock().unwrap().push(action)),
    )
    .unwrap();
    (tray, actions)
}
fn settle(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline, "private tray fixture timed out");
        thread::sleep(Duration::from_millis(5));
    }
}
fn request(
    bus: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    method: &str,
    params: &Variant,
) -> Result<Variant, glib::Error> {
    bus.call_sync(
        Some(name),
        path,
        interface,
        method,
        Some(params),
        None,
        gio::DBusCallFlags::NONE,
        500,
        gio::Cancellable::NONE,
    )
}

fn assert_connection_removed(peer: &gio::DBusConnection, connection: &gio::DBusConnection) {
    // GIO closes the transport synchronously, but its connection-closed flag and
    // the bus's name removal can arrive later on their separate workers. Require
    // the socket closed immediately, then bounded confirmation from both instead
    // of assuming the first NameHasOwner reply has observed the disconnect.
    assert!(
        connection.stream().is_closed(),
        "tray shutdown left its transport open"
    );
    let name = connection.unique_name().unwrap();
    settle(|| {
        connection.is_closed()
            && request(
                peer,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "NameHasOwner",
                &(name.as_str(),).to_variant(),
            )
            .unwrap()
            .get::<(bool,)>()
                == Some((false,))
    });
}

#[test]
fn real_objects_export_icon_menu_and_typed_actions_without_indicator_library() {
    let bus = Bus::new();
    let host = Host::new(&bus, Mode::Accept, true);
    let (tray, actions) = client(&bus);
    settle(|| tray.registered());
    let peer = bus.connect();
    let name = tray.bus.unique_name().unwrap();
    let props = request(
        &peer,
        &name,
        ITEM_PATH,
        "org.freedesktop.DBus.Properties",
        "GetAll",
        &(ITEM_INTERFACE,).to_variant(),
    )
    .unwrap();
    let (values,) = props.get::<(BTreeMap<String, Variant>,)>().unwrap();
    assert_eq!(
        values["IconPixmap"].get::<Vec<(i32, i32, Vec<u8>)>>(),
        Some(vec![(1, 1, vec![40, 10, 20, 30])])
    );
    assert_eq!(values["Menu"].type_().as_str(), "o");
    assert_eq!(values["ItemIsMenu"].get::<bool>(), Some(false));
    assert_eq!(values["Status"].get::<String>().as_deref(), Some("Active"));
    for method in ["Activate", "SecondaryActivate", "ContextMenu"] {
        request(
            &peer,
            &name,
            ITEM_PATH,
            ITEM_INTERFACE,
            method,
            &(0_i32, 0_i32).to_variant(),
        )
        .unwrap();
    }
    assert_eq!(actions.lock().unwrap().as_slice(), &[Action::Open; 3]);
    let layout = request(
        &peer,
        &name,
        MENU_PATH,
        "com.canonical.dbusmenu",
        "GetLayout",
        &(0_i32, -1_i32, Vec::<String>::new()).to_variant(),
    )
    .unwrap();
    assert_eq!(layout.type_().as_str(), "(u(ia{sv}av))");
    let root = layout.child_value(1);
    let children = root.child_value(2).get::<Vec<Variant>>().unwrap();
    assert_eq!(children.len(), 2);
    for (child, id, label) in [
        (&children[0], 1, "Open Redunar"),
        (&children[1], 2, "Quit Redunar"),
    ] {
        let (actual, properties, descendants) = child
            .get::<(i32, BTreeMap<String, Variant>, Vec<Variant>)>()
            .unwrap();
        assert_eq!(actual, id);
        assert_eq!(properties["label"].get::<String>().as_deref(), Some(label));
        assert!(descendants.is_empty());
        request(
            &peer,
            &name,
            MENU_PATH,
            "com.canonical.dbusmenu",
            "Event",
            &(id, "clicked", 0_i32.to_variant(), 0_u32).to_variant(),
        )
        .unwrap();
    }
    assert_eq!(
        actions.lock().unwrap().as_slice(),
        &[
            Action::Open,
            Action::Open,
            Action::Open,
            Action::Open,
            Action::Quit
        ]
    );
    tray.set_tooltip("Replay saved (30s)");
    let tooltip = request(
        &peer,
        &name,
        ITEM_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        &(ITEM_INTERFACE, "ToolTip").to_variant(),
    )
    .unwrap()
    .child_value(0)
    .as_variant()
    .unwrap();
    assert_eq!(
        tooltip.child_value(3).get::<String>().as_deref(),
        Some("Replay saved (30s)")
    );
    assert_eq!(host.state.lock().unwrap().registrations, 1);
    assert_eq!(host.state.lock().unwrap().observed.len(), 1);
    let connection = tray.bus.clone();
    drop(tray);
    assert_connection_removed(&peer, &connection);
}

#[test]
fn absent_host_denial_and_another_apps_registration_never_allow_hiding() {
    let bus = Bus::new();
    let (tray, _) = client(&bus);
    assert!(!tray.registered());
    drop(tray);
    for (mode, has_host) in [
        (Mode::Reject, true),
        (Mode::AcknowledgeOnly, true),
        (Mode::Accept, false),
    ] {
        let host = Host::new(&bus, mode, has_host);
        host.state
            .lock()
            .unwrap()
            .items
            .push(":9.123/StatusNotifierItem".into());
        let (tray, _) = client(&bus);
        settle(|| host.state.lock().unwrap().registrations == 1);
        assert!(!tray.registered());
        drop(tray);
        drop(host);
    }
}

#[test]
fn host_loss_recovers_and_replacement_discards_old_registration_replies() {
    let bus = Bus::new();
    let host = Host::new(&bus, Mode::Accept, true);
    let (tray, actions) = client(&bus);
    settle(|| tray.registered());
    host.state.lock().unwrap().host = false;
    host.changed();
    settle(|| !tray.registered());
    settle(|| actions.lock().unwrap().contains(&Action::Recover));
    host.state.lock().unwrap().host = true;
    host.changed();
    settle(|| tray.registered());
    host.release_name();
    let delayed = Host::new(&bus, Mode::Hold, true);
    settle(|| delayed.state.lock().unwrap().pending == 1);
    assert!(!tray.registered());
    delayed.release_name();
    let replacement = Host::new(&bus, Mode::AcknowledgeOnly, true);
    settle(|| replacement.state.lock().unwrap().registrations == 1);
    delayed.state.lock().unwrap().release_pending = true;
    thread::sleep(Duration::from_millis(30));
    assert!(!tray.registered());
    assert!(tray.is_running());
    drop(tray);
}

#[test]
fn repeated_connections_remove_the_previous_item_and_bus_loss_recovers() {
    let mut bus = Bus::new();
    let _host = Host::new(&bus, Mode::Accept, true);
    let peer = bus.connect();
    for _ in 0..3 {
        let (tray, _) = client(&bus);
        settle(|| tray.registered());
        let connection = tray.bus.clone();
        drop(tray);
        assert_connection_removed(&peer, &connection);
    }
    let (tray, actions) = client(&bus);
    settle(|| tray.registered());
    bus.child.kill().unwrap();
    bus.child.wait().unwrap();
    settle(|| !tray.is_running());
    settle(|| actions.lock().unwrap().contains(&Action::Recover));
    assert!(!tray.registered());
    drop(tray);
}

#[test]
fn malformed_menu_requests_and_unsupported_icons_are_rejected_without_actions() {
    assert!(Icon::from_rgba(0, 1, &[]).is_err());
    assert!(Icon::from_rgba(257, 1, &vec![0; 257 * 4]).is_err());
    assert!(Icon::from_rgba(1, 1, &[0; 3]).is_err());
    let bus = Bus::new();
    let _host = Host::new(&bus, Mode::Accept, true);
    let (tray, actions) = client(&bus);
    settle(|| tray.registered());
    let peer = bus.connect();
    let name = tray.bus.unique_name().unwrap();
    for params in [
        (9_i32, -1_i32, Vec::<String>::new()).to_variant(),
        (0_i32, -2_i32, Vec::<String>::new()).to_variant(),
        (0_i32, 1_i32, vec!["label"; 17]).to_variant(),
    ] {
        assert!(request(
            &peer,
            &name,
            MENU_PATH,
            "com.canonical.dbusmenu",
            "GetLayout",
            &params
        )
        .is_err());
    }
    assert!(request(
        &peer,
        &name,
        MENU_PATH,
        "com.canonical.dbusmenu",
        "Event",
        &(999_i32, "clicked", 0_i32.to_variant(), 0_u32).to_variant()
    )
    .is_err());
    assert!(request(
        &peer,
        &name,
        ITEM_PATH,
        ITEM_INTERFACE,
        "Scroll",
        &(0_i32, "invalid").to_variant()
    )
    .is_err());
    assert!(actions.lock().unwrap().is_empty());
    let depth_zero = request(
        &peer,
        &name,
        MENU_PATH,
        "com.canonical.dbusmenu",
        "GetLayout",
        &(0_i32, 0_i32, vec!["label"]).to_variant(),
    )
    .unwrap();
    assert_eq!(depth_zero.child_value(1).child_value(2).n_children(), 0);
}

#[test]
fn failed_private_bus_startup_is_bounded_and_cleans_the_worker() {
    let start = Instant::now();
    assert!(NativeTray::start_at(
        Some("unix:path=/tmp/redunar-tray-nonexistent.sock".into()),
        icon(),
        Arc::new(|_| {})
    )
    .is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
}
