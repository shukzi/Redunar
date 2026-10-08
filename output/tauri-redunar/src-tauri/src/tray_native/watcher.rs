//! Registration follows the current watcher owner and this private connection.
//! A method acknowledgement or another app's tray icon never grants hiding.
use super::{Action, Callback, Shared, ITEM_PATH, WATCHER, WATCHER_PATH};
use glib::{variant::ToVariant, Variant};
use gtk::{gio, glib};

pub(super) struct State {
    running: bool,
    owner: Option<String>,
    generation: u64,
    accepted: bool,
    registered: bool,
    pending: bool,
    dirty: bool,
    query_revision: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            running: true,
            owner: None,
            generation: 0,
            accepted: false,
            registered: false,
            pending: false,
            dirty: false,
            query_revision: 0,
        }
    }
}

pub(super) fn running(state: &Shared) -> bool {
    state.lock().is_ok_and(|state| state.running)
}

pub(super) fn stop(state: &Shared) {
    if let Ok(mut state) = state.lock() {
        state.running = false;
        state.registered = false;
        state.generation += 1;
    }
}

fn replace_owner(state: &Shared, owner: Option<String>, action: &Callback) -> u64 {
    let mut state = state.lock().expect("tray state");
    let recover = state.registered;
    state.generation += 1;
    state.owner = owner;
    state.accepted = false;
    state.registered = false;
    state.pending = false;
    state.dirty = false;
    state.query_revision += 1;
    let generation = state.generation;
    drop(state);
    if recover {
        action(Action::Recover);
    }
    generation
}

fn snapshot(state: &Shared) -> Option<(String, u64)> {
    let state = state.lock().ok()?;
    state.running.then_some(())?;
    Some((state.owner.clone()?, state.generation))
}

fn apply(
    state: &Shared,
    owner: &str,
    generation: u64,
    value: bool,
    revision: u64,
    action: &Callback,
) -> bool {
    let mut state = state.lock().expect("tray state");
    if state.query_revision != revision
        || !state.running
        || state.generation != generation
        || state.owner.as_deref() != Some(owner)
    {
        return false;
    }
    let value = value && state.accepted;
    let recover = state.registered && !value;
    state.registered = value;
    drop(state);
    if recover {
        action(Action::Recover);
    }
    value
}

fn owns(items: &Variant, name: &str) -> bool {
    if !items.is_type(glib::VariantTy::STRING_ARRAY) || items.n_children() > 1024 {
        return false;
    }
    let path = format!("{name}{ITEM_PATH}");
    (0..items.n_children()).any(|index| {
        let item = items.child_value(index);
        item.str()
            .is_some_and(|entry| entry == name || entry == path)
    })
}

fn properties_registered(reply: &Variant, name: &str) -> bool {
    if reply.type_().as_str() != "(a{sv})" {
        return false;
    }
    let dict = glib::VariantDict::new(Some(&reply.child_value(0)));
    dict.lookup::<bool>("IsStatusNotifierHostRegistered")
        .ok()
        .flatten()
        == Some(true)
        && dict
            .lookup_value(
                "RegisteredStatusNotifierItems",
                Some(glib::VariantTy::STRING_ARRAY),
            )
            .is_some_and(|items| owns(&items, name))
}

pub(super) fn verify_sync(bus: &gio::DBusConnection, state: &Shared, action: &Callback) -> bool {
    let Some((owner, generation)) = snapshot(state) else {
        return false;
    };
    let revision = {
        let mut current = state.lock().expect("tray state");
        current.query_revision += 1;
        current.query_revision
    };
    // The only synchronous query runs at close/background reachability checks,
    // with a 250 ms limit. No D-Bus round trip runs in the measurement loop.
    let valid = bus
        .call_sync(
            Some(&owner),
            WATCHER_PATH,
            "org.freedesktop.DBus.Properties",
            "GetAll",
            Some(&(WATCHER,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            250,
            gio::Cancellable::NONE,
        )
        .ok()
        .is_some_and(|reply| {
            bus.unique_name()
                .is_some_and(|name| properties_registered(&reply, &name))
        });
    apply(state, &owner, generation, valid, revision, action)
}

fn refresh(
    bus: &gio::DBusConnection,
    state: &Shared,
    action: &Callback,
    cancel: &gio::Cancellable,
) {
    let Some((owner, generation)) = snapshot(state) else {
        return;
    };
    let revision = {
        let mut state = state.lock().expect("tray state");
        if !state.accepted {
            return;
        }
        if state.pending {
            state.dirty = true;
            return;
        }
        state.pending = true;
        state.query_revision += 1;
        state.query_revision
    };
    let state = state.clone();
    let action = action.clone();
    let name = bus
        .unique_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    let destination = owner.clone();
    let next_bus = bus.clone();
    let next_cancel = cancel.clone();
    bus.call(
        Some(&destination),
        WATCHER_PATH,
        "org.freedesktop.DBus.Properties",
        "GetAll",
        Some(&(WATCHER,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        250,
        Some(cancel),
        move |reply| {
            let valid = reply.is_ok_and(|reply| properties_registered(&reply, &name));
            let mut current = state.lock().expect("tray state");
            if current.generation != generation || current.owner.as_deref() != Some(&owner) {
                return;
            }
            current.pending = false;
            let dirty = current.dirty;
            current.dirty = false;
            drop(current);
            if dirty {
                refresh(&next_bus, &state, &action, &next_cancel);
            } else {
                apply(&state, &owner, generation, valid, revision, &action);
            }
        },
    );
}

fn register(
    bus: &gio::DBusConnection,
    state: &Shared,
    action: &Callback,
    cancel: &gio::Cancellable,
) {
    let Some((owner, generation)) = snapshot(state) else {
        return;
    };
    let Some(name) = bus.unique_name() else {
        return;
    };
    let connection = bus.clone();
    let state = state.clone();
    let action = action.clone();
    let callback_cancel = cancel.clone();
    let destination = owner.clone();
    bus.call(
        Some(&destination),
        WATCHER_PATH,
        WATCHER,
        "RegisterStatusNotifierItem",
        Some(&(name.as_str(),).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        500,
        Some(cancel),
        move |reply| {
            let mut current = state.lock().expect("tray state");
            if !current.running
                || current.generation != generation
                || current.owner.as_deref() != Some(&owner)
            {
                return;
            }
            current.accepted = reply.is_ok();
            drop(current);
            refresh(&connection, &state, &action, &callback_cancel);
        },
    );
}

pub(super) fn observe(
    bus: &gio::DBusConnection,
    state: Shared,
    action: Callback,
    cancel: &gio::Cancellable,
) -> Vec<gio::SignalSubscriptionId> {
    // Observe the bus's authenticated owner changes directly. The GIO 0.18
    // name-watch closure cannot handle the null connection on bus disconnect.
    let owner_state = state.clone();
    let owner_action = action.clone();
    let owner_cancel = cancel.clone();
    let owner_changes = bus.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(WATCHER),
        gio::DBusSignalFlags::NONE,
        move |bus, _, _, _, _, params| {
            let Some((name, _, owner)) = params.get::<(String, String, String)>() else {
                return;
            };
            if name == WATCHER && running(&owner_state) {
                replace_owner(
                    &owner_state,
                    (!owner.is_empty()).then_some(owner),
                    &owner_action,
                );
                register(bus, &owner_state, &owner_action, &owner_cancel);
            }
        },
    );
    let initial_bus = bus.clone();
    let initial_state = state.clone();
    let initial_action = action.clone();
    let initial_cancel = cancel.clone();
    bus.call(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetNameOwner",
        Some(&(WATCHER,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        500,
        Some(cancel),
        move |reply| {
            let current = initial_state.lock().expect("tray state");
            if !current.running || current.generation != 0 {
                return;
            }
            drop(current);
            let owner = reply
                .ok()
                .and_then(|reply| reply.get::<(String,)>())
                .map(|(owner,)| owner);
            replace_owner(&initial_state, owner, &initial_action);
            register(
                &initial_bus,
                &initial_state,
                &initial_action,
                &initial_cancel,
            );
        },
    );
    let signal_state = state.clone();
    let signal_action = action.clone();
    let signal_cancel = cancel.clone();
    let items = bus.signal_subscribe(
        Some(WATCHER),
        Some(WATCHER),
        None,
        Some(WATCHER_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |bus, sender, _, _, member, _| {
            if snapshot(&signal_state).is_some_and(|(owner, _)| owner == sender) {
                if member == "StatusNotifierHostRegistered" {
                    let accepted = signal_state.lock().is_ok_and(|state| state.accepted);
                    if !accepted {
                        register(bus, &signal_state, &signal_action, &signal_cancel);
                    }
                }
                refresh(bus, &signal_state, &signal_action, &signal_cancel);
            }
        },
    );
    let property_state = state.clone();
    let property_action = action.clone();
    let property_cancel = cancel.clone();
    let properties = bus.signal_subscribe(
        Some(WATCHER),
        Some("org.freedesktop.DBus.Properties"),
        Some("PropertiesChanged"),
        Some(WATCHER_PATH),
        Some(WATCHER),
        gio::DBusSignalFlags::NONE,
        move |bus, sender, _, _, _, _| {
            if snapshot(&property_state).is_some_and(|(owner, _)| owner == sender) {
                refresh(bus, &property_state, &property_action, &property_cancel);
            }
        },
    );
    bus.connect_closed(move |_, _, _| {
        let recover = state.lock().is_ok_and(|state| state.running);
        stop(&state);
        if recover {
            action(Action::Recover);
        }
    });
    vec![owner_changes, items, properties]
}
