use std::process::Command;

// conformance: compute command boots through the shared loader and loadable compute CLI.

#[test]
fn compute_devices_boots_modeled_and_auto_headless() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["compute", "devices", "--json", "--max-devices", "8"])
        .output()
        .expect("run sim compute devices");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"site\":\"site/compute/model\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"site\":\"site/compute/auto\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"installed\":true"), "{stdout}");
    assert!(
        stdout.contains("\"site\":\"site/compute/cuda\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"site\":\"site/compute/rocm\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"status\":\"no-adapter\""), "{stdout}");
}

#[test]
fn compute_profile_uses_headless_profile_store_policy() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["compute", "profile", "list", "--json"])
        .output()
        .expect("run sim compute profile list");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"no-profile-store\",\"keys\":[],\"decision\":null}\n"
    );
}

#[test]
fn explicit_loads_still_override_compute_default_sources() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["--load", "host:missing", "compute", "devices"])
        .output()
        .expect("run sim compute with explicit missing host");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("sim: unknown host library: missing")
    );
}
