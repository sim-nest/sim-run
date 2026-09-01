//! Structural proof for the bootloader/platform membrane.

use std::{fs, path::Path};

const AMBIENT_PROCESS_INPUTS: &[&str] = &[
    "std::env::args()",
    "std::env::args_os()",
    "std::env::current_dir()",
    "std::env::var(",
    "std::env::var_os",
    "std::env::vars",
];

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some("check-run-host-surfaces") || args.len() != 2 {
        return Err(format!("usage: {program} check-run-host-surfaces"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask manifest has no workspace parent".to_owned())?;
    let contract = fs::read_to_string(root.join("run-host-surfaces.toml"))
        .map_err(|error| format!("read run-host-surfaces.toml: {error}"))?;
    for required in [
        "schema = \"sim.run-host-surfaces/v1\"",
        "role = \"host-tool\"",
        "sim_platform_ubuntu_pc::UbuntuProcessEnvelope::capture()",
    ] {
        if !contract.contains(required) {
            return Err(format!("run host-surface contract is missing `{required}`"));
        }
    }
    for relative in ["crates/sim-run-core/src", "crates/sim-run/src"] {
        check_tree(&root.join(relative), AMBIENT_PROCESS_INPUTS)?;
    }
    let main = fs::read_to_string(root.join("crates/sim-run/src/main.rs"))
        .map_err(|error| format!("read sim main: {error}"))?;
    if main.lines().filter(|line| !line.trim().is_empty()).count() != 3
        || !main.contains("sim_run::process_main();")
    {
        return Err("sim main is not the one-call bootloader frame".to_owned());
    }
    println!("check-run-host-surfaces: OK");
    Ok(())
}

fn check_tree(root: &Path, forbidden: &[&str]) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|error| format!("read {}: {error}", root.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            check_tree(&path, forbidden)?;
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs"))
        {
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            for needle in forbidden {
                if source.contains(needle) {
                    return Err(format!(
                        "{} contains ambient process input `{needle}`",
                        path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}
