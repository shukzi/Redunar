use crate::{KmsOutput, discover_outputs_at};
use std::env;
use std::path::Path;

const DIAGNOSTIC_ENV: &str = "REDUNAR_KMS_REPLAY_DIAGNOSTIC";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KmsProbeBlocker {
    DiagnosticNotRequested,
    DiscoveryFailed,
    NoConnectedOutput,
    PrivilegedHelperNotIntegrated,
    FramebufferExportNotIntegrated,
    EncoderSurfaceInteropNotIntegrated,
    SustainedFramePacingNotVerified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KmsProbe {
    pub outputs: Vec<KmsOutput>,
    pub blockers: Vec<KmsProbeBlocker>,
}

impl KmsProbe {
    #[must_use]
    pub fn local() -> Self {
        Self::at(Path::new("/"), diagnostic_requested())
    }

    #[must_use]
    pub fn at(root: &Path, requested: bool) -> Self {
        if !requested {
            return Self {
                outputs: Vec::new(),
                blockers: vec![KmsProbeBlocker::DiagnosticNotRequested],
            };
        }
        let mut blockers = Vec::new();
        let outputs = if let Ok(outputs) = discover_outputs_at(root) {
            outputs
        } else {
            blockers.push(KmsProbeBlocker::DiscoveryFailed);
            Vec::new()
        };
        if outputs.is_empty() {
            blockers.push(KmsProbeBlocker::NoConnectedOutput);
        }
        blockers.push(KmsProbeBlocker::SustainedFramePacingNotVerified);
        Self { outputs, blockers }
    }

    #[must_use]
    pub fn production_ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

#[must_use]
pub fn diagnostic_requested() -> bool {
    env::var(DIAGNOSTIC_ENV).ok().as_deref() == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("redunar-kms-probe-{}-{nonce}", std::process::id()));
        let connector = root.join("sys/class/drm/card0-HDMI-A-1");
        fs::create_dir_all(&connector).expect("connector");
        fs::write(connector.join("status"), "connected\n").expect("status");
        fs::write(connector.join("modes"), "1920x1080\n").expect("modes");
        root
    }

    #[test]
    fn probe_is_explicit_and_never_claims_production_readiness() {
        let root = fixture();
        let disabled = KmsProbe::at(&root, false);
        assert_eq!(
            disabled.blockers,
            vec![KmsProbeBlocker::DiagnosticNotRequested]
        );

        let enabled = KmsProbe::at(&root, true);
        assert_eq!(enabled.outputs.len(), 1);
        assert!(!enabled.production_ready());
        assert_eq!(
            enabled.blockers,
            vec![KmsProbeBlocker::SustainedFramePacingNotVerified]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
