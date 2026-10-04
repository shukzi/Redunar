use super::tests::{Fixture, amd_fixture};
use super::*;
use std::os::unix::fs::symlink;

fn render_card(fixture: &Fixture, card: &str, render: &str, vendor: &str) {
    fixture.write(&format!("sys/class/drm/{card}/device/vendor"), vendor);
    fixture.write(&format!("sys/class/drm/{card}/device/device"), "0x1234\n");
    fixture.write(
        &format!("sys/class/drm/{card}/device/drm/{render}/dev"),
        "226:128\n",
    );
    let root = fixture.probe().sys_root.join("class/drm");
    fs::create_dir_all(root.join(render)).expect("render fixture");
    symlink(
        root.join(card).join("device"),
        root.join(render).join("device"),
    )
    .expect("render device link");
}

fn nvidia_only() -> Fixture {
    let fixture = amd_fixture();
    fs::remove_dir_all(fixture.probe().sys_root.join("class/drm/card1")).expect("remove AMD");
    render_card(&fixture, "card1", "renderD128", "0x10de\n");
    // Deliberately no PCI identity: sampler tests remain completely isolated
    // from the host NVML library while covering its diagnostic boundary.
    fixture
}

#[test]
fn nvidia_only_beta_retains_identity_with_unavailable_metrics() {
    let fixture = nvidia_only();
    let (snapshot, readiness) = fixture
        .probe()
        .with_nvidia_beta_enabled(true)
        .snapshot_with_nvidia_readiness()
        .expect("probe");
    assert_eq!(readiness, NvidiaReadiness::Ready);
    assert_eq!(snapshot.gpus.len(), 1);
    assert!(is_nvidia(&snapshot.gpus[0]));
    assert_eq!(snapshot.gpus[0].temperature_celsius, None);
    assert_eq!(snapshot.gpus[0].utilization_percent, None);
    assert_eq!(snapshot.gpus[0].clock_mhz, None);
    assert_eq!(snapshot.gpus[0].vram_used_bytes, None);
    assert_eq!(snapshot.gpus[0].vram_total_bytes, None);
    assert_eq!(snapshot.gpus[0].power_watts, None);
    let probe = fixture.probe();
    let mut sampler =
        LinuxTelemetrySampler::with_nvidia_beta(probe.proc_root, probe.sys_root, true)
            .expect("sampler");
    assert_eq!(
        sampler.nvidia_diagnostics().failure,
        Some(redunar_nvidia_nvml::NvmlFailure::PciIdentityUnavailable)
    );
    assert_eq!(sampler.sample().gpus[0].vram_total_bytes, None);
}

#[test]
fn nvidia_only_beta_off_never_admits_nvidia() {
    let fixture = nvidia_only();
    let (snapshot, readiness) = fixture
        .probe()
        .snapshot_with_nvidia_readiness()
        .expect("probe");
    assert_eq!(readiness, NvidiaReadiness::BetaDisabled);
    assert!(snapshot.gpus.is_empty());
    let probe = fixture.probe();
    let sampler = LinuxTelemetrySampler::new(probe.proc_root, probe.sys_root).expect("sampler");
    assert_eq!(
        sampler.nvidia_diagnostics().readiness,
        NvidiaReadiness::BetaDisabled
    );
}

#[test]
fn display_only_cards_and_connectors_do_not_withhold_single_nvidia_render_gpu() {
    let fixture = nvidia_only();
    fixture.write(
        "sys/class/drm/card0/device/uevent",
        "DRIVER=simple-framebuffer\n",
    );
    fixture.write("sys/class/drm/card2/device/vendor", "0x8086\n");
    fixture.write("sys/class/drm/card1-HDMI-A-1/device/vendor", "0x8086\n");
    let (snapshot, readiness) = fixture
        .probe()
        .with_nvidia_beta_enabled(true)
        .snapshot_with_nvidia_readiness()
        .expect("probe");
    assert_eq!(readiness, NvidiaReadiness::Ready);
    assert_eq!(snapshot.gpus.len(), 1);
    assert_eq!(snapshot.gpus[0].card, "card1");
}

#[test]
fn one_nvidia_alongside_amd_preserves_both_identities_and_amd_metrics() {
    let fixture = amd_fixture();
    render_card(&fixture, "card1", "renderD128", "0x1002\n");
    render_card(&fixture, "card2", "renderD129", "0x10de\n");
    for beta in [false, true] {
        let probe = fixture.probe().with_nvidia_beta_enabled(beta);
        let (snapshot, readiness) = probe.snapshot_with_nvidia_readiness().expect("probe");
        assert_eq!(
            readiness,
            if beta {
                NvidiaReadiness::Ready
            } else {
                NvidiaReadiness::BetaDisabled
            }
        );
        assert_eq!(snapshot.gpus.len(), if beta { 2 } else { 1 });
        assert_eq!(snapshot.gpus[0].card, "card1");
        assert_eq!(snapshot.gpus[0].utilization_percent, Some(96.0));
        assert_eq!(snapshot.gpus[0].temperature_celsius, Some(62.0));
        let mut sampler =
            LinuxTelemetrySampler::with_nvidia_beta(probe.proc_root, probe.sys_root, beta)
                .expect("sampler");
        assert_eq!(sampler.sample().gpus[0].power_watts, Some(210.0));
        // The fixture has no PCI identity and must never load host NVML.
        assert_eq!(
            sampler.nvidia_diagnostics().readiness,
            if beta {
                NvidiaReadiness::Unavailable
            } else {
                NvidiaReadiness::BetaDisabled
            }
        );
        if beta {
            assert_eq!(
                sampler.nvidia_diagnostics().failure,
                Some(redunar_nvidia_nvml::NvmlFailure::PciIdentityUnavailable)
            );
        }
    }
}

#[test]
fn intel_hybrid_is_admitted_but_multiple_nvidia_gpus_are_withheld() {
    for vendor in ["0x8086\n", "0x10de\n"] {
        let fixture = nvidia_only();
        render_card(&fixture, "card2", "renderD129", vendor);
        let (snapshot, readiness) = fixture
            .probe()
            .with_nvidia_beta_enabled(true)
            .snapshot_with_nvidia_readiness()
            .expect("probe");
        if vendor == "0x8086\n" {
            assert_eq!(readiness, NvidiaReadiness::Ready);
            assert_eq!(snapshot.gpus.len(), 1);
        } else {
            assert_eq!(readiness, NvidiaReadiness::AmbiguousTopology);
            assert!(snapshot.gpus.is_empty());
        }
    }
}

#[test]
fn missing_render_device_and_unidentified_render_node_fail_closed() {
    let fixture = nvidia_only();
    let root = fixture.probe().sys_root.join("class/drm");
    fs::remove_dir_all(root.join("renderD128")).expect("remove render class");
    fs::remove_dir_all(root.join("card1/device/drm")).expect("remove device render");
    let (snapshot, readiness) = fixture
        .probe()
        .with_nvidia_beta_enabled(true)
        .snapshot_with_nvidia_readiness()
        .expect("probe");
    assert_eq!(readiness, NvidiaReadiness::NoRenderDevice);
    assert!(snapshot.gpus.is_empty());

    fixture.write("sys/class/drm/card1/device/drm/renderD128/dev", "226:128\n");
    // The global render link can be missing in a restricted sysfs view;
    // per-device DRM still proves this is a render GPU.
    assert_eq!(
        fixture
            .probe()
            .with_nvidia_beta_enabled(true)
            .snapshot_with_nvidia_readiness()
            .expect("probe")
            .1,
        NvidiaReadiness::Ready
    );
    fixture.write("sys/class/drm/renderD129/dev", "226:129\n");
    let (snapshot, readiness) = fixture
        .probe()
        .with_nvidia_beta_enabled(true)
        .snapshot_with_nvidia_readiness()
        .expect("probe");
    assert_eq!(readiness, NvidiaReadiness::AmbiguousTopology);
    assert!(snapshot.gpus.is_empty());
}

#[test]
fn multiple_drm_cards_for_one_physical_render_gpu_are_deduplicated() {
    let fixture = nvidia_only();
    let root = fixture.probe().sys_root.join("class/drm");
    fs::create_dir_all(root.join("card3")).expect("duplicate card");
    symlink(root.join("card1/device"), root.join("card3/device")).expect("duplicate device link");
    let topology = drm::DrmTopology::discover(&fixture.probe().sys_root);
    assert_eq!(topology.nvidia_readiness(true), NvidiaReadiness::Ready);
    assert_eq!(
        fixture
            .probe()
            .with_nvidia_beta_enabled(true)
            .snapshot()
            .expect("probe")
            .gpus
            .len(),
        1
    );
}

#[test]
fn inaccessible_render_identity_and_failed_topology_scan_do_not_enable_nvidia() {
    let fixture = nvidia_only();
    let root = fixture.probe().sys_root.join("class/drm");
    // The broken link models a visible node whose target cannot be inspected.
    // Enumeration must keep counting it instead of dropping it via is_dir().
    symlink(root.join("missing-render-device"), root.join("renderD129"))
        .expect("uninspectable render");
    let probe = fixture.probe().with_nvidia_beta_enabled(true);
    assert_eq!(
        probe.snapshot_with_nvidia_readiness().expect("probe").1,
        NvidiaReadiness::AmbiguousTopology
    );
    fs::remove_file(root.join("renderD129")).expect("remove unknown node");
    // A regular file in place of an unreadable DRM directory produces the
    // same failed-read behavior regardless of the test runner's privileges.
    fs::remove_dir_all(root.join("card1/device/drm")).expect("remove device DRM");
    fs::write(root.join("card1/device/drm"), []).expect("failed directory read");
    assert_eq!(
        probe.snapshot_with_nvidia_readiness().expect("probe").1,
        NvidiaReadiness::Unavailable
    );
}

#[test]
fn sampler_shutdown_reports_final_nvml_state_without_loading_hardware() {
    let fixture = nvidia_only();
    let probe = fixture.probe();
    let mut sampler =
        LinuxTelemetrySampler::with_nvidia_beta(probe.proc_root, probe.sys_root, true)
            .expect("sampler");
    assert_eq!(
        sampler.shutdown_nvidia().readiness,
        NvidiaReadiness::Stopped
    );
    assert_eq!(
        sampler.shutdown_nvidia().readiness,
        NvidiaReadiness::Stopped
    );
    let snapshot = sampler.sample();
    assert_eq!(snapshot.cpu.temperature_celsius, Some(42.25));
    assert_eq!(snapshot.gpus[0].utilization_percent, None);
    assert_eq!(sampler.nvidia_diagnostics().retry_delay, None);
}

#[test]
fn game_vendor_selection_is_independent_of_card_and_render_numbering() {
    for (nvidia_card, nvidia_render, intel_card, intel_render) in [
        ("card0", "renderD128", "card1", "renderD129"),
        ("card1", "renderD129", "card0", "renderD128"),
    ] {
        let fixture = amd_fixture();
        fs::remove_dir_all(fixture.probe().sys_root.join("class/drm/card1")).unwrap();
        render_card(&fixture, nvidia_card, nvidia_render, "0x10de\n");
        render_card(&fixture, intel_card, intel_render, "0x8086\n");
        let selected = unique_render_device(&fixture.probe().sys_root, 0x10de).unwrap();
        assert_eq!(selected.card, nvidia_card);
        assert_eq!(
            format!("renderD{}", selected.render_node_index),
            nvidia_render
        );
        assert_eq!(
            unique_render_device(&fixture.probe().sys_root, 0x8086)
                .unwrap()
                .card,
            intel_card
        );
        assert_eq!(
            fixture
                .probe()
                .with_nvidia_beta_enabled(true)
                .snapshot_with_nvidia_readiness()
                .unwrap()
                .1,
            NvidiaReadiness::Ready
        );
    }
}

#[test]
fn game_vendor_selection_withholds_duplicate_physical_gpus_and_unknown_devices() {
    let fixture = nvidia_only();
    render_card(&fixture, "card2", "renderD129", "0x10de\n");
    assert!(unique_render_device(&fixture.probe().sys_root, 0x10de).is_none());
    fs::remove_dir_all(fixture.probe().sys_root.join("class/drm/card2")).unwrap();
    assert!(unique_render_device(&fixture.probe().sys_root, 0x10de).is_none());
}
