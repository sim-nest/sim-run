// conformance: admission rejects incompatible or insufficiently proven native candidates.

use std::{fmt, sync::Arc};

use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Cx, HandleSeed, LibBootReceipt, LibManifest, LibSource, Symbol};
use sim_run_loaders::{LoadRequest, LoaderKind, LoaderPort};
use sim_storage_port::HostDirPort;

use crate::{
    AchievedLimits, ArtifactCandidate, CandidateTestResult, CompatibilityPolicy,
    CompatibilityReport, PreflightLimits,
    artifact::{content_id, hex},
    compatibility, preflight,
};

/// Latest completed generation recorded by the hotload journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotloadGeneration {
    /// Managed library identity.
    pub library: Symbol,
    /// Content identity installed by that completed activation.
    pub content: ContentId,
    /// Manifest bound to that completed activation.
    pub manifest: LibManifest,
}

/// Complete, data-only request for isolated candidate admission.
pub struct AdmissionRequest<'a> {
    /// Candidate produced by the immutable build stage.
    pub candidate: &'a ArtifactCandidate,
    /// Loader kind selected from the installed platform capsule.
    pub loader_kind: LoaderKind,
    /// Exact loader source whose bytes are addressed by `candidate.content`.
    pub source: LibSource,
    /// Latest completed managed generation, if any.
    pub latest_generation: Option<&'a HotloadGeneration>,
    /// Replacement export policy.
    pub compatibility: CompatibilityPolicy,
    /// Boot receipts for exactly the target's dependencies.
    pub dependency_receipts: &'a [LibBootReceipt],
    /// Seed for the fresh shadow context's handle namespace.
    pub shadow_seed: HandleSeed,
    /// Test evidence bounds.
    pub limits: PreflightLimits,
}

/// Content-identified proof that a candidate passed admission away from live state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionReceipt {
    /// Identity of this receipt's canonical content.
    pub content: ContentId,
    /// Candidate artifact identity.
    pub artifact: ContentId,
    /// Current managed generation, for replacement.
    pub current_generation: Option<ContentId>,
    /// Candidate manifest inspected through the loader port.
    pub manifest: LibManifest,
    /// Compatibility evidence.
    pub compatibility: CompatibilityReport,
    /// Stable loader identity.
    pub loader: Symbol,
    /// Sorted dependency symbols used to seed the shadow.
    pub dependencies: Vec<Symbol>,
    /// Candidate-declared conformance results.
    pub tests: Vec<CandidateTestResult>,
    /// Limits achieved by candidate execution.
    pub achieved_limits: AchievedLimits,
}

/// Closed admission refusal; no operation mutates the live context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionFailure(pub String);
impl fmt::Display for AdmissionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for AdmissionFailure {}

/// Admission coordinator over the existing immutable store and loader membrane.
pub struct AdmissionService<'a> {
    artifacts: &'a dyn HostDirPort,
    loader: Arc<dyn LoaderPort>,
}

impl<'a> AdmissionService<'a> {
    /// Composes existing storage and loader ports without native or ABI access.
    pub fn new(artifacts: &'a dyn HostDirPort, loader: Arc<dyn LoaderPort>) -> Self {
        Self { artifacts, loader }
    }

    /// Inspects, checks, and exercises a candidate in a fresh shadow context.
    pub fn admit(
        &self,
        live: &Cx,
        request: AdmissionRequest<'_>,
    ) -> Result<AdmissionReceipt, AdmissionFailure> {
        let bytes = self
            .artifacts
            .read(&[hex(&request.candidate.content.bytes)])
            .map_err(|error| AdmissionFailure(format!("artifact re-read failed: {error}")))?;
        if content_id(&bytes) != request.candidate.content {
            return Err(AdmissionFailure(
                "artifact content identity changed before admission".into(),
            ));
        }
        require_candidate_source(&request.source, &request.candidate.content, &bytes)?;

        let current = live
            .registry()
            .manifest_by_symbol(&request.candidate.expected_library);
        let current_generation = match (current, request.latest_generation) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err(AdmissionFailure(
                    "completed generation exists but library is absent".into(),
                ));
            }
            (Some(_), None) => {
                return Err(AdmissionFailure(
                    "loaded library is not named by a completed hotload receipt".into(),
                ));
            }
            (Some(_), Some(generation))
                if generation.library != request.candidate.expected_library =>
            {
                return Err(AdmissionFailure(
                    "completed generation names a different library".into(),
                ));
            }
            (Some(loaded), Some(generation)) if &generation.manifest != loaded => {
                return Err(AdmissionFailure(
                    "loaded library does not match the latest completed generation".into(),
                ));
            }
            (Some(_), Some(generation)) => Some(generation.content.clone()),
        };

        if let Some(loaded) = live.registry().lib(&request.candidate.expected_library) {
            let mut probe = live.registry().clone();
            if let Err(sim_kernel::Error::LibHasDependents { mut dependents, .. }) =
                probe.unload(loaded.id)
            {
                dependents.sort();
                return Err(AdmissionFailure(format!(
                    "loaded dependents refuse replacement: {}",
                    dependents
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }

        let mut inspection = fresh_context(live, request.shadow_seed, &[])?;
        let manifest = self
            .loader
            .inspect(
                &mut inspection,
                &LoadRequest {
                    kind: request.loader_kind.clone(),
                    source: clone_source(&request.source)?,
                },
            )
            .map_err(|error| AdmissionFailure(format!("loader inspection failed: {error}")))?
            .ok_or_else(|| AdmissionFailure("loader cannot inspect candidate manifest".into()))?;
        let compatibility = compatibility::compare(
            &request.candidate.expected_library,
            &manifest,
            current,
            request.compatibility,
        )
        .map_err(AdmissionFailure)?;

        let mut dependencies = manifest
            .requires
            .iter()
            .map(|dependency| dependency.id.clone())
            .collect::<Vec<_>>();
        dependencies.sort();
        let mut supplied = request
            .dependency_receipts
            .iter()
            .map(|receipt| receipt.manifest.id.clone())
            .collect::<Vec<_>>();
        supplied.sort();
        if supplied != dependencies {
            return Err(AdmissionFailure(
                "dependency boot receipts do not exactly match candidate requirements".into(),
            ));
        }
        for receipt in request.dependency_receipts {
            let live_receipt = live
                .registry()
                .boot_receipt(
                    receipt.lib_id,
                    receipt.requested_source.clone(),
                    receipt.resolved_source.clone(),
                )
                .ok_or_else(|| {
                    AdmissionFailure(format!("dependency {} is not loaded", receipt.manifest.id))
                })?;
            if &live_receipt != receipt {
                return Err(AdmissionFailure(format!(
                    "dependency boot receipt for {} is stale",
                    receipt.manifest.id
                )));
            }
        }

        let mut shadow = fresh_context(live, request.shadow_seed, &manifest.capabilities)?;
        *shadow.registry_mut() = live.registry().subset_for_libs(&dependencies);
        let outcome = self
            .loader
            .realize(
                &mut shadow,
                LoadRequest {
                    kind: request.loader_kind.clone(),
                    source: clone_source(&request.source)?,
                },
            )
            .map_err(|error| AdmissionFailure(format!("candidate realization failed: {error}")))?;
        if outcome.manifest != manifest || outcome.library.manifest() != manifest {
            return Err(AdmissionFailure(
                "realized candidate manifest differs from inspected manifest".into(),
            ));
        }
        shadow
            .load_lib(outcome.library.as_ref())
            .map_err(|error| AdmissionFailure(format!("shadow load failed: {error}")))?;
        let symbols = shadow
            .registry()
            .tests_for_lib(&manifest.id)
            .unwrap_or_default()
            .to_vec();
        if symbols.len() > request.limits.max_tests {
            return Err(AdmissionFailure(
                "candidate declared too many conformance tests".into(),
            ));
        }
        let mut tests = Vec::with_capacity(symbols.len());
        let mut max_events = 0;
        let mut max_detail = 0;
        for symbol in symbols {
            let test = shadow
                .registry()
                .test_by_symbol(&symbol)
                .cloned()
                .ok_or_else(|| AdmissionFailure(format!("candidate test {symbol} disappeared")))?;
            let report = test.run(&mut shadow).map_err(|error| {
                AdmissionFailure(format!("candidate test {symbol} failed to run: {error}"))
            })?;
            max_events = max_events.max(report.events.len());
            max_detail = max_detail.max(
                report
                    .detail
                    .as_deref()
                    .map(str::chars)
                    .map(Iterator::count)
                    .unwrap_or(0)
                    .min(request.limits.max_detail_chars),
            );
            let result =
                preflight::bounded_result(report, request.limits).map_err(AdmissionFailure)?;
            if !result.passed {
                return Err(AdmissionFailure(format!(
                    "candidate test {} did not pass",
                    result.symbol
                )));
            }
            tests.push(result);
        }
        tests.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        let achieved_limits = AchievedLimits {
            tests_run: tests.len(),
            max_events_observed: max_events,
            max_detail_chars_observed: max_detail,
        };
        let loader = request.loader_kind.symbol().clone();
        let receipt_content = receipt_id(&AdmissionIdentity {
            artifact: &request.candidate.content,
            current: current_generation.as_ref(),
            manifest: &manifest,
            compatibility: &compatibility,
            loader: &loader,
            dependencies: &dependencies,
            tests: &tests,
            limits: &achieved_limits,
        });
        Ok(AdmissionReceipt {
            content: receipt_content,
            artifact: request.candidate.content.clone(),
            current_generation,
            manifest,
            compatibility,
            loader,
            dependencies,
            tests,
            achieved_limits,
        })
    }
}

fn fresh_context(
    live: &Cx,
    seed: HandleSeed,
    capabilities: &[sim_kernel::CapabilityName],
) -> Result<Cx, AdmissionFailure> {
    let (mut context, seat) = Cx::new_seated(live.eval_policy_ref(), live.factory_ref(), seed);
    context.set_control_policy(live.control_policy_ref());
    for capability in capabilities {
        seat.grant(&mut context, capability.clone())
            .map_err(|error| {
                AdmissionFailure(format!(
                    "cannot grant shadow capability {capability}: {error}"
                ))
            })?;
    }
    Ok(context)
}

fn require_candidate_source(
    source: &LibSource,
    content: &ContentId,
    bytes: &[u8],
) -> Result<(), AdmissionFailure> {
    if sim_run_loaders::bytes_from_source(source)
        .map_err(|error| AdmissionFailure(format!("candidate source is malformed: {error}")))?
        .as_deref()
        == Some(bytes)
    {
        return Ok(());
    }
    let expected_hex = hex(&content.bytes);
    let addressed = match sim_run_loaders::content_address_payload(source) {
        Some(sim_kernel::Datum::Bytes(digest)) => digest.as_slice() == content.bytes,
        Some(sim_kernel::Datum::String(digest)) => digest == &expected_hex,
        _ => false,
    };
    if addressed {
        Ok(())
    } else {
        Err(AdmissionFailure(
            "loader source does not name the verified candidate content".into(),
        ))
    }
}

fn clone_source(source: &LibSource) -> Result<LibSource, AdmissionFailure> {
    match source {
        LibSource::Symbol(value) => Ok(LibSource::Symbol(value.clone())),
        LibSource::Open { kind, payload } => Ok(LibSource::Open {
            kind: kind.clone(),
            payload: payload.clone(),
        }),
        LibSource::Host(_) => Err(AdmissionFailure(
            "host library sources cannot be admitted".into(),
        )),
    }
}

struct AdmissionIdentity<'a> {
    artifact: &'a ContentId,
    current: Option<&'a ContentId>,
    manifest: &'a LibManifest,
    compatibility: &'a CompatibilityReport,
    loader: &'a Symbol,
    dependencies: &'a [Symbol],
    tests: &'a [CandidateTestResult],
    limits: &'a AchievedLimits,
}

fn receipt_id(identity: &AdmissionIdentity<'_>) -> ContentId {
    let AdmissionIdentity {
        artifact,
        current,
        manifest,
        compatibility,
        loader,
        dependencies,
        tests,
        limits,
    } = identity;
    let canonical = format!(
        "artifact={artifact:?}\ncurrent={current:?}\nmanifest={manifest:?}\ncompatibility={compatibility:?}\nloader={loader}\ndependencies={dependencies:?}\ntests={tests:?}\nlimits={limits:?}\n"
    );
    ContentId::from_bytes(
        Symbol::qualified("core", "sha256"),
        Sha256::digest(canonical.as_bytes()).into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::Datum;

    #[test]
    fn artifact_source_must_bind_the_verified_bytes_or_digest() {
        let bytes = b"candidate";
        let content = content_id(bytes);
        let direct = sim_run_loaders::bytes_source(bytes);
        assert!(require_candidate_source(&direct, &content, bytes).is_ok());

        let addressed =
            sim_run_loaders::content_address_source(Datum::Bytes(content.bytes.to_vec()));
        assert!(require_candidate_source(&addressed, &content, bytes).is_ok());
        assert!(require_candidate_source(&direct, &content, b"mutated").is_err());
    }

    #[test]
    fn host_sources_cannot_cross_the_admission_membrane() {
        struct HostLib;
        impl sim_kernel::Lib for HostLib {
            fn manifest(&self) -> LibManifest {
                unreachable!("host source is rejected before manifest access")
            }
            fn load(
                &self,
                _cx: &mut sim_kernel::LoadCx,
                _linker: &mut sim_kernel::Linker,
            ) -> sim_kernel::Result<()> {
                unreachable!("host source is rejected before native behavior")
            }
        }
        assert!(clone_source(&LibSource::Host(Box::new(HostLib))).is_err());
    }
}
