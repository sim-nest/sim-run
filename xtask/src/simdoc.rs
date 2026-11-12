// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Thin simdoc launcher: the isolated encoder, reached from this repository.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some("simdoc") {
        return Err(format!("usage: {program} simdoc [--check]"));
    }

    let root = env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let manifest = locate_simdoc_manifest(&root)?;
    let mut command = Command::new("cargo");
    command.args(["run", "--locked", "--manifest-path"]);
    command.arg(&manifest);
    command.args(["--quiet", "--", "simdoc", "--repo-root"]);
    command.arg(&root);
    for arg in args.iter().skip(2) {
        command.arg(arg);
    }

    let status = command
        .status()
        .map_err(|err| format!("run isolated simdoc encoder: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "isolated simdoc encoder failed with status {status}"
        ))
    }
}

fn locate_simdoc_manifest(repo_root: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SIMDOC_TOOLING_MANIFEST") {
        // A workspace manifest refuses the simdoc verb. Resolve that hint
        // to the isolated encoder.
        return isolated_manifest_from_hint(Path::new(&path));
    }
    let isolated = sibling_isolated_manifest(repo_root);
    if isolated.is_file() {
        return Ok(isolated);
    }
    Err(
        "set SIMDOC_TOOLING_MANIFEST to the sim-tooling checkout or crates/simdoc/Cargo.toml"
            .to_owned(),
    )
}

fn sibling_isolated_manifest(repo_root: &Path) -> PathBuf {
    repo_root
        .parent()
        .unwrap_or(repo_root)
        .join("sim-tooling")
        .join("crates")
        .join("simdoc")
        .join("Cargo.toml")
}

fn isolated_manifest_from_hint(hint: &Path) -> Result<PathBuf, String> {
    let isolated_rel = Path::new("crates").join("simdoc").join("Cargo.toml");
    let candidates = [
        hint.to_path_buf(),
        hint.join("Cargo.toml"),
        hint.join(&isolated_rel),
        hint.parent().unwrap_or(hint).join(&isolated_rel),
    ];
    for candidate in candidates {
        if candidate.is_file() && candidate.ends_with(&isolated_rel) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "SIMDOC_TOOLING_MANIFEST={} does not locate crates/simdoc/Cargo.toml",
        hint.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_rel() -> PathBuf {
        Path::new("crates").join("simdoc").join("Cargo.toml")
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "sim-run-simdoc-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            let simdoc = root.join("sim-tooling").join("crates").join("simdoc");
            std::fs::create_dir_all(&simdoc).expect("fixture directory");
            std::fs::write(simdoc.join("Cargo.toml"), "[package]\nname = \"simdoc\"\n")
                .expect("fixture manifest");
            std::fs::write(simdoc.join("Cargo.lock"), "# fixture\n").expect("fixture lock");
            std::fs::write(root.join("sim-tooling").join("Cargo.toml"), "[workspace]\n")
                .expect("fixture workspace");
            std::fs::create_dir_all(root.join("sim-run")).expect("fixture repository");
            Self { root }
        }

        fn repo(&self) -> PathBuf {
            self.root.join("sim-run")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn tooling_hints_resolve_to_the_isolated_encoder() {
        let fixture = Fixture::new();
        let selected = sibling_isolated_manifest(&fixture.repo());
        assert!(selected.is_file(), "{}", selected.display());
        assert!(selected.ends_with(isolated_rel()));
        assert!(selected.with_file_name("Cargo.lock").is_file());
        let tooling = fixture.root.join("sim-tooling");
        assert_eq!(
            isolated_manifest_from_hint(&tooling.join("Cargo.toml")).expect("workspace hint"),
            selected
        );
        assert_eq!(
            isolated_manifest_from_hint(&selected).expect("isolated hint"),
            selected
        );
        assert_eq!(
            isolated_manifest_from_hint(&tooling).expect("checkout hint"),
            selected
        );
    }

    #[test]
    fn missing_tooling_hint_is_an_error() {
        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("no-such-tooling")
            .join("Cargo.toml");
        let error = isolated_manifest_from_hint(&missing).expect_err("missing hint");
        assert!(error.contains("does not locate crates/simdoc/Cargo.toml"));
    }
}
