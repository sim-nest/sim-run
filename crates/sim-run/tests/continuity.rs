use std::{fs, process::Command};

// conformance: continuity plans degrade optional services and refuse absent required services.

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(args)
        .output()
        .expect("run sim continuity")
}

#[test]
fn help_and_dry_run_requirement_are_explicit() {
    let help = run(&["continuity", "--help"]);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("Usage: sim continuity")
    );

    let live = run(&["continuity"]);
    assert_eq!(live.status.code(), Some(2));
    assert!(
        String::from_utf8(live.stderr)
            .unwrap()
            .contains("requires --dry-run or --artifact-dir")
    );
}

#[test]
fn carrier_only_boots_networkless_with_satellites_absent() {
    let output = run(&[
        "continuity",
        "--dry-run",
        "--plan",
        "continuity/carrier-only",
        "--absent",
        "watch",
        "--absent",
        "desk-display",
        "--absent",
        "halo",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("plan=continuity/carrier-only"));
    assert!(stdout.contains("optional-absent: none"));
}

#[test]
fn missing_optional_degrades_and_missing_required_refuses() {
    let optional = run(&[
        "continuity",
        "--dry-run",
        "--plan",
        "continuity/walk",
        "--absent",
        "watch",
    ]);
    assert!(optional.status.success());
    assert!(
        String::from_utf8(optional.stdout)
            .unwrap()
            .contains("optional-absent: watch")
    );

    let required = run(&["continuity", "--dry-run", "--absent", "lifecycle"]);
    assert_eq!(required.status.code(), Some(2));
    assert!(
        String::from_utf8(required.stderr)
            .unwrap()
            .contains("required continuity service is absent: lifecycle")
    );
}

#[test]
fn android_and_host_modeled_artifacts_are_deterministic_and_unsigned() {
    let root = std::env::temp_dir().join(format!("sim-continuity-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for mode in ["android", "host-modeled"] {
        let first = run(&[
            "continuity",
            "--mode",
            mode,
            "--artifact-dir",
            root.to_str().unwrap(),
        ]);
        let second = run(&[
            "continuity",
            "--mode",
            mode,
            "--artifact-dir",
            root.to_str().unwrap(),
        ]);
        assert!(first.status.success());
        assert_eq!(first.stdout, second.stdout);
        assert!(
            String::from_utf8(first.stdout)
                .unwrap()
                .contains("unsigned")
        );
    }
    let android = fs::read_to_string(root.join("continuity.android.bundle")).unwrap();
    let host = fs::read_to_string(root.join("continuity.host-modeled.bundle")).unwrap();
    assert!(android.contains("mode=android\n"));
    assert!(host.contains("mode=host-modeled\n"));
    assert!(android.contains("unsigned=true"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shutdown_restart_and_replay_are_loaded_events() {
    for event in ["shutdown", "restart", "replay"] {
        let output = run(&["continuity", event, "--dry-run"]);
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(&format!("event={event}"))
        );
    }
}

#[test]
fn plan_file_changes_composition_without_bootloader_change() {
    let root = std::env::temp_dir().join(format!("sim-continuity-plan-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let plan = root.join("plans.toml");
    fs::write(&plan, "schema = \"sim.continuity-plans/v1\"\n[[plan]]\nid = \"continuity/carrier-only\"\nrevision = 9\nroot = \"android-root\"\nrequired = [\"phone-review\", \"lifecycle\", \"mounts\", \"capture\", \"render\", \"stop\", \"journal-append\"]\noptional_roles = []\n").unwrap();
    let output = run(&[
        "continuity",
        "--dry-run",
        "--plan-file",
        plan.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("revision=9")
    );
    fs::remove_dir_all(root).unwrap();
}
