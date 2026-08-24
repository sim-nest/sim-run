#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Structured, offline, sandboxed construction of immutable native candidates.
//!
//! Callers provide identities and preopened mounts, never commands or host paths.
//! The build policy is fixed here; execution remains behind `sim-lib-exec`.

mod artifact;
mod build;
mod error;
mod request;

pub use artifact::{ArtifactCandidate, ArtifactStore};
pub use build::{BuildMounts, NativeBuilder};
pub use error::{BuildFailure, FailureKind};
pub use request::{NativeBuildRequest, ToolchainIdentity};
