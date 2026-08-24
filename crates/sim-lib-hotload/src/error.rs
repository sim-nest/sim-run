use std::{error::Error, fmt};

const MAX_DIAGNOSTIC: usize = 2048;

/// Stable failure categories for native construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    /// Structured request was refused before spawn.
    RequestRefusal,
    /// Sandbox refused or could not prove execution controls.
    SandboxRefusal,
    /// Cargo completed with a compilation failure.
    CargoFailure,
    /// Delivered toolchain identity or invocation was invalid.
    ToolchainFailure,
    /// Cargo JSON output was malformed or ambiguous.
    MalformedCargoOutput,
    /// Immutable artifact publication or verification failed.
    ArtifactStoreFailure,
}

/// Bounded, sanitized build failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildFailure {
    /// Machine-stable category.
    pub kind: FailureKind,
    /// Bounded diagnostic with control characters removed.
    pub diagnostic: String,
}

impl BuildFailure {
    pub(crate) fn new(kind: FailureKind, diagnostic: impl AsRef<str>) -> Self {
        let diagnostic = diagnostic
            .as_ref()
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(MAX_DIAGNOSTIC)
            .collect();
        Self { kind, diagnostic }
    }
    pub(crate) fn request(v: impl AsRef<str>) -> Self {
        Self::new(FailureKind::RequestRefusal, v)
    }
    pub(crate) fn toolchain(v: impl AsRef<str>) -> Self {
        Self::new(FailureKind::ToolchainFailure, v)
    }
    pub(crate) fn artifact(v: impl AsRef<str>) -> Self {
        Self::new(FailureKind::ArtifactStoreFailure, v)
    }
}

impl fmt::Display for BuildFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.diagnostic)
    }
}
impl Error for BuildFailure {}
