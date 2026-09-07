// conformance: admission rejects incompatible or insufficiently proven native candidates.

use std::{fmt, sync::Arc};

use sim_kernel::{
    ContentId, Cx, Datum, Export, HandleSeed, LibBootReceipt, LibManifest, LibSource,
    NumberLiteral, RuntimeId, Symbol,
};
use sim_run_loaders::{LoadRequest, LoaderKind, LoaderPort};
use sim_storage_port::HostDirPort;

use crate::{
    AchievedLimits, ArtifactCandidate, ArtifactContentId, CandidateTestResult, CompatibilityPolicy,
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
    pub content: ArtifactContentId,
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
    pub artifact: ArtifactContentId,
    /// Current managed generation, for replacement.
    pub current_generation: Option<ArtifactContentId>,
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
            .read(&[hex(&request.candidate.content.content_id().bytes)])
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
        })?;
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
    content: &ArtifactContentId,
    bytes: &[u8],
) -> Result<(), AdmissionFailure> {
    if sim_run_loaders::bytes_from_source(source)
        .map_err(|error| AdmissionFailure(format!("candidate source is malformed: {error}")))?
        .as_deref()
        == Some(bytes)
    {
        return Ok(());
    }
    let expected_hex = hex(&content.content_id().bytes);
    let addressed = match sim_run_loaders::content_address_payload(source) {
        Some(sim_kernel::Datum::Bytes(digest)) => digest.as_slice() == content.content_id().bytes,
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
    artifact: &'a ArtifactContentId,
    current: Option<&'a ArtifactContentId>,
    manifest: &'a LibManifest,
    compatibility: &'a CompatibilityReport,
    loader: &'a Symbol,
    dependencies: &'a [Symbol],
    tests: &'a [CandidateTestResult],
    limits: &'a AchievedLimits,
}

pub(crate) fn admission_receipt_datum(receipt: &AdmissionReceipt) -> Datum {
    receipt_datum(&AdmissionIdentity {
        artifact: &receipt.artifact,
        current: receipt.current_generation.as_ref(),
        manifest: &receipt.manifest,
        compatibility: &receipt.compatibility,
        loader: &receipt.loader,
        dependencies: &receipt.dependencies,
        tests: &receipt.tests,
        limits: &receipt.achieved_limits,
    })
}

fn receipt_id(identity: &AdmissionIdentity<'_>) -> Result<ContentId, AdmissionFailure> {
    receipt_datum(identity)
        .content_id()
        .map_err(|error| AdmissionFailure(format!("admission identity is not canonical: {error}")))
}

fn receipt_datum(identity: &AdmissionIdentity<'_>) -> Datum {
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
    Datum::Node {
        tag: Symbol::qualified("hotload", "AdmissionIdentityV2"),
        fields: vec![
            (Symbol::new("artifact"), artifact_content_datum(artifact)),
            (
                Symbol::new("current-generation"),
                current.map(artifact_content_datum).unwrap_or(Datum::Nil),
            ),
            (Symbol::new("manifest"), manifest_datum(manifest)),
            (
                Symbol::new("compatibility"),
                compatibility_datum(compatibility),
            ),
            (Symbol::new("loader"), Datum::Symbol((*loader).clone())),
            (
                Symbol::new("dependencies"),
                Datum::Set(dependencies.iter().cloned().map(Datum::Symbol).collect()),
            ),
            (
                Symbol::new("tests"),
                Datum::List(tests.iter().map(test_result_datum).collect()),
            ),
            (Symbol::new("achieved-limits"), limits_datum(limits)),
        ],
    }
}

pub(crate) fn artifact_content_datum(artifact: &ArtifactContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("hotload", "ArtifactBytesIdentityV1"),
        fields: vec![(
            Symbol::new("content"),
            content_id_datum(artifact.content_id()),
        )],
    }
}

pub(crate) fn content_id_datum(id: &ContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("core", "ContentId"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(id.algorithm.clone()),
            ),
            (Symbol::new("bytes"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}

pub(crate) fn compatibility_datum(report: &CompatibilityReport) -> Datum {
    let export = |(kind, symbol): &(sim_kernel::ExportKind, Symbol)| Datum::Node {
        tag: Symbol::qualified("hotload", "Export"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(kind.symbol().clone())),
            (Symbol::new("symbol"), Datum::Symbol(symbol.clone())),
        ],
    };
    Datum::Node {
        tag: Symbol::qualified("hotload", "CompatibilityReportV1"),
        fields: vec![
            (
                Symbol::new("policy"),
                report.policy.map_or(Datum::Nil, |policy| {
                    Datum::Symbol(Symbol::qualified(
                        "hotload-compatibility",
                        match policy {
                            CompatibilityPolicy::Exact => "exact",
                            CompatibilityPolicy::Additive => "additive",
                        },
                    ))
                }),
            ),
            (
                Symbol::new("candidate-exports"),
                Datum::List(report.candidate_exports.iter().map(export).collect()),
            ),
            (
                Symbol::new("added-exports"),
                Datum::List(report.added_exports.iter().map(export).collect()),
            ),
        ],
    }
}

fn test_result_datum(result: &CandidateTestResult) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("hotload", "CandidateTestResultV1"),
        fields: vec![
            (Symbol::new("symbol"), Datum::Symbol(result.symbol.clone())),
            (Symbol::new("passed"), Datum::Bool(result.passed)),
            (
                Symbol::new("detail"),
                result
                    .detail
                    .clone()
                    .map(Datum::String)
                    .unwrap_or(Datum::Nil),
            ),
        ],
    }
}

fn manifest_datum(manifest: &LibManifest) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("hotload", "LibManifestV1"),
        fields: vec![
            (Symbol::new("id"), Datum::Symbol(manifest.id.clone())),
            (
                Symbol::new("version"),
                Datum::String(manifest.version.0.clone()),
            ),
            (
                Symbol::new("abi-major"),
                u64_datum(u64::from(manifest.abi.major)),
            ),
            (
                Symbol::new("abi-minor"),
                u64_datum(u64::from(manifest.abi.minor)),
            ),
            (
                Symbol::new("target"),
                Datum::Symbol(manifest.target.to_symbol()),
            ),
            (
                Symbol::new("requires"),
                Datum::Set(
                    manifest
                        .requires
                        .iter()
                        .map(|dependency| Datum::Node {
                            tag: Symbol::qualified("hotload", "DependencyV1"),
                            fields: vec![
                                (Symbol::new("id"), Datum::Symbol(dependency.id.clone())),
                                (
                                    Symbol::new("minimum-version"),
                                    dependency
                                        .minimum_version
                                        .as_ref()
                                        .map(|version| Datum::String(version.0.clone()))
                                        .unwrap_or(Datum::Nil),
                                ),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                Symbol::new("capabilities"),
                Datum::Set(
                    manifest
                        .capabilities
                        .iter()
                        .map(|capability| Datum::String(capability.as_str().to_owned()))
                        .collect(),
                ),
            ),
            (
                Symbol::new("exports"),
                Datum::Set(manifest.exports.iter().map(manifest_export_datum).collect()),
            ),
        ],
    }
}

fn manifest_export_datum(export: &Export) -> Datum {
    let stable_id = match export {
        Export::Class { class_id, .. } => class_id.map(|id| runtime_id_datum(RuntimeId::Class(id))),
        Export::Function { function_id, .. } => {
            function_id.map(|id| runtime_id_datum(RuntimeId::Function(id)))
        }
        Export::Macro { macro_id, .. } => macro_id.map(|id| runtime_id_datum(RuntimeId::Macro(id))),
        Export::Shape { shape_id, .. } => shape_id.map(|id| runtime_id_datum(RuntimeId::Shape(id))),
        Export::Codec { codec_id, .. } => codec_id.map(|id| runtime_id_datum(RuntimeId::Codec(id))),
        Export::NumberDomain {
            number_domain_id, ..
        } => number_domain_id.map(|id| runtime_id_datum(RuntimeId::NumberDomain(id))),
        Export::Site { runtime_id, .. } => runtime_id.map(runtime_id_datum),
        Export::Value { .. } | Export::Open { .. } => None,
    };
    Datum::Node {
        tag: Symbol::qualified("hotload", "ManifestExportV1"),
        fields: vec![
            (
                Symbol::new("kind"),
                Datum::Symbol(export.kind_symbol().symbol().clone()),
            ),
            (
                Symbol::new("symbol"),
                Datum::Symbol(export.symbol().clone()),
            ),
            (Symbol::new("stable-id"), stable_id.unwrap_or(Datum::Nil)),
        ],
    }
}

fn runtime_id_datum(id: RuntimeId) -> Datum {
    let (kind, value) = match id {
        RuntimeId::Class(value) => ("class", Some(u64::from(value.0))),
        RuntimeId::Function(value) => ("function", Some(u64::from(value.0))),
        RuntimeId::Macro(value) => ("macro", Some(u64::from(value.0))),
        RuntimeId::Shape(value) => ("shape", Some(u64::from(value.0))),
        RuntimeId::Codec(value) => ("codec", Some(u64::from(value.0))),
        RuntimeId::NumberDomain(value) => ("number-domain", Some(u64::from(value.0))),
        RuntimeId::Site(value) => ("site", Some(u64::from(value.0))),
        RuntimeId::Value => ("value", None),
    };
    Datum::Node {
        tag: Symbol::qualified("core", "RuntimeId"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(Symbol::new(kind))),
            (
                Symbol::new("value"),
                value.map(u64_datum).unwrap_or(Datum::Nil),
            ),
        ],
    }
}

fn limits_datum(limits: &AchievedLimits) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("hotload", "AchievedLimitsV1"),
        fields: vec![
            (Symbol::new("tests-run"), usize_datum(limits.tests_run)),
            (
                Symbol::new("max-events-observed"),
                usize_datum(limits.max_events_observed),
            ),
            (
                Symbol::new("max-detail-chars-observed"),
                usize_datum(limits.max_detail_chars_observed),
            ),
        ],
    }
}

fn usize_datum(value: usize) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "usize"),
        canonical: value.to_string(),
    })
}

fn u64_datum(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
