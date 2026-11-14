use crate::{AppPreferences, AppPreferencesError, RedunarService, app_preferences};

impl RedunarService {
    /// Load daemon-owned application behavior. Missing state keeps normal
    /// desktop close semantics and performs no write.
    ///
    /// # Errors
    ///
    /// Returns [`AppPreferencesError`] for unsafe, malformed, or unreadable
    /// local state.
    pub fn app_preferences(&self) -> Result<AppPreferences, AppPreferencesError> {
        app_preferences::load(&self.state_directory)
    }

    /// Persist close-to-tray intent atomically without claiming that a desktop
    /// tray host is currently available.
    ///
    /// # Errors
    ///
    /// Returns [`AppPreferencesError`] if private local state cannot be saved.
    pub fn set_close_to_tray_enabled(
        &self,
        enabled: bool,
    ) -> Result<AppPreferences, AppPreferencesError> {
        app_preferences::set_close_to_tray(&self.state_directory, enabled)
    }

    /// Persist whether Redunar should check signed release metadata
    /// automatically. Installation remains a separate user-confirmed action.
    ///
    /// # Errors
    ///
    /// Returns [`AppPreferencesError`] if private local state cannot be saved.
    pub fn set_automatic_updates_enabled(
        &self,
        enabled: bool,
    ) -> Result<AppPreferences, AppPreferencesError> {
        app_preferences::set_automatic_updates(&self.state_directory, enabled)
    }

    /// Persist the opt-in diagnostic logging switch. Enabling takes effect on
    /// the next app start; the running process keeps its current stream.
    ///
    /// # Errors
    ///
    /// Propagates preference persistence failures.
    pub fn set_diagnostic_log_enabled(
        &self,
        enabled: bool,
    ) -> Result<AppPreferences, AppPreferencesError> {
        app_preferences::set_diagnostic_log(&self.state_directory, enabled)
    }

    /// Persist the last usable window size and maximized state.
    ///
    /// # Errors
    ///
    /// Returns [`AppPreferencesError`] when the dimensions are unsafe or the
    /// private preference file cannot be updated atomically.
    pub fn set_window_state(
        &self,
        width: i32,
        height: i32,
        maximized: bool,
    ) -> Result<AppPreferences, AppPreferencesError> {
        app_preferences::set_window_state(&self.state_directory, width, height, maximized)
    }
}
