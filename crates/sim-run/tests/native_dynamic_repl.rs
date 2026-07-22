#![cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]

mod support;

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use support::{
    FeatureBuildContext, cargo_bin, maybe_feature_build_context, remove_dir_all_if_exists,
    unique_target_dir,
};

const NUMBERS_F64_SOURCE: (&str, &str, &str) = (
    "sim-lib-numbers-f64",
    "sim-numbers",
    "crates/sim-lib-numbers-f64",
);
const LISP_CODEC_SOURCE: (&str, &str, &str) =
    ("sim-codec-lisp", "sim-codecs", "crates/sim-codec-lisp");
const STANDARD_CORE_SOURCE: (&str, &str, &str) = (
    "sim-lib-standard-core",
    "sim-runtime",
    "crates/sim-lib-standard-core",
);
const PATCHES: &[(&str, &str, &str)] = &[
    ("sim-citizen", "sim-citizen", "crates/sim-citizen"),
    (
        "sim-citizen-derive",
        "sim-citizen",
        "crates/sim-citizen-derive",
    ),
    ("sim-codec", "sim-codecs", "crates/sim-codec"),
    ("sim-codec-binary", "sim-codecs", "crates/sim-codec-binary"),
    ("sim-lib-core", "sim-runtime", "crates/sim-lib-core"),
    ("sim-cookbook", "sim-foundation", "crates/sim-cookbook"),
    ("sim-kernel", "sim-kernel", "."),
    (
        "sim-lib-numbers-core",
        "sim-numbers",
        "crates/sim-lib-numbers-core",
    ),
    (
        "sim-lib-numbers-f64",
        "sim-numbers",
        "crates/sim-lib-numbers-f64",
    ),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-nest", "sim-sdk", "."),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];
const REQUIRED_SOURCES: &[(&str, &str, &str)] = &[
    LISP_CODEC_SOURCE,
    NUMBERS_F64_SOURCE,
    STANDARD_CORE_SOURCE,
    ("sim-citizen", "sim-citizen", "crates/sim-citizen"),
    (
        "sim-citizen-derive",
        "sim-citizen",
        "crates/sim-citizen-derive",
    ),
    ("sim-codec", "sim-codecs", "crates/sim-codec"),
    ("sim-codec-binary", "sim-codecs", "crates/sim-codec-binary"),
    ("sim-lib-core", "sim-runtime", "crates/sim-lib-core"),
    ("sim-cookbook", "sim-foundation", "crates/sim-cookbook"),
    ("sim-kernel", "sim-kernel", "."),
    (
        "sim-lib-numbers-core",
        "sim-numbers",
        "crates/sim-lib-numbers-core",
    ),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-nest", "sim-sdk", "."),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];

#[test]
fn sim_repl_loads_native_proof_bundle_and_evaluates_stdin() {
    let Some(context) = maybe_feature_build_context(REQUIRED_SOURCES) else {
        return;
    };
    let bundle_dir = build_repl_bundle(&context);
    assert!(bundle_dir.join(dylib_file_name("sim_codec_lisp")).is_file());
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

    let output = run_repl_input(&bundle_dir, "42\n");
    assert_repl_success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");

    let output = run_repl_input(&bundle_dir, "(math/add 1 2)\n");
    assert_repl_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("UnknownFunction"), "{stdout}");
    assert!(stdout.contains("math"), "{stdout}");
    assert!(stdout.contains("add"), "{stdout}");
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

fn build_repl_bundle(context: &FeatureBuildContext) -> PathBuf {
    let target_dir = unique_target_dir("repl-native-bundle");
    build_native_dylib(
        context,
        "sim-codec-lisp",
        LISP_CODEC_SOURCE,
        &["native-export"],
        &target_dir,
    );
    build_native_dylib(
        context,
        "sim-lib-numbers-f64",
        NUMBERS_F64_SOURCE,
        &["native-export"],
        &target_dir,
    );
    build_native_dylib(
        context,
        "sim-lib-standard-core",
        STANDARD_CORE_SOURCE,
        &["native-export"],
        &target_dir,
    );
    target_dir.join("debug")
}

fn build_native_dylib(
    context: &FeatureBuildContext,
    package: &str,
    manifest_spec: (&str, &str, &str),
    features: &[&str],
    target_dir: &Path,
) {
    let mut command = Command::new(cargo_bin());
    context.configure_build(&mut command, package, manifest_spec, PATCHES);
    if !features.is_empty() {
        command.arg("--features").arg(features.join(","));
    }
    command
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .arg("--target-dir")
        .arg(target_dir);

    let status = command
        .status()
        .unwrap_or_else(|err| panic!("cargo build for {package} should start: {err}"));
    assert!(status.success(), "{package} native dylib build failed");
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
