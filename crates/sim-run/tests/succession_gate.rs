// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: the installed binary loads succeed and fresh. An independent
// change closes with no second-pass work and stays unsealed. The construction
// specimen stays a fresh gap.

use std::process::{Command, Output};

fn export(project: &str) -> String {
    format!(
        "\
export world/succession-export-v1
package sim-incremental-core 0.5.2 registry
package sim-storage-port 0.2.2 registry
package sim-lib-world 0.1.1 registry
project {project}
input provider/widget observed widget-body
input remote/hostile unavailable -
"
    )
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(args)
        .output()
        .expect("run sim")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf8 stdout")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("utf8 stderr")
}

fn one_line(output: &Output) -> String {
    let err = stderr(output);
    let out = stdout(output);
    assert!(output.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(err.is_empty(), "{err}");
    assert!(!out.contains("no loaded lib claims"), "{out}");
    assert!(!out.contains("PortableClosed"), "{out}");
    assert!(!out.contains("derive_completion"), "{out}");
    let line = out.trim_end();
    assert!(!line.contains('\n'), "{out}");
    line.to_owned()
}

#[test]
fn installed_succeed_closes_an_independent_change_and_repeats() {
    let export = export("harbor");
    let change = "independent harbor provider/marker observed marker-next";
    let first = one_line(&run(&["world", "succeed", &export, change]));
    let second = one_line(&run(&["world", "succeed", &export, change]));
    assert_eq!(first, second);
    assert!(first.starts_with("#(world/succession-v1 "));
    assert!(first.contains("(change \"independent\")"));
    assert!(first.contains("(closed \"portable\")"));
    assert!(first.contains("(project \"harbor\")"));
    assert!(first.contains("(linux \"absent\")"));
    assert!(first.contains("(converge-second 0)"));
    assert!(first.contains("(deliver-second 0)"));
    assert!(first.contains("(effects 0)"));
    assert!(first.contains("(subprocesses 0)"));
    assert!(first.contains("(resident true)"));
}

#[test]
fn installed_succeed_refuses_a_construction_fixture_change() {
    let line = one_line(&run(&[
        "world",
        "succeed",
        &export("lantern"),
        "lantern lantern provider/shade observed shade-next",
    ]));
    assert_eq!(
        line,
        "#(world/succession-gap-v1 (closed \"open\") (effects 0) (reason \"fixture-change\") (resident true) (subprocesses 0))"
    );
}

#[test]
fn installed_fresh_admits_without_a_completion_seal() {
    let line = one_line(&run(&[
        "world",
        "fresh",
        &export("harbor"),
        "independent harbor provider/marker observed marker-next",
    ]));
    assert_eq!(
        line,
        "#(world/fresh-v1 (change \"independent\") (converge-second 0) (deliver-second 0) (effects 0) (linux \"absent\") (project \"harbor\") (resident true) (seal \"unclaimed\") (subprocesses 0) (succession \"portable\"))"
    );
}

#[test]
fn installed_fresh_keeps_the_construction_specimen_open() {
    let closed = one_line(&run(&[
        "world",
        "succeed",
        &export("lantern"),
        "shade-1 lantern provider/shade observed shade-next",
    ]));
    assert!(closed.contains("(change \"shade-1\")"));
    assert!(closed.contains("(closed \"portable\")"));
    let fresh = one_line(&run(&[
        "world",
        "fresh",
        &export("lantern"),
        "shade-1 lantern provider/shade observed shade-next",
    ]));
    assert_eq!(
        fresh,
        "#(world/fresh-gap-v1 (effects 0) (reason \"construction-specimen\") (resident true) (seal \"unclaimed\") (subprocesses 0))"
    );
}

#[test]
fn installed_fresh_refuses_warm_state_and_a_linux_claim() {
    let warm = one_line(&run(&[
        "world",
        "fresh",
        &format!("{}checkout\n", export("harbor")),
        "independent harbor provider/marker observed marker-next",
    ]));
    assert_eq!(
        warm,
        "#(world/fresh-gap-v1 (effects 0) (reason \"warm-state\") (resident true) (seal \"unclaimed\") (subprocesses 0))"
    );
    let linux = one_line(&run(&[
        "world",
        "fresh",
        &format!("{}claim reboot\n", export("harbor")),
        "independent harbor provider/marker observed marker-next",
    ]));
    assert_eq!(
        linux,
        "#(world/fresh-gap-v1 (effects 0) (reason \"linux-claim\") (resident true) (seal \"unclaimed\") (subprocesses 0))"
    );
}
