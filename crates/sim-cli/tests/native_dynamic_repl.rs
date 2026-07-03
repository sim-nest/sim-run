#![cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const PATCHES: &[(&str, &str, &str)] = &[
    ("sim-citizen", "sim-citizen", "crates/sim-citizen"),
    (
        "sim-citizen-derive",
        "sim-citizen",
        "crates/sim-citizen-derive",
    ),
    ("sim-codec", "sim-codecs", "crates/sim-codec"),
    ("sim-codec-binary", "sim-codecs", "crates/sim-codec-binary"),
    ("sim-cookbook", "sim-foundation", "crates/sim-cookbook"),
    ("sim-kernel", "sim-kernel", "."),
    (
        "sim-lib-numbers-core",
        "sim-numbers",
        "crates/sim-lib-numbers-core",
    ),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];

#[test]
fn sim_repl_loads_eval_bundle_and_evaluates_stdin() {
    let bundle_dir = build_repl_bundle();
    assert!(
        bundle_dir
            .join(dylib_file_name("sim_lib_numbers_f64"))
            .is_file()
    );
    assert!(
        bundle_dir
            .join(dylib_file_name("sim_lib_standard_core"))
            .is_file()
    );

    let output = run_repl_input(&bundle_dir, "foo\n");
    assert_repl_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("UnknownSymbol"), "{stdout}");
    assert!(stdout.contains("foo"), "{stdout}");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");

    let output = run_repl_input(&bundle_dir, "(foo 1)\n");
    assert_repl_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("UnknownFunction"), "{stdout}");
    assert!(stdout.contains("foo"), "{stdout}");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");

    let output = run_repl_input(&bundle_dir, "(math/add (math/mul 6 7) 0)\n");
    assert_repl_success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");

    let target_dir = bundle_dir
        .parent()
        .expect("bundle dir should be target/debug");
    remove_dir_all_if_exists(target_dir);
}

fn run_repl_input(bundle_dir: &Path, input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("repl")
        .env("SIM_REPL_BUNDLE_DIR", bundle_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sim repl should start");

    child
        .stdin
        .as_mut()
        .expect("sim repl stdin should be piped")
        .write_all(input.as_bytes())
        .expect("write repl input");
    child.wait_with_output().expect("wait for sim repl")
}

fn assert_repl_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "sim repl failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn build_repl_bundle() -> PathBuf {
    let target_dir = unique_target_dir();
    build_native_dylib(
        "sim-lib-numbers-f64",
        numbers_f64_manifest_path(),
        &["native-export"],
        &target_dir,
    );
    build_native_dylib(
        "sim-lib-standard-core",
        standard_core_manifest_path(),
        &["native-export"],
        &target_dir,
    );
    target_dir.join("debug")
}

fn build_native_dylib(package: &str, manifest_path: PathBuf, features: &[&str], target_dir: &Path) {
    let mut command = Command::new(cargo_bin());
    command.env("RUSTFLAGS", "-D warnings").arg("build");
    if let Some(meta_manifest) = meta_workspace_manifest() {
        command
            .arg("--manifest-path")
            .arg(meta_manifest)
            .arg("-p")
            .arg(package);
    } else {
        command.arg("--manifest-path").arg(manifest_path);
        add_patch_args(&mut command);
    }
    if !features.is_empty() {
        command.arg("--features").arg(features.join(","));
    }
    command.arg("--target-dir").arg(target_dir);

    let status = command
        .status()
        .unwrap_or_else(|err| panic!("cargo build for {package} should start: {err}"));
    assert!(status.success(), "{package} native dylib build failed");
}

fn meta_workspace_manifest() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "packages")
    {
        return manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("Cargo.toml"));
    }
    None
}

fn numbers_f64_manifest_path() -> PathBuf {
    package_path(
        "sim-lib-numbers-f64",
        "sim-numbers",
        "crates/sim-lib-numbers-f64",
    )
    .join("Cargo.toml")
}

fn standard_core_manifest_path() -> PathBuf {
    package_path(
        "sim-lib-standard-core",
        "sim-runtime",
        "crates/sim-lib-standard-core",
    )
    .join("Cargo.toml")
}

fn package_path(crate_name: &str, repo_name: &str, source_path: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "packages")
    {
        return manifest_dir
            .parent()
            .expect("meta-workspace package should have a packages parent")
            .join(crate_name);
    }

    let sim_cli_repo = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("sim-cli package should live under crates/sim-cli");
    if repo_name == "sim-cli" {
        return sim_cli_repo.join(source_path);
    }
    sim_cli_repo
        .parent()
        .expect("sim-cli checkout should have sibling repos")
        .join(repo_name)
        .join(source_path)
}

fn add_patch_args(command: &mut Command) {
    for (crate_name, repo_name, source_path) in PATCHES {
        let path = package_path(crate_name, repo_name, source_path);
        command.arg("--config").arg(format!(
            "patch.crates-io.{crate_name}.path={}",
            toml_string(&path)
        ));
    }
}

fn toml_string(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn unique_target_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("sim-repl-native-bundle-{nanos}"))
}

fn dylib_file_name(base: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("{base}.dll")
    }
    #[cfg(target_os = "macos")]
    {
        format!("lib{base}.dylib")
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        format!("lib{base}.so")
    }
}

fn remove_dir_all_if_exists(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}
