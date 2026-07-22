use std::{
    path::{Path, PathBuf},
    process::Command,
};

const META_WORKSPACE_MANIFEST_ENV: &str = "SIM_META_WORKSPACE_MANIFEST";

pub struct FeatureBuildContext {
    meta_manifest: Option<PathBuf>,
    repo_root: PathBuf,
}

impl FeatureBuildContext {
    pub fn detect(required_sources: &[(&str, &str, &str)]) -> Result<Self, String> {
        let repo_root = repo_root();
        if let Some(meta_manifest) = meta_workspace_manifest()? {
            return Ok(Self {
                meta_manifest: Some(meta_manifest),
                repo_root,
            });
        }

        let missing = required_sources
            .iter()
            .filter_map(|(_, repo_name, source_path)| {
                let path = sibling_source_path(&repo_root, repo_name, source_path);
                (!path.exists()).then(|| path.display().to_string())
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "set {META_WORKSPACE_MANIFEST_ENV} to a generated meta-workspace Cargo.toml or clone the required sibling checkout(s): {}",
                missing.join(", ")
            ));
        }

        Ok(Self {
            meta_manifest: None,
            repo_root,
        })
    }

    pub fn configure_build(
        &self,
        command: &mut Command,
        package: &str,
        manifest_spec: (&str, &str, &str),
        patches: &[(&str, &str, &str)],
    ) {
        command.env("RUSTFLAGS", "-D warnings").arg("build");
        if let Some(meta_manifest) = &self.meta_manifest {
            command
                .arg("--manifest-path")
                .arg(meta_manifest)
                .arg("-p")
                .arg(package);
            return;
        }

        command
            .arg("--manifest-path")
            .arg(self.manifest_path(manifest_spec));
        self.add_patch_args(command, patches);
    }

    pub fn source_path(&self, crate_name: &str, repo_name: &str, source_path: &str) -> PathBuf {
        if let Some(meta_manifest) = &self.meta_manifest {
            return meta_manifest
                .parent()
                .expect("meta-workspace manifest should have a parent")
                .join("packages")
                .join(crate_name);
        }
        sibling_source_path(&self.repo_root, repo_name, source_path)
    }

    pub fn manifest_path(&self, spec: (&str, &str, &str)) -> PathBuf {
        self.source_path(spec.0, spec.1, spec.2).join("Cargo.toml")
    }

    fn add_patch_args(&self, command: &mut Command, patches: &[(&str, &str, &str)]) {
        for (crate_name, repo_name, source_path) in patches {
            let path = self.source_path(crate_name, repo_name, source_path);
            command.arg("--config").arg(format!(
                "patch.crates-io.{crate_name}.path={}",
                toml_string(&path)
            ));
        }
    }
}

pub fn maybe_feature_build_context(
    required_sources: &[(&str, &str, &str)],
) -> Option<FeatureBuildContext> {
    match FeatureBuildContext::detect(required_sources) {
        Ok(context) => Some(context),
        Err(reason) => {
            eprintln!("feature test skipped: {reason}");
            None
        }
    }
}

pub fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn toml_string(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

fn meta_workspace_manifest() -> Result<Option<PathBuf>, String> {
    if let Some(manifest) = std::env::var_os(META_WORKSPACE_MANIFEST_ENV) {
        let manifest = PathBuf::from(manifest);
        if manifest.is_file() {
            return Ok(Some(manifest));
        }
        return Err(format!(
            "{META_WORKSPACE_MANIFEST_ENV} points to a missing manifest: {}",
            manifest.display()
        ));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "packages")
    {
        let manifest = manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("meta-workspace package should have a workspace root")
            .join("Cargo.toml");
        if manifest.is_file() {
            return Ok(Some(manifest));
        }
        return Err(format!(
            "embedded meta-workspace manifest is missing: {}",
            manifest.display()
        ));
    }

    Ok(None)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("sim-run package should live under crates/sim-run")
        .to_path_buf()
}

fn sibling_source_path(repo_root: &Path, repo_name: &str, source_path: &str) -> PathBuf {
    if repo_name == "sim-run" {
        repo_root.join(source_path)
    } else {
        repo_root
            .parent()
            .expect("sim-run checkout should have sibling repos")
            .join(repo_name)
            .join(source_path)
    }
}
