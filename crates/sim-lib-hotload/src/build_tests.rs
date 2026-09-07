use super::*;
use crate::{FailureKind, ToolchainIdentity};
use sim_kernel::{Symbol, datum_content_algorithm};
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

fn report() -> SandboxReport {
    SandboxReport {
        launcher: "sealed-launcher".into(),
        controls: vec![SandboxEvidence {
            control: SandboxControl::Network,
            achieved: true,
            detail: "isolated namespace".into(),
        }],
        limit_hits: vec!["output-truncated".into()],
        cleanup: "reaped".into(),
    }
}

#[test]
fn build_receipt_binds_every_request_field_and_typed_dependencies() {
    let artifact = crate::artifact::content_id(b"artifact");
    let sandbox = sandbox_report_id(&report()).unwrap();
    let baseline = request();
    let expected = build_receipt_id(&baseline, &artifact, &sandbox).unwrap();
    assert_eq!(expected.content_id().algorithm, datum_content_algorithm());
    assert_eq!(expected.content_id().bytes.len(), 32);

    let mut variants = Vec::new();
    let mut value = baseline.clone();
    value.source_mount = "sha256:other-source".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.manifest = "native/Cargo.toml".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.package = "other".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.features.insert("trace".into());
    variants.push(value);
    let mut value = baseline.clone();
    value.expected_library = Symbol::qualified("guest", "other");
    variants.push(value);
    let mut value = baseline.clone();
    value.toolchain.content = "sha256:other-toolchain".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.toolchain.cargo_program = "other-cargo".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.toolchain.environment[0].1 = "/other/bin".into();
    variants.push(value);

    for variant in variants {
        assert_ne!(
            build_receipt_id(&variant, &artifact, &sandbox).unwrap(),
            expected
        );
    }
    assert_ne!(
        build_receipt_id(
            &baseline,
            &crate::artifact::content_id(b"other-artifact"),
            &sandbox,
        )
        .unwrap(),
        expected
    );
    assert_ne!(
        build_receipt_id(
            &baseline,
            &artifact,
            &sandbox_report_id(&SandboxReport {
                cleanup: "unknown".into(),
                ..report()
            })
            .unwrap(),
        )
        .unwrap(),
        expected
    );
}

#[test]
fn sandbox_report_identity_is_explicit_and_order_independent_for_controls() {
    let mut reordered = report();
    reordered.controls.push(SandboxEvidence {
        control: SandboxControl::Mounts,
        achieved: true,
        detail: "sealed roots".into(),
    });
    let expected = sandbox_report_id(&reordered).unwrap();
    reordered.controls.reverse();
    assert_eq!(sandbox_report_id(&reordered).unwrap(), expected);

    reordered.controls[0].detail = "different evidence".into();
    assert_ne!(sandbox_report_id(&reordered).unwrap(), expected);
}

#[test]
fn duplicate_toolchain_bindings_are_refused_before_identity_or_spawn() {
    let mut value = request();
    value
        .toolchain
        .environment
        .push(("PATH".into(), "/other/bin".into()));
    assert_eq!(
        value.validate_fields().unwrap_err().kind,
        FailureKind::ToolchainFailure
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
