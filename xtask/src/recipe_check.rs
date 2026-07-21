use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(args: &[String]) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.len() != 2 {
        return Err(format!("usage: {program} check-recipes"));
    }

    let root = env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let setups = setup_scripts(&root)?;
    if setups.is_empty() {
        return Err("check-recipes: no recipe setup scripts found".to_owned());
    }

    run_cargo(&root, ["build", "-p", "sim-run"])?;
    let meta_manifest = meta_workspace_manifest(&root)?;
    let path = path_with_front(root.join("target/debug"))?;

    for setup in &setups {
        let rel = setup.strip_prefix(&root).unwrap_or(setup);
        println!("check-recipes: {}", rel.display());
        let status = Command::new("sh")
            .arg(setup)
            .current_dir(setup.parent().unwrap_or(&root))
            .env("SIM_META_WORKSPACE_MANIFEST", &meta_manifest)
            .env("PATH", &path)
            .status()
            .map_err(|err| format!("run {}: {err}", rel.display()))?;
        if !status.success() {
            return Err(format!(
                "check-recipes: {} failed with status {status}",
                rel.display()
            ));
        }
    }

    println!(
        "check-recipes: OK ({} recipe setup script(s))",
        setups.len()
    );
    Ok(())
}

fn run_cargo<const N: usize>(root: &Path, args: [&str; N]) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|err| format!("run cargo: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo failed with status {status}"))
    }
}

fn setup_scripts(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    collect_setup_scripts(&root.join("recipes"), &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_setup_scripts(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {} entry: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_setup_scripts(&path, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("setup.sh") {
            out.push(path);
        }
    }
    Ok(())
}

fn meta_workspace_manifest(root: &Path) -> Result<PathBuf, String> {
    if let Ok(value) = env::var("SIM_META_WORKSPACE_MANIFEST") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "SIM_META_WORKSPACE_MANIFEST does not name a file: {}",
            path.display()
        ));
    }

    let sibling = root
        .parent()
        .unwrap_or(root)
        .join("sim-private")
        .join(".meta-workspace")
        .join("Cargo.toml");
    if sibling.is_file() {
        return Ok(sibling);
    }
    Err("run `sh bin/simctl meta-build` or set SIM_META_WORKSPACE_MANIFEST".to_owned())
}

fn path_with_front(front: PathBuf) -> Result<OsString, String> {
    let current = env::var_os("PATH").unwrap_or_default();
    env::join_paths(std::iter::once(front).chain(env::split_paths(&current)))
        .map_err(|err| format!("build PATH: {err}"))
}
