/// One app-wide production feature with a runtime capability boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppModule {
    FrameMetrics,
    InGameOverlay,
    InstantReplay,
}

/// Effective feature state derived from current runtime capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleStatus {
    Enabled,
    UnavailableOnSystem { reason: String },
    PlannedUnavailable { reason: String },
}

impl ModuleStatus {
    #[must_use]
    pub const fn allows_runtime(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleStatuses {
    pub frame_metrics: ModuleStatus,
    pub in_game_overlay: ModuleStatus,
    pub instant_replay: ModuleStatus,
}

impl ModuleStatuses {
    #[must_use]
    pub const fn get(&self, module: AppModule) -> &ModuleStatus {
        match module {
            AppModule::FrameMetrics => &self.frame_metrics,
            AppModule::InGameOverlay => &self.in_game_overlay,
            AppModule::InstantReplay => &self.instant_replay,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModuleCapability {
    Available,
    UnavailableOnSystem(String),
    PlannedUnavailable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleCapabilities {
    pub frame_metrics: ModuleCapability,
    pub in_game_overlay: ModuleCapability,
    pub instant_replay: ModuleCapability,
}

pub(crate) fn permanent_core_statuses(capabilities: &ModuleCapabilities) -> ModuleStatuses {
    ModuleStatuses {
        frame_metrics: resolve_one(&capabilities.frame_metrics),
        in_game_overlay: resolve_one(&capabilities.in_game_overlay),
        instant_replay: resolve_one(&capabilities.instant_replay),
    }
}

fn resolve_one(capability: &ModuleCapability) -> ModuleStatus {
    match capability {
        ModuleCapability::Available => ModuleStatus::Enabled,
        ModuleCapability::UnavailableOnSystem(reason) => ModuleStatus::UnavailableOnSystem {
            reason: reason.clone(),
        },
        ModuleCapability::PlannedUnavailable(reason) => ModuleStatus::PlannedUnavailable {
            reason: reason.clone(),
        },
    }
}
