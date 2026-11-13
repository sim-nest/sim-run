// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The installed binary loads roadmap through the boot selector.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(args)
        .output()
        .expect("run sim")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("utf8 stderr")
}

fn refuses_missing_identity(args: &[&str]) {
    let output = run(args);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{err}");
    assert!(err.contains("missing content identity --conduct"), "{err}");
    assert!(!err.contains("no loaded lib claims"), "{err}");
    assert!(!err.contains("range start index"), "{err}");
}

#[test]
fn bare_roadmap_reaches_the_identity_refusal() {
    refuses_missing_identity(&["roadmap"]);
}

#[test]
fn roadmap_status_reaches_the_identity_refusal() {
    refuses_missing_identity(&["roadmap", "status"]);
}

#[test]
fn roadmap_status_with_identity_dispatches() {
    let output = run(&[
        "roadmap",
        "status",
        "--conduct",
        "conduct/local",
        "--model-pick",
        "model/local",
        "--proof-catalog",
        "proof/local",
        "--runner-generation",
        "generation/local",
    ]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(!err.contains("no loaded lib claims"), "{err}");
    assert!(!err.contains("range start index"), "{err}");
}
