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
mod surface;

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
pub use surface::{
    HotloadLib, HotloadOperation, HotloadPort, HotloadRecord, hotload_capability,
    hotload_lib_symbol, hotload_operation_cards_symbol, hotload_operation_symbols,
};

#[cfg(test)]
mod ownership_tests {
    #[test]
    fn hotload_surface_does_not_implement_host_effects() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["libloading", "native-open", "journal-sqlite"] {
            assert!(
                !manifest.contains(forbidden),
                "forbidden dependency: {forbidden}"
            );
        }
        let surface = include_str!("surface.rs");
        for forbidden in ["std::process", "std::fs", "Command::new", "OpenOptions"] {
            assert!(
                !surface.contains(forbidden),
                "host implementation escaped its owner: {forbidden}"
            );
        }
    }
}
