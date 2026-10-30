//! Driver-independent checks of the production native bridge state machine.
use std::process::Command;

#[test]
fn native_bridge_errors_retain_unproven_work_and_allow_fresh_owners() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = std::env::temp_dir().join(format!("redunar-bridge-test-{}", std::process::id()));
    let status = Command::new("cc")
        .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(root.join("tools/fixtures/nvidia_memory_test.c"))
        .args(["-ldl", "-lpthread", "-o"])
        .arg(&binary)
        .status()
        .expect("native C regression compiler");
    assert!(status.success());
    let result = Command::new(&binary)
        .status()
        .expect("native regression executable");
    std::fs::remove_file(binary).expect("remove owned regression executable");
    assert!(result.success());
}
