// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: the installed binary loads prove and keeps other verbs on their own paths.

use std::process::{Command, Output};

fn load_receipt() -> String {
    let mut text = String::from("#(prove/load-receipt-v1 (provider \"");
    text.push_str("platform/site/model");
    text.push_str("\") (loader \"");
    text.push_str("loader/static-v1");
    text.push_str("\") (artifact \"");
    text.push_str("lib/prove-command");
    text.push_str("\"))");
    text
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

fn record_and_receipt(text: &str) -> (&str, &str) {
    let mut lines = text.lines();
    let record = lines.next().expect("record");
    let receipt = lines.next().expect("receipt");
    assert!(lines.next().is_none(), "extra prove output: {text}");
    (record, receipt)
}

fn assert_quiet_receipt(output: &Output) -> String {
    assert!(output.stderr.is_empty(), "{}", stderr(output));
    assert_record(output)
}

fn assert_record(output: &Output) -> String {
    assert!(output.status.success(), "stderr: {}", stderr(output));
    assert!(output.stderr.is_empty(), "{}", stderr(output));
    let text = stdout(output);
    let (record, receipt) = record_and_receipt(&text);
    assert_eq!(receipt, load_receipt());
    assert!(!record.contains("PortableClosed"), "{record}");
    assert!(!record.contains("derive_completion"), "{record}");
    record.to_owned()
}

fn field(record: &str, name: &str) -> String {
    let key = format!("({name} ");
    let start = record
        .find(&key)
        .unwrap_or_else(|| panic!("missing {name} in {record}"))
        + key.len();
    let rest = &record[start..];
    let end = rest
        .find([' ', ')'])
        .unwrap_or_else(|| panic!("unterminated {name}"));
    rest[..end].to_owned()
}

fn view_of(record: &str) -> String {
    const KEY: &str = "(view \"";
    let start = record.find(KEY).expect("view field") + KEY.len();
    let mut view = String::new();
    let mut chars = record[start..].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            view.push(chars.next().expect("escaped view character"));
            continue;
        }
        if ch == '"' {
            return view;
        }
        view.push(ch);
    }
    panic!("view string did not close");
}

#[test]
fn prove_frontier_without_a_view_asks_for_refresh() {
    let first = assert_quiet_receipt(&run(&["prove", "frontier"]));
    let second = assert_quiet_receipt(&run(&["prove", "frontier"]));
    assert_eq!(first, second);
    assert_eq!(
        first,
        "#(prove/gap-v1 (call \"frontier\") (reason \"refresh-needed\") (effects 0) (subprocesses 0))"
    );
}

#[test]
fn prove_names_excluded_and_unknown_leaves() {
    let native = assert_quiet_receipt(&run(&["prove", "frontier", "leaf/native-worker-adapter"]));
    assert_eq!(
        native,
        "#(prove/gap-v1 (call \"leaf/native-worker-adapter\") (reason \"excluded-scope\") (effects 0) (subprocesses 0))"
    );
    let assay = assert_quiet_receipt(&run(&["prove", "frontier", "leaf/native-assay"]));
    assert_eq!(
        assay,
        "#(prove/gap-v1 (call \"leaf/native-assay\") (reason \"excluded-scope\") (effects 0) (subprocesses 0))"
    );
    let missing = assert_quiet_receipt(&run(&["prove", "frontier", "leaf/not-in-the-union"]));
    assert_eq!(
        missing,
        "#(prove/gap-v1 (call \"leaf/not-in-the-union\") (reason \"unavailable-proof\") (effects 0) (subprocesses 0))"
    );
}

#[test]
fn prove_usage_is_a_prove_refusal() {
    let output = run(&["prove", "not-a-command"]);
    assert!(!output.status.success());
    let text = stdout(&output);
    assert_eq!(
        text,
        "#(prove/refusal-v1 (call \"usage\") (reason \"usage\"))\n"
    );
    let err = stderr(&output);
    assert!(err.contains("sim prove"), "{err}");
    assert!(!err.contains("no codec"), "{err}");
}

#[test]
fn unknown_verb_still_needs_a_codec() {
    let output = run(&["nosuchverb", "frontier"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("no codec 'lisp'"), "{err}");
    assert!(!stdout(&output).contains("prove/load-receipt-v1"));
}

#[test]
fn world_frontier_stays_on_the_world_command() {
    let output = run(&["world", "frontier"]);
    assert!(!output.status.success());
    let text = stdout(&output);
    assert!(text.contains("world/refusal-v1"), "{text}");
    assert!(!text.contains("prove/load-receipt-v1"), "{text}");
    let err = stderr(&output);
    assert!(err.contains("deliver "), "{err}");
}

#[test]
fn prove_plan_lists_the_union_and_keeps_native_leaves_out() {
    let first = assert_quiet_receipt(&run(&["prove", "plan"]));
    let second = assert_quiet_receipt(&run(&["prove", "plan"]));
    assert_eq!(first, second);
    assert!(first.starts_with("#(prove/plan-v1 "), "{first}");
    assert_eq!(field(&first, "denominator"), "29");
    assert_eq!(field(&first, "effects"), "0");
    assert_eq!(field(&first, "subprocesses"), "0");
    assert_eq!(field(&first, "owner-commands"), "0");
    assert_eq!(field(&first, "resident"), "true");
    assert!(first.contains("leaf/native-worker-adapter"), "{first}");
    assert!(first.contains("leaf/native-assay"), "{first}");
}

#[test]
fn prove_cold_record_matches_on_a_second_run() {
    let cold = assert_quiet_receipt(&run(&["prove", "run", "cold"]));
    assert!(cold.starts_with("#(prove/cold-v1 "), "{cold}");
    assert!(!cold.contains("proof-failed"), "{cold}");
    assert_eq!(field(&cold, "denominator"), "29");
    assert_eq!(field(&cold, "executed"), "29");
    assert_eq!(field(&cold, "effects"), "0");
    assert_eq!(field(&cold, "subprocesses"), "0");
    assert_eq!(field(&cold, "reuse-disabled"), "true");
    assert!(cold.contains("leaf/native-worker-adapter"), "{cold}");
    assert!(cold.contains("leaf/native-assay"), "{cold}");
    let view = view_of(&cold);
    assert!(view.starts_with("sim-world-portable-union-v1;"), "{view}");

    let second = assert_quiet_receipt(&run(&["prove", "run", "cold"]));
    assert_eq!(cold, second);
}

const HOST_ARGUMENT_LIMIT: usize = 131_072;
const CHUNK: usize = 100_000;

fn quoted(record: &str, name: &str) -> String {
    let key = format!("({name} \"");
    let start = record
        .find(&key)
        .unwrap_or_else(|| panic!("missing {name} in {record}"))
        + key.len();
    let mut value = String::new();
    let mut chars = record[start..].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            value.push(chars.next().expect("escaped character"));
            continue;
        }
        if ch == '"' {
            return value;
        }
        value.push(ch);
    }
    panic!("{name} string did not close");
}

fn chunks(view: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = view;
    while !rest.is_empty() {
        let mut end = rest.len().min(CHUNK);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = rest.split_at(end);
        parts.push(head);
        rest = tail;
    }
    parts
}

fn prove(prefix: &[&str], parts: &[&str]) -> String {
    let mut args = Vec::with_capacity(prefix.len() + parts.len());
    args.extend_from_slice(prefix);
    args.extend_from_slice(parts);
    assert_quiet_receipt(&run(&args))
}

#[test]
fn a_split_cold_view_reaches_the_installed_binary() {
    let cold = assert_quiet_receipt(&run(&["prove", "run", "cold"]));
    let view = view_of(&cold);
    assert!(
        view.len() > HOST_ARGUMENT_LIMIT,
        "cold view is {} bytes",
        view.len()
    );
    let parts = chunks(&view);
    assert!(parts.len() > 1);
    assert_eq!(parts.concat(), view);
    assert!(parts.iter().all(|part| part.len() <= CHUNK));
    assert!(parts[0].starts_with("sim-world-portable-union-v1"));

    let refused = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["prove", "status", view.as_str()])
        .output()
        .expect_err("one host argument");
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::ArgumentListTooLong,
        "{refused}"
    );

    let status = prove(&["prove", "status"], &parts);
    assert!(status.starts_with("#(prove/status-v1 "), "{status}");
    assert_eq!(field(&status, "denominator"), field(&cold, "denominator"));
    assert_eq!(field(&status, "executed"), field(&cold, "executed"));

    let frontier = prove(&["prove", "frontier"], &parts);
    assert!(frontier.starts_with("#(prove/frontier-v1 "), "{frontier}");
    assert_eq!(field(&frontier, "complete"), "true");
    assert_eq!(field(&frontier, "denominator"), field(&cold, "denominator"));
    assert_eq!(field(&frontier, "executed"), field(&cold, "executed"));

    let leaf = prove(&["prove", "frontier", "phase/28"], &parts);
    assert_eq!(leaf, frontier);

    let reused = prove(&["prove", "run", "D1"], &parts);
    assert_eq!(field(&reused, "reused"), field(&cold, "executed"));

    let why = prove(&["prove", "why", "D2", "phase/28"], &parts);
    assert_eq!(field(&why, "reused"), "true");

    let token = quoted(&frontier, "continuation");
    let resumed = prove(&["prove", "resume", &token], &parts);
    assert_eq!(resumed, frontier);
}
