use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

// conformance: the standard sim distribution dispatches the frozen expression-tree recipe.

#[test]
fn expr_tree_help_uses_the_standard_bootloader_envelope() {
    let output = sim().args(["expr-tree", "--help"]).output().unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: sim"), "{stdout}");
    assert!(stdout.contains("--config-file"), "{stdout}");
}

#[test]
fn expr_tree_configured_storage_kinds_boot_and_shutdown_cleanly() {
    for (kind, storage, thread) in [
        ("memory", "memory-tree", 83_001),
        ("filesystem", "filesystem-tree", 83_002),
        ("database", "database-tree", 83_003),
    ] {
        let config = TempConfig::new(
            kind,
            &format!(
                "[lib/expr-tree-serve]\n\
                 dry-run = true\n\
                 storage = \"{storage}\"\n\
                 browser-resource = \"{kind}-browser-tree\"\n\
                 bridge-thread = {thread}\n"
            ),
        );
        let output = sim()
            .arg("--config-file")
            .arg(config.path())
            .arg("expr-tree")
            .output()
            .unwrap();

        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("sim-web-shell: dry-run OK"),
            "{kind}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn expr_tree_invalid_config_is_refused_by_the_serve_library() {
    let config = TempConfig::new(
        "invalid",
        "[lib/expr-tree-serve]\ndry-run = true\nstorage = \"\"\n",
    );
    let output = sim()
        .arg("--config-file")
        .arg(config.path())
        .arg("expr-tree")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("expression-tree config: storage must not be empty"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn expr_tree_external_server_placement_fails_closed_without_the_site() {
    let config = TempConfig::new(
        "external",
        "[lib/expr-tree-serve]\n\
         dry-run = true\n\
         placement = \"external\"\n\
         server-site = \"site/missing-expression-tree\"\n",
    );
    let output = sim()
        .arg("--config-file")
        .arg(config.path())
        .arg("expr-tree")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "configured expression-tree EvalFabric site site/missing-expression-tree is not loaded"
        ),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn expr_tree_explicit_loads_override_the_standard_recipe_sources() {
    let output = sim()
        .args(["--load", "host:missing", "expr-tree"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("sim: unknown host library: missing"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sim() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sim"))
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempConfig {
    path: PathBuf,
}

impl TempConfig {
    fn new(label: &str, contents: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sim-run-expr-tree-{label}-{}-{nonce}.toml",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
