// conformance: the installed binary reaches all read-only world operations.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(args)
        .output()
        .expect("run sim world")
}

#[test]
fn world_why_boots_without_an_external_codec() {
    let output = run(&[
        "world",
        "why",
        "conclusion/public-release",
        "policy/no-v3-disclosure",
    ]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("world/explanation-v1"));
    assert!(stdout.contains("owner/sim-private/disclosure"));
}

#[test]
fn world_project_and_diff_return_stable_records() {
    let project = run(&[
        "world",
        "project",
        "world/public-api-v1",
        "source/public-api",
        "pub fn project()",
    ]);
    assert!(project.status.success());
    assert!(project.stderr.is_empty());
    let projected = String::from_utf8(project.stdout).unwrap();
    assert!(projected.contains("world/projection-v1"));
    assert!(projected.contains("conclusion/source-api"));

    let diff = run(&[
        "world",
        "diff",
        "no-v3/disclosure-policy-v1",
        "policy/no-v3-disclosure",
        "internal",
        "public",
    ]);
    assert!(diff.status.success());
    assert!(diff.stderr.is_empty());
    let changed = String::from_utf8(diff.stdout).unwrap();
    assert!(changed.contains("world/diff-v1"));
    assert!(changed.contains("(changed true)"));
    assert!(changed.contains("conclusion/public-release"));
    assert!(!changed.contains("conclusion/source-api"));
}
