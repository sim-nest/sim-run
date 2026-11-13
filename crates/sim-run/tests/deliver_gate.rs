// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The installed binary loads deliver through the world command.

use std::process::{Command, Output};

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

fn exactly_one_line(text: &str) -> &str {
    let Some(line) = text.strip_suffix('\n') else {
        panic!("missing trailing newline:\n{text}");
    };
    assert!(!line.contains('\n'), "{text}");
    line
}

fn one_line(output: &Output) -> String {
    let err = stderr(output);
    let out = stdout(output);
    assert!(output.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(err.is_empty(), "{err}");
    assert!(!out.contains("no loaded lib claims"), "{out}");
    assert!(!out.contains("PortableClosed"), "{out}");
    assert!(!out.contains("derive_completion"), "{out}");
    exactly_one_line(&out).to_owned()
}

fn quiet_catalog(realization: &str) -> String {
    let first = one_line(&run(&["world", "deliver", realization]));
    let second = one_line(&run(&["world", "deliver", realization]));
    assert_eq!(first, second, "{realization}");
    assert!(first.starts_with("#(world/deliver-v1 "), "{first}");
    assert!(
        first.contains(&format!("(realization \"{realization}\")")),
        "{first}"
    );
    assert!(first.contains("(effects 0)"), "{first}");
    assert!(first.contains("(subprocesses 0)"), "{first}");
    assert!(first.contains("(resident true)"), "{first}");
    assert!(first.contains("(recovered true)"), "{first}");
    assert!(first.contains("(disclosure "), "{first}");
    assert!(!first.contains("private-material:"), "{first}");
    assert!(!first.contains("/home/"), "{first}");
    first
}

#[test]
fn installed_deliver_repeats_the_model_catalog() {
    let line = quiet_catalog("model");
    assert!(line.contains("model/"), "{line}");
}

#[test]
fn installed_deliver_keeps_native_and_real_unavailable() {
    for realization in ["native", "real"] {
        let line = quiet_catalog(realization);
        assert!(
            line.contains("unavailable-receiver"),
            "{realization}: {line}"
        );
        assert!(!line.contains("model/"), "{realization}: {line}");
    }
}

#[test]
fn installed_deliver_refuses_a_private_realization() {
    let line = one_line(&run(&["world", "deliver", "private-material:s3cret-token"]));
    assert_eq!(
        line,
        "#(world/deliver-gap-v1 (effects 0) (realization \"\") (reason \"private-leakage\") (resident true) (selector \"\") (subprocesses 0) (view \"sim-world-delivery-view-v1 cbeb8e5c1eabf7ec6a7de60bc3bc70661a23ef077ba11710acd737faa1be4acf #(world/deliver-gap-v1 (effects 0) (realization \\\"\\\") (reason \\\"private-leakage\\\") (resident true) (selector \\\"\\\") (subprocesses 0))\"))"
    );
    assert!(!line.contains("s3cret-token"), "{line}");
    assert!(!line.contains("private-material:"), "{line}");
}

#[test]
fn installed_deliver_refuses_an_unknown_realization() {
    let line = one_line(&run(&["world", "deliver", "host"]));
    assert_eq!(
        line,
        "#(world/deliver-gap-v1 (effects 0) (realization \"host\") (reason \"unknown-realization\") (resident true) (selector \"\") (subprocesses 0) (view \"sim-world-delivery-view-v1 df86cb639b324a619794705dac4a153450a3fd301cb3be45f0f692cc8c02a1f4 #(world/deliver-gap-v1 (effects 0) (realization \\\"host\\\") (reason \\\"unknown-realization\\\") (resident true) (selector \\\"\\\") (subprocesses 0))\"))"
    );
}

#[test]
fn installed_deliver_why_hides_a_private_selector() {
    let line = one_line(&run(&[
        "world",
        "deliver-why",
        "model",
        "/home/private/secret-path",
    ]));
    assert_eq!(
        line,
        "#(world/deliver-gap-v1 (effects 0) (realization \"model\") (reason \"private-leakage\") (resident true) (selector \"\") (subprocesses 0))"
    );
    assert!(!line.contains("/home/"), "{line}");
}

#[test]
fn installed_deliver_status_asks_for_a_refresh() {
    let line = one_line(&run(&["world", "deliver-status", "model"]));
    assert_eq!(
        line,
        "#(world/deliver-gap-v1 (effects 0) (realization \"model\") (reason \"refresh-needed\") (resident false) (selector \"\") (subprocesses 0))"
    );
}

#[test]
fn installed_deliver_usage_stays_on_the_world_command() {
    let output = run(&["world", "deliver"]);
    let err = stderr(&output);
    let out = stdout(&output);
    assert_eq!(output.status.code(), Some(2), "{err}");
    assert_eq!(
        exactly_one_line(&out),
        "#(world/refusal-v1 (call \"usage\") (reason \"usage\"))"
    );
    assert!(err.contains("deliver REALIZATION"), "{err}");
    assert!(!err.contains("no loaded lib claims"), "{err}");
    assert!(!out.contains("world/deliver-v1 "), "{out}");
}
