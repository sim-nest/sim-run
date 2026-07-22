use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

const TOOLING_MANIFEST_ENV: &str = "SIM_TOOLING_XTASK_MANIFEST";

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let manifest = tooling_manifest()?;
    let status = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--")
        .args(args.iter().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| format!("spawn sim-tooling index-check: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("index-check failed via {}", manifest.display()))
    }
}

fn tooling_manifest() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var(TOOLING_MANIFEST_ENV) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "{TOOLING_MANIFEST_ENV} points at missing file {}",
            path.display()
        ));
    }

    let cwd = std::env::current_dir().map_err(|err| format!("read current directory: {err}"))?;
    let sibling = cwd
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", cwd.display()))?
        .join("sim-tooling")
        .join("Cargo.toml");
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(format!(
            "sim-tooling manifest not found at {}; set {TOOLING_MANIFEST_ENV}",
            sibling.display()
        ))
    }
}
