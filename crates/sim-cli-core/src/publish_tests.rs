use std::{fs, path::PathBuf};

#[test]
fn manifests_carry_publish_metadata_and_version_requirements() {
    let root = source_root();
    let workspace = read(&root, "Cargo.toml");
    let binary = read(&root, "crates/sim-cli/Cargo.toml");
    let core = read(&root, "crates/sim-cli-core/Cargo.toml");

    assert_contains(
        &workspace,
        "[workspace.package]",
        "workspace package metadata",
    );
    assert_contains(&workspace, "license = \"MPL-2.0\"", "workspace license");
    assert_contains(
        &workspace,
        "repository = \"https://github.com/sim-nest/sim-cli\"",
        "workspace repository",
    );
    assert_contains(
        &workspace,
        "homepage = \"https://github.com/sim-nest/sim-cli\"",
        "workspace homepage",
    );

    assert_package_metadata(
        &core,
        "sim-cli-core",
        "Core command entry API for the SIM bootloader.",
        true, // published to crates.io
    );
    assert_contains(
        &core,
        "sim-kernel = { version = \"0.1.0\" }",
        "sim-kernel version requirement",
    );
    assert!(
        !core.contains("sim-kernel = { path"),
        "sim-kernel must not be a committed path dependency"
    );

    assert_package_metadata(&binary, "sim-run", "SIM bootloader command line.", false); // deferred CLI, not published
    assert_contains(&binary, "name = \"sim\"", "binary name");
    assert_contains(
        &binary,
        "sim-cli-core = { version = \"0.1.0\", path = \"../sim-cli-core\" }",
        "sim-cli-core versioned workspace dependency",
    );
}

#[test]
fn committed_manifests_do_not_use_absolute_local_paths() {
    let root = source_root();
    for rel in [
        "Cargo.toml",
        "crates/sim-cli/Cargo.toml",
        "crates/sim-cli-core/Cargo.toml",
        "xtask/Cargo.toml",
    ] {
        let text = read(&root, rel);
        for (line_index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            assert!(
                !is_absolute_path_dependency(trimmed),
                "{rel}:{} commits an absolute local path dependency: {trimmed}",
                line_index + 1
            );
            assert!(
                !trimmed.contains("/home/") && !trimmed.contains("/Users/"),
                "{rel}:{} commits a host-local path: {trimmed}",
                line_index + 1
            );
        }
    }
}

#[test]
fn local_source_override_config_stays_out_of_git() {
    let root = source_root();
    let gitignore = read(&root, ".gitignore");
    assert_contains(&gitignore, "/.cargo/", "local cargo config ignore rule");

    let readme = read(&root, "README.md");
    assert_contains(
        &readme,
        "cargo package --manifest-path .meta-workspace/Cargo.toml -p sim-cli-core --allow-dirty --list",
        "core package-list command",
    );
    assert_contains(
        &readme,
        "cargo package --manifest-path .meta-workspace/Cargo.toml -p sim-cli --allow-dirty --list",
        "binary package-list command",
    );
    assert_contains(
        &readme,
        "Temporary `.cargo/config.toml` files may carry local `[patch.crates-io]`",
        "local source override note",
    );
}

#[test]
fn publish_readiness_recipe_uses_meta_workspace_manifest() {
    let root = source_root();
    let book = read(&root, "recipes/book.toml");
    assert_contains(
        &book,
        "\"publish-readiness\"",
        "publish readiness chapter registration",
    );

    let recipe = read(&root, "recipes/publish-readiness/package-list/recipe.toml");
    assert_contains(&recipe, "id = \"package-list\"", "recipe id");
    assert_contains(&recipe, "network = false", "offline package-list recipe");
    assert_contains(
        &recipe,
        "requires = [\"sim-cli\", \"sim-cli-core\", \"SIM_META_WORKSPACE_MANIFEST\"]",
        "recipe requirements",
    );

    let setup = read(&root, "recipes/publish-readiness/package-list/setup.sh");
    assert_contains(
        &setup,
        "cargo package --manifest-path \"$SIM_META_WORKSPACE_MANIFEST\" -p sim-cli-core --allow-dirty --list",
        "core package-list setup",
    );
    assert_contains(
        &setup,
        "cargo package --manifest-path \"$SIM_META_WORKSPACE_MANIFEST\" -p sim-cli --allow-dirty --list",
        "binary package-list setup",
    );
}

fn assert_package_metadata(manifest: &str, package: &str, description: &str, publish: bool) {
    assert_contains(manifest, &format!("name = \"{package}\""), package);
    assert_contains(manifest, "version = \"0.1.0\"", package);
    assert_contains(manifest, "license.workspace = true", package);
    assert_contains(
        manifest,
        &format!("description = \"{description}\""),
        package,
    );
    assert_contains(manifest, "readme = \"README.md\"", package);
    assert_contains(manifest, &format!("publish = {publish}"), package);
    assert_contains(
        manifest,
        "repository = \"https://github.com/sim-nest/sim-cli\"",
        package,
    );
    assert_contains(
        manifest,
        "homepage = \"https://github.com/sim-nest/sim-cli\"",
        package,
    );
}

fn assert_contains(haystack: &str, needle: &str, label: &str) {
    assert!(
        haystack.contains(needle),
        "{label} missing expected text: {needle}"
    );
}

fn is_absolute_path_dependency(line: &str) -> bool {
    line.contains("path = \"/") || line.contains("path = '/") || line.contains("path = \"~/")
}

fn read(root: &std::path::Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("read {rel}: {err}"))
}

fn source_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in [
        manifest_dir.join("../.."),
        manifest_dir.join("../../../../sim-cli"),
    ] {
        if candidate.join("recipes/book.toml").exists() {
            return candidate;
        }
    }
    panic!("could not locate sim-cli source root from {manifest_dir:?}");
}
