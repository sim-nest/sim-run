#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Structured, offline, sandboxed construction of immutable native candidates.
//!
//! Callers provide identities and preopened mounts, never commands or host paths.
//! The build policy is fixed here; execution remains behind `sim-lib-exec`.

mod activation;
mod admission;
mod artifact;
mod build;
mod compatibility;
mod error;
mod preflight;
mod request;

pub use activation::{
    ActivationAudit, ActivationFailure, ActivationReceipt, ActivationRequest, ActivationService,
    ActivationStatus,
};
pub use admission::{
    AdmissionFailure, AdmissionReceipt, AdmissionRequest, AdmissionService, HotloadGeneration,
};
pub use artifact::{ArtifactCandidate, ArtifactStore};
pub use build::{BuildMounts, NativeBuilder};
pub use compatibility::{CompatibilityPolicy, CompatibilityReport};
pub use error::{BuildFailure, FailureKind};
pub use preflight::{AchievedLimits, CandidateTestResult, PreflightLimits};
pub use request::{NativeBuildRequest, ToolchainIdentity};
