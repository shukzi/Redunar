use super::*;
use crate::session_lifecycle::{Event, Outcome};

fn completed_fixture() -> (Fixture, u64) {
    let mut f = Fixture::new();
    f.engine.service.set_close_to_tray_enabled(false).unwrap();
    f.engine.start(f.plan("/bin/true", &[])).unwrap();
    f.reap();
    let Event {
        generation,
        outcome,
    } = f.engine.lifecycle.pending.take().unwrap();
    assert_eq!(outcome, Outcome::Finished);
    assert!(!f.engine.has_live_game());
    assert_eq!(f.engine.lifecycle.finished_generation(), Some(generation));
    assert_eq!(f.engine.service.session_history().unwrap().len(), 1);
    (f, generation)
}

#[test]
fn completion_claims_exit_once_and_rejects_new_launches() {
    let (mut f, generation) = completed_fixture();
    assert!(f.engine.prepare_background_exit(generation).unwrap());
    assert!(!f.engine.prepare_background_exit(generation).unwrap());
    assert!(f.engine.ensure_idle().is_err());
    f.engine.tick(&MonitorSnapshot::default());
    assert!(f.engine.lifecycle.pending.is_none());
}

#[test]
fn delayed_completion_cannot_quit_a_newer_session() {
    let (mut f, old_generation) = completed_fixture();
    f.engine.start(f.plan("/bin/true", &[])).unwrap();
    assert!(!f.engine.prepare_background_exit(old_generation).unwrap());
    f.reap();
    let new_generation = f.engine.lifecycle.pending.take().unwrap().generation;
    assert_ne!(old_generation, new_generation);
    assert!(!f.engine.prepare_background_exit(old_generation).unwrap());
    assert!(f.engine.prepare_background_exit(new_generation).unwrap());
    assert_eq!(f.engine.service.session_history().unwrap().len(), 2);
}

#[test]
fn tray_preference_is_checked_at_completion_and_failure_keeps_owner_alive() {
    let (mut f, generation) = completed_fixture();
    f.engine.service.set_close_to_tray_enabled(true).unwrap();
    assert!(!f.engine.prepare_background_exit(generation).unwrap());
    std::fs::write(f.root.join("state/app-preferences-v1.txt"), "corrupt").unwrap();
    assert!(f.engine.prepare_background_exit(generation).is_err());
    assert!(!f.engine.shutdown);
    std::fs::remove_file(f.root.join("state/app-preferences-v1.txt")).unwrap();
    f.engine.service.set_close_to_tray_enabled(false).unwrap();
    assert!(f.engine.prepare_background_exit(generation).unwrap());
}

#[test]
fn end_keeps_a_live_game_owned_until_natural_exit() {
    let mut f = Fixture::new();
    let mut plan = f.plan("/bin/cat", &[]);
    plan.command.stdin(Stdio::piped());
    f.engine.start(plan).unwrap();
    f.engine.finish().unwrap();
    f.engine.tick(&MonitorSnapshot::default());
    assert!(f.engine.launch_locked());
    assert!(f.engine.has_live_game());
    assert!(f.engine.lifecycle.pending.is_none());
    let child = f.engine.active.as_mut().unwrap().child.child_mut();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    f.engine.tick(&MonitorSnapshot::default());
    assert!(!f.engine.launch_locked());
    assert!(!f.engine.has_live_game());
    assert_eq!(
        f.engine.lifecycle.pending.as_ref().unwrap().outcome,
        Outcome::Finished
    );
    assert_eq!(f.engine.service.session_history().unwrap().len(), 1);
}

#[test]
fn failed_history_emits_attention_once_and_retry_completes_once() {
    let mut f = Fixture::new();
    f.engine.start(f.plan("/bin/true", &[])).unwrap();
    std::fs::create_dir_all(f.root.join("state")).unwrap();
    let history = f.root.join("state/session-history-v1.tsv");
    std::fs::write(&history, "invalid header").unwrap();
    f.reap();
    let event = f.engine.lifecycle.pending.take().unwrap();
    assert!(matches!(event.outcome, Outcome::NeedsAttention(_)));
    assert!(!f.engine.has_live_game());
    assert!(f.engine.can_end());
    assert_eq!(f.engine.lifecycle.finished_generation(), None);
    assert!(!f.engine.prepare_background_exit(event.generation).unwrap());
    f.engine.tick(&MonitorSnapshot::default());
    assert!(f.engine.lifecycle.pending.is_none());
    std::fs::remove_file(history).unwrap();
    f.engine.finish().unwrap();
    assert_eq!(
        f.engine.lifecycle.pending.take().unwrap().outcome,
        Outcome::Finished
    );
    f.engine.tick(&MonitorSnapshot::default());
    assert!(f.engine.lifecycle.pending.is_none());
    assert_eq!(f.engine.service.session_history().unwrap().len(), 1);
}

#[test]
fn native_worker_notifies_without_ui_polling_and_outside_ownership_locks() {
    let (f, generation) = completed_fixture();
    let shared = Arc::new(Shared {
        engine: Mutex::new(Engine::new(f.engine.service.clone())),
        wake: Condvar::new(),
        lifecycle_handler: Mutex::new(None),
    });
    // Queue completion before attaching the desktop, as a fast Steam game can
    // finish while GTK starts. No live monitor, input device or app is needed.
    {
        let mut engine = lock(&shared.engine);
        engine.lifecycle.started();
        engine.lifecycle.finished();
    }
    let worker = start_worker(shared.clone(), || Arc::new(MonitorSnapshot::default())).unwrap();
    let sessions = Sessions {
        shared: shared.clone(),
        worker: Mutex::new(Some(worker)),
        steam: Mutex::new(None),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let weak = Arc::downgrade(&shared);
    sessions.set_lifecycle_handler(move |event| {
        let shared = weak.upgrade().unwrap();
        assert!(!lock(&shared.engine).launch_locked());
        assert!(shared.lifecycle_handler.try_lock().is_ok());
        sender.send(event).unwrap();
    });
    let event = receiver.recv_timeout(Duration::from_secs(3));
    sessions.shutdown();
    assert_eq!(
        event.unwrap(),
        Event {
            generation,
            outcome: Outcome::Finished
        }
    );
    assert!(receiver.try_recv().is_err());
}
