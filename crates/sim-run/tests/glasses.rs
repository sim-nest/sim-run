use std::process::Command;

// conformance: glasses command dry-runs modeled and hardware boot plans.

#[test]
fn glasses_both_dry_run_boots_co_use_plan() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["glasses", "--device", "both", "--pose", "auto", "--dry-run"])
        .output()
        .expect("run sim glasses dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "glasses: viture profile=luma-ultra tier=rich adapter=reprojector loop=60/120 stale=predict+clamp\n\
glasses: halo   profile=halo tier=actuator adapter=glance loop=5/30 stale=hold-last\n\
glasses: co-use hub ready; boot plan ready\n"
    );
}

#[test]
fn glasses_unverified_real_route_falls_back_to_modeled() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args([
            "glasses",
            "--device",
            "halo",
            "--route",
            "ble-direct",
            "--dry-run",
        ])
        .output()
        .expect("run sim glasses hardware dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("glasses: halo   route=ble-direct fallback=modeled reason=lane halo_ble_direct not verified"),
        "{stdout}"
    );
    assert!(stdout.contains("glasses: boot plan ready"), "{stdout}");
}

#[test]
fn glasses_auto_mirror_runs_headless_modeled() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["glasses", "--device", "auto", "--mirror", "--dry-run"])
        .output()
        .expect("run sim glasses mirror dry-run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "glasses: modeled profile=display-only tier=display adapter=mirror loop=1/1 stale=hold-last\n\
glasses: boot plan ready\n"
    );
}
