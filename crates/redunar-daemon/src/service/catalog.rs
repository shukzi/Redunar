use crate::{
    AddGameRequest, DiscoveredGame, GameCatalogError, GameDiscoveryError, RedunarService,
    ResolvedCatalogGame, game_catalog,
};
use redunar_platform::{DesktopGameDiscovery, SteamGameDiscovery};
use std::{env, path::PathBuf};

impl RedunarService {
    /// Load the bounded local, launcher-agnostic game catalog.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when stored state is unreadable or malformed.
    pub fn load_game_catalog(&self) -> Result<redunar_core::GameCatalog, GameCatalogError> {
        game_catalog::load(&self.state_directory)
    }

    /// Resolve the current global defaults and one game's explicit overrides.
    /// This is read-only and does not start capture or overlay work.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when stored state is invalid or the game
    /// no longer exists.
    pub fn effective_game_profile(
        &self,
        id: redunar_core::GameId,
    ) -> Result<redunar_core::EffectiveGameProfile, GameCatalogError> {
        let catalog = self.load_game_catalog()?;
        let game = catalog
            .games
            .iter()
            .find(|game| game.id == id)
            .ok_or_else(|| GameCatalogError::new("the selected game no longer exists"))?;
        Ok(game.profile.resolve(catalog.global_profile))
    }

    /// Resolve a detected process against local game identity and return an
    /// effective profile only for a unique, high-confidence match.
    ///
    /// Suggested filename matches and ambiguous identities intentionally carry
    /// no effective profile, so detection alone cannot activate a session feature.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when stored state is invalid.
    pub fn resolve_game_process(
        &self,
        process: &redunar_core::GameProcess,
    ) -> Result<ResolvedCatalogGame, GameCatalogError> {
        let mut resolved = game_catalog::resolve_process(&self.state_directory, process)?;
        if let redunar_core::GameResolution::Automatic(id) = resolved.resolution {
            resolved.effective_profile = Some(self.effective_game_profile(id)?);
        }
        Ok(resolved)
    }

    /// Add one direct executable to the local game catalog.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] for invalid, duplicate, or unsavable records.
    pub fn add_game(
        &self,
        request: AddGameRequest,
    ) -> Result<redunar_core::GameRecord, GameCatalogError> {
        game_catalog::add(&self.state_directory, request)
    }

    /// Atomically import a bounded set of direct executable records.
    ///
    /// Every entry is validated and canonicalized before the catalog is
    /// changed. A duplicate or invalid entry rejects the whole batch.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] for an empty or oversized batch, invalid
    /// launch data, duplicates, catalog capacity, or persistence failure.
    pub fn import_games(
        &self,
        requests: Vec<AddGameRequest>,
    ) -> Result<Vec<redunar_core::GameRecord>, GameCatalogError> {
        game_catalog::import(&self.state_directory, requests)
    }

    /// Discover local Steam installs and direct native XDG game entries.
    /// Discovery is bounded and read-only; it does not add, launch, or tune a
    /// game. Shared desktop launchers are left to their dedicated adapters so
    /// they cannot become an ambiguous process identity.
    ///
    /// # Errors
    ///
    /// Returns [`GameDiscoveryError`] when the local home or an existing Steam
    /// library cannot be inspected safely.
    pub fn discover_installed_games(&self) -> Result<Vec<DiscoveredGame>, GameDiscoveryError> {
        let home = env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            GameDiscoveryError::new("HOME is unavailable for local game discovery")
        })?;
        let mut discovered = SteamGameDiscovery::new(home.clone()).discover()?;
        discovered.extend(DesktopGameDiscovery::from_environment(&home).discover()?);
        Ok(discovered)
    }

    /// Atomically add candidates which have a verified shell-free launch.
    /// Shared launchers are not used as process identity; their explicit Steam
    /// app identifiers remain the canonical matching evidence.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] if any candidate is not launchable, has
    /// invalid identity, duplicates a saved game, or cannot be persisted.
    pub fn import_discovered_games(
        &self,
        candidates: Vec<DiscoveredGame>,
    ) -> Result<Vec<redunar_core::GameRecord>, GameCatalogError> {
        let requests = candidates
            .into_iter()
            .map(|candidate| {
                let launch = candidate.launch.ok_or_else(|| {
                    GameCatalogError::new(
                        "a discovered game has no verified shell-free launch on this system",
                    )
                })?;
                Ok(AddGameRequest::external_launcher(
                    candidate.display_name,
                    launch,
                    candidate.match_rules,
                ))
            })
            .collect::<Result<Vec<_>, GameCatalogError>>()?;
        game_catalog::import(&self.state_directory, requests)
    }

    /// Atomically refresh saved launch commands found again by local
    /// discovery. Only a unique strong identity such as a Steam app ID may
    /// update a record; profiles and display names are preserved.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when a discovered launch is invalid or
    /// the updated catalog cannot be persisted safely.
    pub fn refresh_discovered_game_launches(
        &self,
        candidates: &[DiscoveredGame],
    ) -> Result<Vec<redunar_core::GameRecord>, GameCatalogError> {
        let requests = candidates
            .iter()
            .filter_map(|candidate| {
                candidate.launch.clone().map(|launch| {
                    AddGameRequest::external_launcher(
                        candidate.display_name.clone(),
                        launch,
                        candidate.match_rules.clone(),
                    )
                })
            })
            .collect();
        game_catalog::refresh_discovered_launches(&self.state_directory, requests)
    }

    /// Build an import candidate from an already conservatively detected game
    /// process only when its exact executable is a unique host executable.
    ///
    /// # Errors
    ///
    /// Returns [`GameDiscoveryError`] for stale, non-executable, shared
    /// launcher, Wine, or Proton process identities.
    pub fn candidate_from_game_process(
        &self,
        process: &redunar_core::GameProcess,
    ) -> Result<DiscoveredGame, GameDiscoveryError> {
        redunar_platform::candidate_from_process(process)
    }

    /// Replace one game's shell-free launch configuration.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when the game or launch data is invalid.
    pub fn update_game_launch(
        &self,
        id: redunar_core::GameId,
        launch: redunar_core::GameLaunchConfig,
    ) -> Result<redunar_core::GameRecord, GameCatalogError> {
        game_catalog::update_launch(&self.state_directory, id, launch)
    }

    /// Persist explicit per-game inheritance or overrides. This records user
    /// intent only and does not change an active capture session by itself.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when the game is missing or state cannot be saved.
    pub fn update_game_profile(
        &self,
        id: redunar_core::GameId,
        profile: redunar_core::PerGameProfile,
    ) -> Result<redunar_core::GameRecord, GameCatalogError> {
        game_catalog::update_profile(&self.state_directory, id, profile)
    }

    /// Remove one user-owned local game record.
    ///
    /// # Errors
    ///
    /// Returns [`GameCatalogError`] when the game is missing or state cannot be saved.
    pub fn remove_game(
        &self,
        id: redunar_core::GameId,
    ) -> Result<redunar_core::GameRecord, GameCatalogError> {
        game_catalog::remove(&self.state_directory, id)
    }
}
