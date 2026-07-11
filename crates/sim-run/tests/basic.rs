use std::process::Command;

#[test]
fn version_flag_prints_binary_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("--version")
        .output()
        .expect("run sim --version");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("sim 0.1.") && stdout.ends_with('\n'),
        "unexpected version line: {stdout:?}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("--help")
        .output()
        .expect("run sim --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage: sim [OPTIONS] [PAYLOAD...]"));
    assert!(stdout.contains("--version"));
    assert!(output.stderr.is_empty());
}

#[test]
fn positional_payload_enters_loaded_boot() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("run")
        .output()
        .expect("run sim run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("sim: no codec 'lisp' available")
    );
}
