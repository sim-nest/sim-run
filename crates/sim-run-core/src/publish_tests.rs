use std::collections::HashMap;
use std::{fs, path::PathBuf};

#[test]
fn manifests_carry_publish_metadata_and_version_requirements() {
    let root = source_root();
    let workspace = Manifest::parse(&read(&root, "Cargo.toml"));
    let binary = Manifest::parse(&read(&root, "crates/sim-run/Cargo.toml"));
    let core = Manifest::parse(&read(&root, "crates/sim-run-core/Cargo.toml"));
    let loaders = Manifest::parse(&read(&root, "crates/sim-run-loaders/Cargo.toml"));
    let repl = Manifest::parse(&read(&root, "crates/sim-lib-repl/Cargo.toml"));
    let tty = Manifest::parse(&read(&root, "crates/sim-view-tty/Cargo.toml"));

    assert_eq_value(&workspace, "workspace.package", "license", "MPL-2.0");
    assert_eq_value(
        &workspace,
        "workspace.package",
        "repository",
        "https://github.com/sim-nest/sim-run",
    );
    assert_eq_value(
        &workspace,
        "workspace.package",
        "homepage",
        "https://github.com/sim-nest/sim-run",
    );

    assert_package_metadata(
        &core,
        "sim-run-core",
        "Core command entry API for the SIM bootloader.",
    );
    assert_dependency_version(&core, "sim-kernel", "0.1.4");
    assert_dependency_has_no_path(&core, "sim-kernel");
    assert_dependency_version(&core, "sim-run-loaders", "0.1.4");
    assert_dependency_path(&core, "sim-run-loaders", "../sim-run-loaders");

    assert_package_metadata(&binary, "sim-run", "SIM bootloader command line.");
    assert_eq_value(&binary, "bin", "name", "sim");
    assert_dependency_version(&binary, "sim-run-core", "0.1.6");
    assert_dependency_path(&binary, "sim-run-core", "../sim-run-core");
    assert_dependency_version(&binary, "sim-run-loaders", "0.1.4");
    assert_dependency_path(&binary, "sim-run-loaders", "../sim-run-loaders");

    assert_package_metadata(
        &loaders,
        "sim-run-loaders",
        "Feature-composable SIM bootloader loaders.",
    );
    assert_package_metadata(
        &repl,
        "sim-lib-repl",
        "Loadable SIM command-line REPL library.",
    );
    assert_package_metadata(
        &tty,
        "sim-view-tty",
        "Loadable terminal (CLI/TUI) view/edit surface for SIM, projecting Scenes to text and reducing key input to Intents.",
    );
}

#[test]
fn committed_manifests_do_not_use_absolute_local_paths() {
    let root = source_root();
    for rel in manifest_paths() {
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
        "SIM_META_WORKSPACE_MANIFEST=\"$PWD/.meta-workspace/Cargo.toml\" sh ../sim-run/recipes/publish-readiness/package-list/setup.sh",
        "package-list recipe command",
    );
    assert_contains(
        &readme,
        "Temporary `.cargo/config.toml` files may carry local `[patch.crates-io]`",
        "local source override note",
    );
}

#[test]
fn publish_readiness_recipe_derives_publishable_workspace_packages() {
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
        "requires = [\"SIM_META_WORKSPACE_MANIFEST\", \"workspace-metadata\"]",
        "recipe requirements",
    );

    let setup = read(&root, "recipes/publish-readiness/package-list/setup.sh");
    assert_contains(
        &setup,
        "cargo metadata --manifest-path \"$REPO_MANIFEST\" --format-version=1 --no-deps",
        "workspace metadata command",
    );
    assert_contains(
        &setup,
        "for package_id in metadata[\"workspace_members\"]",
        "workspace member iteration",
    );
    assert_contains(
        &setup,
        "cargo package --manifest-path \"$SIM_META_WORKSPACE_MANIFEST\" -p \"$package\" --allow-dirty --list",
        "derived package-list setup",
    );
    assert!(
        !setup.contains("-p sim-run-core") && !setup.contains("-p sim-run --"),
        "package-list setup must not hardcode individual package names"
    );

    let purpose = read(&root, "recipes/publish-readiness/package-list/purpose.md");
    assert_contains(
        &purpose,
        "every publishable workspace package",
        "purpose covers all publishable packages",
    );
}

#[derive(Debug)]
struct Manifest {
    sections: HashMap<String, HashMap<String, String>>,
}

impl Manifest {
    fn parse(text: &str) -> Self {
        let mut sections = HashMap::<String, HashMap<String, String>>::new();
        let mut current = String::new();
        for raw_line in text.lines() {
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(section) = table_name(line) {
                current = section;
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        Self { sections }
    }

    fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|section| section.get(key))
            .map(String::as_str)
    }
}

fn assert_package_metadata(manifest: &Manifest, package: &str, description: &str) {
    assert_eq_value(manifest, "package", "name", package);
    assert_present(manifest, "package", "version");
    assert_eq_value(manifest, "package", "license.workspace", "true");
    assert_eq_value(manifest, "package", "description", description);
    assert_eq_value(manifest, "package", "readme", "README.md");
    assert_eq_value(manifest, "package", "publish", "true");
    assert_eq_value(
        manifest,
        "package",
        "repository",
        "https://github.com/sim-nest/sim-run",
    );
    assert_eq_value(
        manifest,
        "package",
        "homepage",
        "https://github.com/sim-nest/sim-run",
    );
}

fn assert_dependency_version(manifest: &Manifest, dependency: &str, expected: &str) {
    let value = dependency_value(manifest, dependency);
    assert_eq!(
        inline_field(value, "version").as_deref(),
        Some(expected),
        "dependency {dependency} must carry version {expected}"
    );
}

fn assert_dependency_path(manifest: &Manifest, dependency: &str, expected: &str) {
    let value = dependency_value(manifest, dependency);
    assert_eq!(
        inline_field(value, "path").as_deref(),
        Some(expected),
        "dependency {dependency} must carry path {expected}"
    );
}

fn assert_dependency_has_no_path(manifest: &Manifest, dependency: &str) {
    let value = dependency_value(manifest, dependency);
    assert!(
        inline_field(value, "path").is_none(),
        "dependency {dependency} must not carry a path"
    );
}

fn dependency_value<'a>(manifest: &'a Manifest, dependency: &str) -> &'a str {
    manifest
        .get("dependencies", dependency)
        .unwrap_or_else(|| panic!("missing dependency {dependency}"))
}

fn assert_present(manifest: &Manifest, section: &str, key: &str) {
    assert!(
        manifest.get(section, key).is_some(),
        "missing [{section}] {key}"
    );
}

fn assert_eq_value(manifest: &Manifest, section: &str, key: &str, expected: &str) {
    let actual = manifest
        .get(section, key)
        .unwrap_or_else(|| panic!("missing [{section}] {key}"));
    assert_eq!(unquote(actual), expected, "[{section}] {key}");
}

fn assert_contains(haystack: &str, needle: &str, label: &str) {
    assert!(
        haystack.contains(needle),
        "{label} missing expected text: {needle}"
    );
}

fn inline_field(value: &str, field: &str) -> Option<String> {
    let table = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    for entry in table.split(',') {
        let (key, value) = entry.split_once('=')?;
        if key.trim() == field {
            return Some(unquote(value.trim()).to_owned());
        }
    }
    None
}

fn table_name(line: &str) -> Option<String> {
    if line.starts_with("[[") && line.ends_with("]]") {
        return Some(line[2..line.len() - 2].trim().to_owned());
    }
    if line.starts_with('[') && line.ends_with(']') {
        return Some(line[1..line.len() - 1].trim().to_owned());
    }
    None
}

fn strip_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(before, _)| before)
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"')
}

fn is_absolute_path_dependency(line: &str) -> bool {
    line.contains("path = \"/") || line.contains("path = '/") || line.contains("path = \"~/")
}

fn manifest_paths() -> &'static [&'static str] {
    &[
        "Cargo.toml",
        "crates/sim-lib-repl/Cargo.toml",
        "crates/sim-run/Cargo.toml",
        "crates/sim-run-core/Cargo.toml",
        "crates/sim-run-loaders/Cargo.toml",
        "crates/sim-view-tty/Cargo.toml",
        "xtask/Cargo.toml",
    ]
}

fn read(root: &std::path::Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("read {rel}: {err}"))
}

fn source_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in [
        manifest_dir.join("../.."),
        manifest_dir.join("../../../../sim-run"),
    ] {
        if candidate.join("recipes/book.toml").exists() {
            return candidate;
        }
    }
    panic!("could not locate sim-run source root from {manifest_dir:?}");
}
