use std::process::Command;

#[test]
fn watch_modeled_dry_run_boots_headless() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args([
            "watch",
            "--profile",
            "watch-glance",
            "--source",
            "modeled",
            "--fleet",
            "one",
            "--dry-run",
        ])
        .output()
        .expect("run sim watch dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "watch: profile=watch-glance tier=sensor+actuator source=modeled route=modeled\n\
watch: rate=1/1hz stale=hold-last glance-adapter=configured(device)\n\
watch: boot plan ready\n"
    );
}

#[test]
fn watch_import_dry_run_defaults_to_import_route() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["watch", "--source", "import", "worn.jsonl", "--dry-run"])
        .output()
        .expect("run sim watch import dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("source=import route=import"), "{stdout}");
    assert!(stdout.contains("watch: boot plan ready"), "{stdout}");
}

#[test]
fn watch_live_dry_run_names_unavailable_lane() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["watch", "--source", "live", "--route", "ble", "--dry-run"])
        .output()
        .expect("run sim watch live dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("route=ble reason=live provider is not installed in this build"),
        "{stdout}"
    );
    assert!(
        stdout.contains("source=live reason=--consent is required"),
        "{stdout}"
    );
}
