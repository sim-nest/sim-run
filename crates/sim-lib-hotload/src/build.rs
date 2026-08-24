use crate::{
    ArtifactCandidate, BuildFailure, FailureKind, NativeBuildRequest,
    artifact::{ArtifactStore, content_id},
};
use serde::Deserialize;
use sim_lib_exec::{
    ArgAtom, MountAccess, ProcessCancellation, ProgramRef, SandboxAttempt, SandboxControl,
    SandboxLauncher, SandboxLimits, SandboxMount, SandboxPolicy, SandboxRequest,
    SandboxRequirement, SealedBindings,
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
        let report = content_id(format!("{:?}", result.report).as_bytes());
        let receipt = content_id(
            format!(
                "{}:{}:{}",
                request.source_mount,
                request.toolchain.content,
                crate::artifact::hex(&content.bytes)
            )
            .as_bytes(),
        );
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
        .metadata(&vec!["Cargo.lock".into()])
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
mod tests {
    use super::*;
    use crate::{FailureKind, ToolchainIdentity};
    use sim_kernel::Symbol;
    use std::collections::BTreeSet;

    fn request() -> NativeBuildRequest {
        NativeBuildRequest {
            source_mount: "sha256:source".into(),
            manifest: "Cargo.toml".into(),
            package: "guest".into(),
            features: BTreeSet::from(["native".into()]),
            expected_library: Symbol::qualified("guest", "lib"),
            toolchain: ToolchainIdentity {
                content: "sha256:toolchain".into(),
                cargo_program: "sealed-cargo".into(),
                environment: vec![("PATH".into(), "/toolchain/bin".into())],
            },
        }
    }

    fn artifact(path: &str) -> Vec<u8> {
        format!(r#"{{"reason":"compiler-artifact","package_id":"guest 0.1.0 (path+file:///source)","target":{{"kind":["cdylib"]}},"filenames":["{path}"]}}"#).into_bytes()
    }

    #[test]
    fn denial_before_spawn_rejects_escaping_manifest() {
        let mut value = request();
        value.manifest = "../Cargo.toml".into();
        assert_eq!(
            value.validate_fields().unwrap_err().kind,
            FailureKind::RequestRefusal
        );
    }

    #[test]
    fn fixed_plan_is_offline_locked_and_has_one_writable_mount() {
        let plan = sandbox_request(&request()).unwrap();
        let args = plan.argv.iter().map(ArgAtom::as_str).collect::<Vec<_>>();
        assert_eq!(
            &args[..4],
            [
                "build",
                "--locked",
                "--offline",
                "--message-format=json-render-diagnostics"
            ]
        );
        assert_eq!(
            plan.policy
                .mounts()
                .iter()
                .filter(|m| m.access == MountAccess::Writable)
                .count(),
            1
        );
        assert!(plan.environment.iter().all(|(k, _)| k == "PATH"));
    }

    #[test]
    fn multiple_artifacts_are_refused() {
        let mut lines = artifact("/target/debug/libguest.so");
        lines.push(b'\n');
        lines.extend(artifact("/target/release/libguest.so"));
        assert_eq!(
            select_artifact(&lines, "guest").unwrap_err().kind,
            FailureKind::MalformedCargoOutput
        );
    }

    #[test]
    fn truncated_json_is_refused() {
        assert_eq!(
            select_artifact(br#"{"reason":"compiler"#, "guest")
                .unwrap_err()
                .kind,
            FailureKind::MalformedCargoOutput
        );
    }

    #[test]
    fn out_of_root_artifact_is_refused() {
        assert_eq!(
            select_artifact(&artifact("/source/escape.so"), "guest")
                .unwrap_err()
                .kind,
            FailureKind::MalformedCargoOutput
        );
    }

    #[test]
    fn source_and_toolchain_identity_change_receipt_material() {
        let a = request();
        let mut b = request();
        b.toolchain.content = "sha256:other".into();
        assert_ne!(
            format!("{}:{}", a.source_mount, a.toolchain.content),
            format!("{}:{}", b.source_mount, b.toolchain.content)
        );
    }

    #[test]
    fn diagnostics_are_bounded_and_sanitized() {
        let failure = BuildFailure::new(
            FailureKind::CargoFailure,
            format!("{}\0secret", "x".repeat(3000)),
        );
        assert!(failure.diagnostic.len() <= 2048);
        assert!(!failure.diagnostic.contains('\0'));
    }
}

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
