// conformance: native builds use sealed offline inputs and publish immutable artifacts.

use crate::{
    ArtifactCandidate, ArtifactContentId, BuildFailure, BuildReceiptId, FailureKind,
    NativeBuildRequest, SandboxReportId, artifact::ArtifactStore,
};
use serde::Deserialize;
use sim_kernel::{Datum, Symbol};
use sim_lib_exec::{
    ArgAtom, MountAccess, ProcessCancellation, ProgramRef, SandboxAttempt, SandboxControl,
    SandboxEvidence, SandboxLauncher, SandboxLimits, SandboxMount, SandboxPolicy, SandboxReport,
    SandboxRequest, SandboxRequirement, SealedBindings,
};
use sim_storage_port::HostDirPort;
use std::collections::BTreeMap;

const SOURCE: &str = "/source";
const TOOLCHAIN: &str = "/toolchain";
const TARGET: &str = "/target";

/// Preopened byte mounts used around sandbox execution.
pub struct BuildMounts<'a> {
    /// Sealed source tree.
    pub source: &'a dyn HostDirPort,
    /// Writable sandbox target tree.
    pub target: &'a dyn HostDirPort,
    /// Immutable artifact store.
    pub artifacts: &'a dyn HostDirPort,
}

/// Native build policy bound to one trusted launcher.
pub struct NativeBuilder<'a> {
    launcher: &'a dyn SandboxLauncher,
}
impl<'a> NativeBuilder<'a> {
    /// Creates a builder over a boot-selected sandbox launcher.
    pub fn new(launcher: &'a dyn SandboxLauncher) -> Self {
        Self { launcher }
    }

    /// Validates, executes, selects, and immutably publishes one candidate.
    pub fn build(
        &self,
        request: &NativeBuildRequest,
        mounts: BuildMounts<'_>,
        cancellation: &ProcessCancellation,
    ) -> Result<ArtifactCandidate, BuildFailure> {
        request.validate_fields()?;
        let manifest_bytes = mounts
            .source
            .read(&split(&request.manifest)?)
            .map_err(|e| BuildFailure::request(e.to_string()))?;
        let manifest: toml::Value = toml::from_str(
            std::str::from_utf8(&manifest_bytes)
                .map_err(|_| BuildFailure::request("manifest is not UTF-8"))?,
        )
        .map_err(|e| BuildFailure::request(e.to_string()))?;
        validate_manifest(&manifest, request, mounts.source)?;
        let sandbox = sandbox_request(request)?;
        let result = match self.launcher.launch(&sandbox, cancellation) {
            SandboxAttempt::Completed(v) if v.report.proves_required(&sandbox.policy) => v,
            SandboxAttempt::Completed(_) => {
                return Err(BuildFailure::new(
                    FailureKind::SandboxRefusal,
                    "required controls were not achieved",
                ));
            }
            SandboxAttempt::Refused(v) | SandboxAttempt::Unknown(v) => {
                return Err(BuildFailure::new(FailureKind::SandboxRefusal, v.reason));
            }
            SandboxAttempt::Stopped(_) => {
                return Err(BuildFailure::new(
                    FailureKind::SandboxRefusal,
                    "sandbox stopped before completion",
                ));
            }
        };
        if result.exit_code != 0 {
            return Err(BuildFailure::new(
                FailureKind::CargoFailure,
                String::from_utf8_lossy(&result.stderr),
            ));
        }
        let artifact_path = select_artifact(&result.stdout, &request.package)?;
        let bytes = mounts
            .target
            .read(&split_target(&artifact_path)?)
            .map_err(|e| BuildFailure::artifact(e.to_string()))?;
        let (content, cache_hit) = ArtifactStore::new(mounts.artifacts).put(&bytes)?;
        let report = sandbox_report_id(&result.report)?;
        let receipt = build_receipt_id(request, &content, &report)?;
        Ok(ArtifactCandidate {
            content,
            bytes: bytes.len() as u64,
            expected_library: request.expected_library.clone(),
            sandbox_report: report,
            build_receipt: receipt,
            cache_hit,
        })
    }
}

fn sandbox_report_id(report: &SandboxReport) -> Result<SandboxReportId, BuildFailure> {
    let datum = Datum::Node {
        tag: Symbol::qualified("hotload", "SandboxReportIdentityV1"),
        fields: vec![
            (
                Symbol::new("launcher"),
                Datum::String(report.launcher.clone()),
            ),
            (
                Symbol::new("controls"),
                Datum::Set(report.controls.iter().map(sandbox_evidence_datum).collect()),
            ),
            (
                Symbol::new("limit-hits"),
                Datum::List(
                    report
                        .limit_hits
                        .iter()
                        .cloned()
                        .map(Datum::String)
                        .collect(),
                ),
            ),
            (
                Symbol::new("cleanup"),
                Datum::String(report.cleanup.clone()),
            ),
        ],
    };
    datum.content_id().map(SandboxReportId).map_err(|error| {
        BuildFailure::artifact(format!("sandbox report is not canonical: {error}"))
    })
}

fn sandbox_evidence_datum(evidence: &SandboxEvidence) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("hotload", "SandboxEvidenceV1"),
        fields: vec![
            (
                Symbol::new("control"),
                Datum::Symbol(Symbol::qualified(
                    "sandbox-control",
                    sandbox_control_name(evidence.control),
                )),
            ),
            (Symbol::new("achieved"), Datum::Bool(evidence.achieved)),
            (
                Symbol::new("detail"),
                Datum::String(evidence.detail.clone()),
            ),
        ],
    }
}

fn sandbox_control_name(control: SandboxControl) -> &'static str {
    match control {
        SandboxControl::Network => "network",
        SandboxControl::Mounts => "mounts",
        SandboxControl::Root => "root",
        SandboxControl::Environment => "environment",
        SandboxControl::Identity => "identity",
        SandboxControl::Cpu => "cpu",
        SandboxControl::Memory => "memory",
        SandboxControl::WallTime => "wall-time",
        SandboxControl::ProcessCount => "process-count",
        SandboxControl::FileCount => "file-count",
        SandboxControl::FileBytes => "file-bytes",
        SandboxControl::Output => "output",
        SandboxControl::Stdin => "stdin",
        SandboxControl::ProcessTree => "process-tree",
    }
}

fn build_receipt_id(
    request: &NativeBuildRequest,
    artifact: &ArtifactContentId,
    sandbox_report: &SandboxReportId,
) -> Result<BuildReceiptId, BuildFailure> {
    let datum = Datum::Node {
        tag: Symbol::qualified("hotload", "BuildReceiptIdentityV2"),
        fields: vec![
            (
                Symbol::new("source-mount"),
                Datum::String(request.source_mount.clone()),
            ),
            (
                Symbol::new("manifest"),
                Datum::String(request.manifest.clone()),
            ),
            (
                Symbol::new("package"),
                Datum::String(request.package.clone()),
            ),
            (
                Symbol::new("features"),
                Datum::Set(
                    request
                        .features
                        .iter()
                        .cloned()
                        .map(Datum::String)
                        .collect(),
                ),
            ),
            (
                Symbol::new("expected-library"),
                Datum::Symbol(request.expected_library.clone()),
            ),
            (
                Symbol::new("toolchain"),
                Datum::Node {
                    tag: Symbol::qualified("hotload", "ToolchainIdentityV1"),
                    fields: vec![
                        (
                            Symbol::new("content"),
                            Datum::String(request.toolchain.content.clone()),
                        ),
                        (
                            Symbol::new("cargo-program"),
                            Datum::String(request.toolchain.cargo_program.clone()),
                        ),
                        (
                            Symbol::new("environment"),
                            Datum::Set(
                                request
                                    .toolchain
                                    .environment
                                    .iter()
                                    .map(|(name, value)| Datum::Node {
                                        tag: Symbol::qualified(
                                            "hotload",
                                            "ToolchainEnvironmentBindingV1",
                                        ),
                                        fields: vec![
                                            (Symbol::new("name"), Datum::String(name.clone())),
                                            (Symbol::new("value"), Datum::String(value.clone())),
                                        ],
                                    })
                                    .collect(),
                            ),
                        ),
                    ],
                },
            ),
            (
                Symbol::new("artifact"),
                crate::admission::content_id_datum(artifact.content_id()),
            ),
            (
                Symbol::new("sandbox-report"),
                crate::admission::content_id_datum(sandbox_report.content_id()),
            ),
        ],
    };
    datum
        .content_id()
        .map(BuildReceiptId)
        .map_err(|error| BuildFailure::artifact(format!("build receipt is not canonical: {error}")))
}

fn validate_manifest(
    value: &toml::Value,
    request: &NativeBuildRequest,
    source: &dyn HostDirPort,
) -> Result<(), BuildFailure> {
    let package = value
        .get("package")
        .and_then(|v| v.get("name"))
        .and_then(toml::Value::as_str);
    if package != Some(&request.package) {
        return Err(BuildFailure::request(
            "manifest package does not match request",
        ));
    }
    let kinds = value
        .get("lib")
        .and_then(|v| v.get("crate-type"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| BuildFailure::request("manifest must declare a cdylib"))?;
    if !kinds.iter().any(|v| v.as_str() == Some("cdylib")) {
        return Err(BuildFailure::request("manifest must declare a cdylib"));
    }
    if source
        .metadata(&["Cargo.lock".into()])
        .map_err(|e| BuildFailure::request(e.to_string()))?
        .is_none()
    {
        return Err(BuildFailure::request("locked manifest requires Cargo.lock"));
    }
    for table in ["dependencies", "build-dependencies", "dev-dependencies"] {
        if let Some(deps) = value.get(table).and_then(toml::Value::as_table) {
            for dep in deps.values() {
                if let Some(t) = dep.as_table() {
                    if t.contains_key("git") || t.contains_key("registry") {
                        return Err(BuildFailure::request(
                            "URL and git dependencies are forbidden",
                        ));
                    }
                    if let Some(path) = t.get("path").and_then(toml::Value::as_str) {
                        split(path)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn sandbox_request(request: &NativeBuildRequest) -> Result<SandboxRequest, BuildFailure> {
    let mut argv = vec![
        "build",
        "--locked",
        "--offline",
        "--message-format=json-render-diagnostics",
        "--manifest-path",
        "/source/",
    ];
    let manifest_arg = format!("{SOURCE}/{}", request.manifest);
    let mut atoms = argv.drain(..5).map(atom).collect::<Result<Vec<_>, _>>()?;
    atoms.push(atom(&manifest_arg)?);
    atoms.extend([
        atom("--package")?,
        atom(&request.package)?,
        atom("--target-dir")?,
        atom(TARGET)?,
    ]);
    if !request.features.is_empty() {
        atoms.extend([
            atom("--features")?,
            atom(
                &request
                    .features
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            )?,
        ]);
    }
    let requirements = all_controls()
        .into_iter()
        .map(|c| (c, SandboxRequirement::Required));
    let policy = SandboxPolicy::new(
        requirements,
        vec![
            SandboxMount {
                source: request.source_mount.clone(),
                guest_path: SOURCE.into(),
                access: MountAccess::ReadOnly,
            },
            SandboxMount {
                source: request.toolchain.content.clone(),
                guest_path: TOOLCHAIN.into(),
                access: MountAccess::ReadOnly,
            },
            SandboxMount {
                source: "hotload-target".into(),
                guest_path: TARGET.into(),
                access: MountAccess::Writable,
            },
        ],
        SandboxLimits {
            cpu_seconds: 300,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            wall_time_ms: 360_000,
            process_count: 64,
            file_count: 100_000,
            file_bytes: 2 * 1024 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024,
            stdin_bytes: 1,
        },
    )
    .map_err(|e| BuildFailure::request(e.to_string()))?;
    let environment = SealedBindings::literals(
        request
            .toolchain
            .environment
            .clone()
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(|e| BuildFailure::toolchain(e.to_string()))?;
    SandboxRequest::new(
        ProgramRef::new(request.toolchain.cargo_program.clone())
            .map_err(|e| BuildFailure::toolchain(e.to_string()))?,
        atoms,
        environment,
        vec![],
        policy,
    )
    .map_err(|e| BuildFailure::request(e.to_string()))
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;

fn atom(v: &str) -> Result<ArgAtom, BuildFailure> {
    ArgAtom::new(v).map_err(|e| BuildFailure::request(e.to_string()))
}
fn all_controls() -> [SandboxControl; 14] {
    [
        SandboxControl::Network,
        SandboxControl::Mounts,
        SandboxControl::Root,
        SandboxControl::Environment,
        SandboxControl::Identity,
        SandboxControl::Cpu,
        SandboxControl::Memory,
        SandboxControl::WallTime,
        SandboxControl::ProcessCount,
        SandboxControl::FileCount,
        SandboxControl::FileBytes,
        SandboxControl::Output,
        SandboxControl::Stdin,
        SandboxControl::ProcessTree,
    ]
}

#[derive(Deserialize)]
struct Message {
    reason: String,
    package_id: Option<String>,
    target: Option<Target>,
    filenames: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
}
fn select_artifact(stdout: &[u8], package: &str) -> Result<String, BuildFailure> {
    let text = std::str::from_utf8(stdout).map_err(|_| {
        BuildFailure::new(
            FailureKind::MalformedCargoOutput,
            "Cargo output is not UTF-8",
        )
    })?;
    let mut found = vec![];
    for line in text.lines() {
        let msg: Message = serde_json::from_str(line).map_err(|_| {
            BuildFailure::new(
                FailureKind::MalformedCargoOutput,
                "truncated or malformed Cargo JSON",
            )
        })?;
        if msg.reason == "compiler-artifact"
            && msg
                .package_id
                .as_deref()
                .is_some_and(|id| id.split_whitespace().next() == Some(package))
            && msg
                .target
                .as_ref()
                .is_some_and(|t| t.kind.iter().any(|k| k == "cdylib"))
        {
            found.extend(
                msg.filenames
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|p| p.starts_with(&format!("{TARGET}/"))),
            );
        }
    }
    if found.len() != 1 {
        return Err(BuildFailure::new(
            FailureKind::MalformedCargoOutput,
            "expected exactly one in-target cdylib artifact",
        ));
    }
    Ok(found.remove(0))
}
fn split(value: &str) -> Result<Vec<String>, BuildFailure> {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(BuildFailure::request("path escapes sealed mount"));
    }
    Ok(path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect())
}
fn split_target(value: &str) -> Result<Vec<String>, BuildFailure> {
    value
        .strip_prefix(&format!("{TARGET}/"))
        .ok_or_else(|| {
            BuildFailure::new(
                FailureKind::MalformedCargoOutput,
                "artifact escaped target root",
            )
        })
        .and_then(split)
}
