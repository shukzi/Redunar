//! Minimal StatusNotifier host on the probe's private bus, never the user's bus.
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::sync::{Arc, Mutex};

const NAME: &str = "org.kde.StatusNotifierWatcher";
const PATH: &str = "/StatusNotifierWatcher";
const XML: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
</interface></node>"#;

pub struct Host {
    main_loop: glib::MainLoop,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub fn start() -> Host {
    let context = glib::MainContext::new();
    let main_loop = glib::MainLoop::new(Some(&context), false);
    let run_loop = main_loop.clone();
    let (ready, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        context
            .with_thread_default(|| {
                let bus = connect();
                ready.send(()).unwrap();
                run_loop.run();
                bus.close_sync(gio::Cancellable::NONE).unwrap();
            })
            .unwrap();
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    Host {
        main_loop,
        thread: Some(thread),
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.main_loop.quit();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn connect() -> gio::DBusConnection {
    let bus = gio::DBusConnection::for_address_sync(
        &std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        gio::Cancellable::NONE,
    )
    .unwrap();
    let items = Arc::new(Mutex::new(Vec::<String>::new()));
    let registered = items.clone();
    let info = gio::DBusNodeInfo::for_xml(XML)
        .unwrap()
        .lookup_interface(NAME)
        .unwrap();
    bus.register_object(
        PATH,
        &info,
        move |connection, _, _, _, method, parameters, invocation| {
            assert_eq!(method, "RegisterStatusNotifierItem");
            let (name,) = parameters.get::<(String,)>().unwrap();
            registered.lock().unwrap().push(name.clone());
            invocation.return_value(Some(&().to_variant()));
            connection
                .emit_signal(
                    None,
                    PATH,
                    NAME,
                    "StatusNotifierItemRegistered",
                    Some(&(name,).to_variant()),
                )
                .unwrap();
        },
        move |_, _, _, _, property| match property {
            "RegisteredStatusNotifierItems" => items.lock().unwrap().clone().to_variant(),
            "IsStatusNotifierHostRegistered" => true.to_variant(),
            "ProtocolVersion" => 0_i32.to_variant(),
            _ => panic!("Unknown fixture property"),
        },
        |_, _, _, _, _, _| false,
    )
    .unwrap();
    let owner = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&(NAME, 4_u32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2_000,
            gio::Cancellable::NONE,
        )
        .unwrap();
    assert_eq!(
        owner.get::<(u32,)>().unwrap(),
        (1,),
        "fixture needs a private bus without a tray host"
    );
    bus
}
