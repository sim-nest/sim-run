use super::*;
use sim_kernel::{
    AbiVersion, CapabilityName, Dependency, Export, LibTarget, Version, datum_content_algorithm,
};

#[test]
fn artifact_source_must_bind_the_verified_bytes_or_digest() {
    let bytes = b"candidate";
    let content = content_id(bytes);
    let direct = sim_run_loaders::bytes_source(bytes);
    assert!(require_candidate_source(&direct, &content, bytes).is_ok());

    let addressed =
        sim_run_loaders::content_address_source(Datum::Bytes(content.content_id().bytes.to_vec()));
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

#[test]
fn admission_identity_is_tagged_complete_and_dependency_order_independent() {
    let artifact = content_id(b"artifact");
    let current = content_id(b"current");
    let manifest = LibManifest {
        id: Symbol::new("candidate"),
        version: Version("1.2.3".into()),
        abi: AbiVersion { major: 1, minor: 2 },
        target: LibTarget::Native,
        requires: vec![
            Dependency {
                id: Symbol::new("a"),
                minimum_version: Some(Version("1.0.0".into())),
            },
            Dependency {
                id: Symbol::new("b"),
                minimum_version: None,
            },
        ],
        capabilities: vec![CapabilityName::new("read")],
        exports: vec![Export::Value {
            symbol: Symbol::new("answer"),
        }],
    };
    let compatibility = CompatibilityReport {
        policy: Some(CompatibilityPolicy::Exact),
        candidate_exports: vec![(
            sim_kernel::ExportKind::named(sim_kernel::ExportKind::VALUE),
            Symbol::new("answer"),
        )],
        added_exports: Vec::new(),
    };
    let tests = vec![CandidateTestResult {
        symbol: Symbol::new("self-test"),
        passed: true,
        detail: Some("passed".into()),
    }];
    let limits = AchievedLimits {
        tests_run: 1,
        max_events_observed: 2,
        max_detail_chars_observed: 6,
    };
    let dependencies = vec![Symbol::new("a"), Symbol::new("b")];
    let native_loader = Symbol::new("native");
    let base = AdmissionIdentity {
        artifact: &artifact,
        current: Some(&current),
        manifest: &manifest,
        compatibility: &compatibility,
        loader: &native_loader,
        dependencies: &dependencies,
        tests: &tests,
        limits: &limits,
    };
    let id = receipt_id(&base).unwrap();
    assert_eq!(id.algorithm, datum_content_algorithm());

    let other_artifact = content_id(b"other-artifact");
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            artifact: &other_artifact,
            ..base
        })
        .unwrap()
    );
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            current: None,
            ..base
        })
        .unwrap()
    );
    let mut changed_manifest = manifest.clone();
    changed_manifest.version = Version("1.2.4".into());
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            manifest: &changed_manifest,
            ..base
        })
        .unwrap()
    );
    let mut changed_compatibility = compatibility.clone();
    changed_compatibility.policy = Some(CompatibilityPolicy::Additive);
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            compatibility: &changed_compatibility,
            ..base
        })
        .unwrap()
    );
    let mut changed_tests = tests.clone();
    changed_tests[0].passed = false;
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            tests: &changed_tests,
            ..base
        })
        .unwrap()
    );
    let changed_limits = AchievedLimits {
        tests_run: limits.tests_run,
        max_events_observed: limits.max_events_observed + 1,
        max_detail_chars_observed: limits.max_detail_chars_observed,
    };
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            limits: &changed_limits,
            ..base
        })
        .unwrap()
    );

    let reversed = vec![Symbol::new("b"), Symbol::new("a")];
    assert_eq!(
        id,
        receipt_id(&AdmissionIdentity {
            dependencies: &reversed,
            ..base
        })
        .unwrap()
    );
    let changed_dependencies = vec![Symbol::new("a")];
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            dependencies: &changed_dependencies,
            ..base
        })
        .unwrap()
    );
    let wasm_loader = Symbol::new("wasm");
    assert_ne!(
        id,
        receipt_id(&AdmissionIdentity {
            loader: &wasm_loader,
            ..base
        })
        .unwrap()
    );
    let old_raw = ContentId::from_bytes(Symbol::qualified("core", "sha256"), id.bytes);
    assert_ne!(id, old_raw);
}
