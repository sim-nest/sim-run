use crate::BuildFailure;
use sim_kernel::Symbol;
use std::collections::BTreeSet;

/// Immutable identity of the delivered Cargo/Rust toolchain mount.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainIdentity {
    /// Content identity of the sealed toolchain.
    pub content: String,
    /// Sandbox program identity resolved by the launcher.
    pub cargo_program: String,
    /// Exact environment allowlist supplied by the trusted composer.
    pub environment: Vec<(String, String)>,
}

/// Pure request for one native library build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBuildRequest {
    /// Boot-resolved identity of the sealed source mount.
    pub source_mount: String,
    /// Relative path to Cargo.toml inside that mount.
    pub manifest: String,
    /// Exact Cargo package name.
    pub package: String,
    /// Sorted feature names.
    pub features: BTreeSet<String>,
    /// Library symbol expected during later admission.
    pub expected_library: Symbol,
    /// Delivered toolchain identity.
    pub toolchain: ToolchainIdentity,
}

impl NativeBuildRequest {
    pub(crate) fn validate_fields(&self) -> Result<(), BuildFailure> {
        validate_relative(&self.manifest)?;
        validate_atom("package", &self.package)?;
        if self.source_mount.is_empty()
            || self.toolchain.content.is_empty()
            || self.toolchain.cargo_program.is_empty()
        {
            return Err(BuildFailure::request(
                "mount and toolchain identities must be non-empty",
            ));
        }
        for feature in &self.features {
            validate_atom("feature", feature)?;
        }
        for (key, value) in &self.toolchain.environment {
            if !matches!(key.as_str(), "PATH" | "RUSTFLAGS" | "RUSTC" | "RUSTDOC")
                || key.is_empty()
                || value.contains('\0')
            {
                return Err(BuildFailure::toolchain(
                    "invalid toolchain environment allowlist",
                ));
            }
        }
        Ok(())
    }
}

fn validate_relative(value: &str) -> Result<(), BuildFailure> {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\0')
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(BuildFailure::request(
            "manifest must be a confined relative path",
        ));
    }
    Ok(())
}

fn validate_atom(label: &str, value: &str) -> Result<(), BuildFailure> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(BuildFailure::request(format!("invalid {label} name")));
    }
    Ok(())
}
