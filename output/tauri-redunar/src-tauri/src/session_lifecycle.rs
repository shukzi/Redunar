//! Native completion edges, independent of webview polling and window visibility.
use tauri::Manager;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Finished,
    NeedsAttention(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Event {
    pub generation: u64,
    pub outcome: Outcome,
}

#[derive(Default)]
pub(crate) struct Lifecycle {
    generation: u64,
    finished: bool,
    pub pending: Option<Event>,
}

impl Lifecycle {
    pub fn started(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.finished = false;
        self.pending = None;
    }

    pub fn finished(&mut self) {
        self.finished = true;
        self.pending = Some(Event {
            generation: self.generation,
            outcome: Outcome::Finished,
        });
    }

    pub fn needs_attention(&mut self, message: String) {
        self.finished = false;
        self.pending = Some(Event {
            generation: self.generation,
            outcome: Outcome::NeedsAttention(message),
        });
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }

    pub fn can_exit(&self, generation: u64) -> bool {
        self.finished && self.is_current(generation)
    }
    pub fn finished_generation(&self) -> Option<u64> {
        self.finished.then_some(self.generation)
    }
}

pub(crate) fn install(app: &tauri::AppHandle) {
    let handle = app.clone();
    app.state::<crate::sessions::Sessions>()
        .set_lifecycle_handler(move |event| {
            let app = handle.clone();
            // Queue work without waiting on GTK from the supervisor. Shutdown
            // joins that worker, so a synchronous main-thread wait would deadlock.
            if handle
                .run_on_main_thread(move || handle_event(&app, event))
                .is_err()
            {
                crate::background_start::retain_window(&handle);
                redunar_daemon::log_op!("Session completion could not reach the desktop window");
            }
        });
}

fn handle_event(app: &tauri::AppHandle, event: Event) {
    if !app
        .state::<crate::background_start::State>()
        .may_auto_exit()
        || !app
            .state::<crate::sessions::Sessions>()
            .read(|engine| engine.lifecycle.is_current(event.generation))
    {
        return;
    }
    match event.outcome {
        Outcome::NeedsAttention(message) => {
            if app
                .state::<crate::sessions::Sessions>()
                .read(|engine| engine.lifecycle.is_current(event.generation) && engine.can_end())
            {
                show_failure(app, &message);
            }
        }
        Outcome::Finished => {
            // Focus alone is not user intent: a tiling compositor may focus the
            // untouched window when the game disappears. Native input and Open
            // actions already retain deliberate use before this completion edge.
            match app
                .state::<crate::sessions::Sessions>()
                .prepare_background_exit(event.generation)
            {
                Ok(true) => app.exit(0),
                Ok(false) => {}
                Err(error) => show_failure(app, &error),
            }
        }
    }
}

pub(crate) fn show_failure(app: &tauri::AppHandle, message: &str) {
    crate::window_lifecycle::show_failure(app, message);
}

pub(crate) fn settle_closed_game(app: &tauri::AppHandle) -> bool {
    let sessions = app.state::<crate::sessions::Sessions>();
    let status = sessions.read(|engine| {
        if engine.has_live_game() {
            None
        } else if engine.can_end() {
            Some(Err(engine.message.clone().unwrap_or_else(|| {
                "Game-session cleanup requires attention.".into()
            })))
        } else {
            Some(Ok(engine.lifecycle.finished_generation()))
        }
    });
    let generation = match status {
        None => return false,
        Some(Err(error)) => {
            show_failure(app, &error);
            return true;
        }
        Some(Ok(generation)) => generation,
    };
    match crate::backend::service().app_preferences() {
        Ok(preferences) if preferences.close_to_tray => false,
        Ok(_) => {
            if let Some(generation) = generation {
                handle_event(
                    app,
                    Event {
                        generation,
                        outcome: Outcome::Finished,
                    },
                );
                // A newer game may have won the shutdown claim; park its window
                // instead of treating stale completion as an app-exit decision.
                !sessions.read(|engine| engine.has_live_game())
            } else {
                show_failure(app, "Game-session completion could not be confirmed. Keep Redunar open and try again.");
                true
            }
        }
        Err(_) => {
            show_failure(
                app,
                "Could not check Close to tray. Keep Redunar open and try again.",
            );
            true
        }
    }
}
