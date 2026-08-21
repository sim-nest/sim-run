// conformance: the published bootloader reaches the host-registered JVM product specimen.

#![cfg(not(any(feature = "dynamic-native", feature = "wasm")))]

use std::process::Command;

#[test]
fn published_binary_runs_caller_selected_bytecode() {
    let hex = include_str!("fixtures/StaticInt.hex").trim();
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["jvm", &hex, "StaticInt", "wholePipeline", "(II)I", "5", "6"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("JVM value: 22"), "{stdout}");
}
