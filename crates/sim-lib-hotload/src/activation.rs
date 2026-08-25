use std::{collections::BTreeMap, fmt, sync::Arc};

use sim_kernel::{ContentId, Cx, ExportRecord, LibSource, Symbol};
use sim_lib_journal::{Journal, JournalBackend, JournalEntry, JournalHead, JournalObject, Lease};
use sim_run_loaders::{LoadRequest, LoaderKind, LoaderPort};

use crate::{AdmissionReceipt, CompatibilityReport};

const INTENT_KIND: &str = "hotload/activation-intent-v1";
const COMPLETE_KIND: &str = "hotload/activation-complete-v1";

/// One admitted activation, including the exact source used at admission.
pub struct ActivationRequest<'a> {
    /// Current admission proof. It is rechecked against live state immediately
    /// before the intent is written and again before the kernel transaction.
    pub admission: &'a AdmissionReceipt,
    /// Exact candidate source bound by the admission proof.
    pub source: LibSource,
    /// Loader selected by admission.
    pub loader_kind: LoaderKind,
}

/// Audit disposition of a committed activation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivationAudit {
    /// The completion record is durable at this journal head.
    Complete(JournalHead),
    /// The kernel committed, but completion still has to be appended.
    Pending(ContentId),
}

/// Durable result binding the old and new generations to the committed surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationReceipt {
    /// Admission/plan identity, used for exact idempotency.
    pub plan: ContentId,
    /// Previous managed generation; absent only for initial installation.
    pub previous_generation: Option<ContentId>,
    /// Newly committed generation.
    pub generation: ContentId,
    /// Exact committed export surface, including stable runtime ids.
    pub exports: Vec<ExportRecord>,
    /// Compatibility evidence applied at admission.
    pub compatibility: CompatibilityReport,
    /// Completion state and journal position.
    pub audit: ActivationAudit,
}

/// Mutation availability exposed to orchestration and status surfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivationStatus {
    /// Further activation is allowed.
    Ready,
    /// A committed activation awaits its completion append.
    AuditPending(ActivationReceipt),
}

/// Closed activation failure. A committed kernel transaction is never returned
/// as this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationFailure(pub String);

impl fmt::Display for ActivationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ActivationFailure {}

/// Atomic hotload activation coordinator over existing kernel, loader, and
/// journal boundaries.
pub struct ActivationService<B> {
    journal: Journal<Arc<B>>,
    lease: Lease,
    loader: Arc<dyn LoaderPort>,
    completed: BTreeMap<ContentId, ActivationReceipt>,
    pending: Option<PendingCompletion>,
}

struct PendingCompletion {
    receipt: ActivationReceipt,
    intent_head: JournalHead,
}

impl<B: JournalBackend + 'static> ActivationService<B> {
    /// Starts a fenced writer. Completed receipts can subsequently be restored
    /// with [`Self::replay_completed`].
    pub fn new(backend: Arc<B>, loader: Arc<dyn LoaderPort>) -> Result<Self, ActivationFailure> {
        let journal = Journal::new(backend);
        let lease = journal.acquire_lease().map_err(journal_failure)?;
        Ok(Self {
            journal,
            lease,
            loader,
            completed: BTreeMap::new(),
            pending: None,
        })
    }

    /// Returns the mutation/audit state.
    pub fn status(&self) -> ActivationStatus {
        self.pending
            .as_ref()
            .map_or(ActivationStatus::Ready, |pending| {
                ActivationStatus::AuditPending(pending.receipt.clone())
            })
    }

    /// Activates exactly the admitted generation. Exact completed redelivery is
    /// a no-op; all other requests are rejected while audit is pending.
    pub fn activate(
        &mut self,
        live: &mut Cx,
        request: ActivationRequest<'_>,
    ) -> Result<ActivationReceipt, ActivationFailure> {
        if let Some(receipt) = self.completed.get(&request.admission.content) {
            return Ok(receipt.clone());
        }
        if self.pending.is_some() {
            return Err(ActivationFailure(
                "activation is sealed: audit completion pending".into(),
            ));
        }
        self.require_current(live, request.admission)?;
        let outcome = self
            .loader
            .realize(
                live,
                LoadRequest {
                    kind: request.loader_kind,
                    source: clone_source(&request.source)?,
                },
            )
            .map_err(|error| ActivationFailure(format!("candidate realization failed: {error}")))?;
        if outcome.manifest != request.admission.manifest
            || outcome.library.manifest() != request.admission.manifest
        {
            return Err(ActivationFailure(
                "candidate artifact changed after admission".into(),
            ));
        }
        self.require_current(live, request.admission)?;

        let intent = JournalObject::from_bytes(intent_payload(request.admission));
        let intent_id = intent.id.clone();
        let before = self.journal.head().map_err(journal_failure)?;
        let intent_head = self.append(before.as_ref(), INTENT_KIND, intent)?;

        // Nothing fallible may be reported as activation failure after this
        // transaction returns success.
        let expected_id = live
            .registry()
            .lib(&request.admission.manifest.id)
            .map(|lib| lib.id);
        live.activate_lib(expected_id, outcome.library.as_ref())
            .map_err(|error| ActivationFailure(format!("kernel activation failed: {error}")))?;
        let loaded = live
            .registry()
            .lib(&request.admission.manifest.id)
            .ok_or_else(|| ActivationFailure("kernel omitted committed library".into()))?;
        let mut receipt = ActivationReceipt {
            plan: request.admission.content.clone(),
            previous_generation: request.admission.current_generation.clone(),
            generation: request.admission.artifact.clone(),
            exports: loaded.exports.clone(),
            compatibility: request.admission.compatibility.clone(),
            audit: ActivationAudit::Pending(intent_id),
        };
        let completion = JournalObject::from_bytes(completion_payload(&receipt));
        match self.append(Some(&intent_head), COMPLETE_KIND, completion) {
            Ok(head) => {
                receipt.audit = ActivationAudit::Complete(head);
                self.completed.insert(receipt.plan.clone(), receipt.clone());
            }
            Err(_) => {
                self.pending = Some(PendingCompletion {
                    receipt: receipt.clone(),
                    intent_head,
                });
            }
        }
        Ok(receipt)
    }

    /// Retries only the completion append after a committed activation. No
    /// kernel state is touched.
    pub fn retry_completion(&mut self) -> Result<ActivationReceipt, ActivationFailure> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| ActivationFailure("no audit completion is pending".into()))?;
        let object = JournalObject::from_bytes(completion_payload(&pending.receipt));
        match self.append(Some(&pending.intent_head), COMPLETE_KIND, object) {
            Ok(head) => {
                let mut receipt = pending.receipt;
                receipt.audit = ActivationAudit::Complete(head);
                self.completed.insert(receipt.plan.clone(), receipt.clone());
                Ok(receipt)
            }
            Err(error) => {
                self.pending = Some(pending);
                Err(error)
            }
        }
    }

    /// Restores only caller-verified completed receipts, in journal order.
    /// Intent-only records are deliberately absent from this input. The caller
    /// reconstructs each generation through ordinary boot receipts and the
    /// loader port before supplying it here.
    pub fn replay_completed(
        &mut self,
        receipts: impl IntoIterator<Item = ActivationReceipt>,
    ) -> Result<(), ActivationFailure> {
        let mut last = None;
        for receipt in receipts {
            let ActivationAudit::Complete(ref head) = receipt.audit else {
                continue;
            };
            if last
                .as_ref()
                .is_some_and(|prior: &u64| *prior >= head.sequence)
            {
                return Err(ActivationFailure(
                    "activation receipts are not in journal order".into(),
                ));
            }
            last = Some(head.sequence);
            self.completed.insert(receipt.plan.clone(), receipt);
        }
        Ok(())
    }

    fn require_current(
        &self,
        live: &Cx,
        admission: &AdmissionReceipt,
    ) -> Result<(), ActivationFailure> {
        let loaded = live.registry().lib(&admission.manifest.id);
        match (admission.current_generation.as_ref(), loaded) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(ActivationFailure(
                "initial installation refused: symbol is already loaded".into(),
            )),
            (Some(expected), Some(current)) => {
                let generation = self.completed.values().find(|receipt| {
                    &receipt.generation == expected && receipt.exports == current.exports
                });
                if generation.is_some() {
                    Ok(())
                } else {
                    Err(ActivationFailure(
                        "stale expected-current generation".into(),
                    ))
                }
            }
            (Some(_), None) => Err(ActivationFailure(
                "stale expected-current generation: library absent".into(),
            )),
        }
    }

    fn append(
        &self,
        expected: Option<&JournalHead>,
        kind: &str,
        object: JournalObject,
    ) -> Result<JournalHead, ActivationFailure> {
        let sequence = expected.map_or(0, |head| head.sequence + 1);
        let entry = JournalEntry::new(
            sequence,
            expected.map(|head| head.entry.clone()),
            Symbol::new(kind),
            vec![object.id.clone()],
        );
        self.journal
            .publish(&self.lease, expected, vec![object], vec![entry])
            .map_err(journal_failure)
    }
}

fn clone_source(source: &LibSource) -> Result<LibSource, ActivationFailure> {
    match source {
        LibSource::Symbol(value) => Ok(LibSource::Symbol(value.clone())),
        LibSource::Open { kind, payload } => Ok(LibSource::Open {
            kind: kind.clone(),
            payload: payload.clone(),
        }),
        LibSource::Host(_) => Err(ActivationFailure(
            "host library sources cannot be activated".into(),
        )),
    }
}

fn intent_payload(receipt: &AdmissionReceipt) -> Vec<u8> {
    format!(
        "plan={:?}\nartifact={:?}\ncurrent={:?}\nmanifest={:?}\n",
        receipt.content, receipt.artifact, receipt.current_generation, receipt.manifest
    )
    .into_bytes()
}

fn completion_payload(receipt: &ActivationReceipt) -> Vec<u8> {
    format!(
        "plan={:?}\nprevious={:?}\ngeneration={:?}\nexports={:?}\ncompatibility={:?}\n",
        receipt.plan,
        receipt.previous_generation,
        receipt.generation,
        receipt.exports,
        receipt.compatibility
    )
    .into_bytes()
}

fn journal_failure(error: sim_lib_journal::JournalError) -> ActivationFailure {
    ActivationFailure(format!("activation journal failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AchievedLimits, CompatibilityPolicy};
    use sim_kernel::{AbiVersion, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Version};
    use sim_lib_journal::{Admission, JournalError, MemoryBackend, StoredState};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct TestLib {
        manifest: LibManifest,
        value: bool,
        fail: bool,
    }
    impl Lib for TestLib {
        fn manifest(&self) -> LibManifest {
            self.manifest.clone()
        }
        fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> sim_kernel::Result<()> {
            linker.value(Symbol::new("hot-value"), cx.factory().bool(self.value)?)?;
            if self.fail {
                return Err(sim_kernel::Error::Lib("injected".into()));
            }
            Ok(())
        }
    }

    struct TestLoader {
        manifest: LibManifest,
        value: bool,
        fail: bool,
        calls: AtomicUsize,
    }
    impl LoaderPort for TestLoader {
        fn loader_kinds(&self) -> Vec<LoaderKind> {
            vec![LoaderKind::new(Symbol::new("test"))]
        }
        fn realize(
            &self,
            _: &mut Cx,
            _: LoadRequest,
        ) -> sim_kernel::Result<sim_run_loaders::LoadOutcome> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(sim_run_loaders::LoadOutcome {
                manifest: self.manifest.clone(),
                library: Box::new(TestLib {
                    manifest: self.manifest.clone(),
                    value: self.value,
                    fail: self.fail,
                }),
            })
        }
        fn inspect(&self, _: &mut Cx, _: &LoadRequest) -> sim_kernel::Result<Option<LibManifest>> {
            Ok(Some(self.manifest.clone()))
        }
    }

    struct FailSecond {
        inner: MemoryBackend,
        admits: AtomicUsize,
        fail: AtomicBool,
    }
    impl FailSecond {
        fn new() -> Self {
            Self {
                inner: MemoryBackend::new(),
                admits: AtomicUsize::new(0),
                fail: AtomicBool::new(true),
            }
        }
    }
    impl JournalBackend for FailSecond {
        fn acquire_lease(&self) -> Result<Lease, JournalError> {
            self.inner.acquire_lease()
        }
        fn read_state(&self) -> Result<StoredState, JournalError> {
            self.inner.read_state()
        }
        fn admit(&self, admission: Admission) -> Result<JournalHead, JournalError> {
            let call = self.admits.fetch_add(1, Ordering::SeqCst);
            if call == 1 && self.fail.swap(false, Ordering::SeqCst) {
                return Err(JournalError::InjectedCrash("completion"));
            }
            self.inner.admit(admission)
        }
    }

    fn id(byte: u8) -> ContentId {
        ContentId::from_bytes(Symbol::new("test"), [byte; 32])
    }
    fn manifest(version: &str) -> LibManifest {
        LibManifest {
            id: Symbol::new("hot-lib"),
            version: Version(version.into()),
            abi: AbiVersion { major: 1, minor: 0 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: vec![Export::Value {
                symbol: Symbol::new("hot-value"),
            }],
        }
    }
    fn admission(
        manifest: LibManifest,
        plan: u8,
        artifact: u8,
        current: Option<ContentId>,
    ) -> AdmissionReceipt {
        AdmissionReceipt {
            content: id(plan),
            artifact: id(artifact),
            current_generation: current,
            manifest,
            compatibility: CompatibilityReport {
                policy: Some(CompatibilityPolicy::Exact),
                candidate_exports: vec![],
                added_exports: vec![],
            },
            loader: Symbol::new("test"),
            dependencies: vec![],
            tests: vec![],
            achieved_limits: AchievedLimits {
                tests_run: 0,
                max_events_observed: 0,
                max_detail_chars_observed: 0,
            },
        }
    }
    fn request(receipt: &AdmissionReceipt) -> ActivationRequest<'_> {
        ActivationRequest {
            admission: receipt,
            source: LibSource::Symbol(Symbol::new("artifact")),
            loader_kind: LoaderKind::new(Symbol::new("test")),
        }
    }

    fn context() -> Cx {
        Cx::new(
            Arc::new(sim_kernel::NoopEvalPolicy),
            Arc::new(sim_kernel::DefaultFactory),
            sim_kernel::HandleSeed::new(7),
        )
    }

    #[test]
    fn initial_install_is_atomic_and_completed_redelivery_is_idempotent() {
        let manifest = manifest("1.0.0");
        let loader = Arc::new(TestLoader {
            manifest: manifest.clone(),
            value: true,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let mut service =
            ActivationService::new(Arc::new(MemoryBackend::new()), loader.clone()).unwrap();
        let receipt = admission(manifest, 1, 2, None);
        let mut cx = context();
        let first = service.activate(&mut cx, request(&receipt)).unwrap();
        assert!(matches!(first.audit, ActivationAudit::Complete(_)));
        assert_eq!(service.activate(&mut cx, request(&receipt)).unwrap(), first);
        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn post_commit_append_failure_seals_without_rollback_and_can_be_retried() {
        let manifest = manifest("1.0.0");
        let loader = Arc::new(TestLoader {
            manifest: manifest.clone(),
            value: true,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let mut service = ActivationService::new(Arc::new(FailSecond::new()), loader).unwrap();
        let receipt = admission(manifest.clone(), 3, 4, None);
        let mut cx = context();
        let committed = service.activate(&mut cx, request(&receipt)).unwrap();
        assert!(matches!(committed.audit, ActivationAudit::Pending(_)));
        assert!(cx.registry().lib(&manifest.id).is_some());
        assert!(matches!(
            service.status(),
            ActivationStatus::AuditPending(_)
        ));
        assert!(matches!(
            service.retry_completion().unwrap().audit,
            ActivationAudit::Complete(_)
        ));
        assert_eq!(service.status(), ActivationStatus::Ready);
    }

    #[test]
    fn changed_artifact_and_kernel_failure_leave_no_live_surface() {
        let wanted = manifest("1.0.0");
        let changed = manifest("2.0.0");
        let receipt = admission(wanted.clone(), 5, 6, None);
        let mut cx = context();
        let loader = Arc::new(TestLoader {
            manifest: changed,
            value: true,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let mut service = ActivationService::new(Arc::new(MemoryBackend::new()), loader).unwrap();
        assert!(service.activate(&mut cx, request(&receipt)).is_err());
        assert!(cx.registry().lib(&wanted.id).is_none());

        let loader = Arc::new(TestLoader {
            manifest: wanted.clone(),
            value: true,
            fail: true,
            calls: AtomicUsize::new(0),
        });
        let mut service = ActivationService::new(Arc::new(MemoryBackend::new()), loader).unwrap();
        assert!(service.activate(&mut cx, request(&receipt)).is_err());
        assert!(cx.registry().lib(&wanted.id).is_none());
    }

    #[test]
    fn held_generation_proxy_keeps_old_guest_alive_until_last_drop() {
        struct Guest {
            behavior: &'static str,
            destroyed: Arc<AtomicUsize>,
        }
        impl Drop for Guest {
            fn drop(&mut self) {
                self.destroyed.fetch_add(1, Ordering::SeqCst);
            }
        }
        #[derive(Clone)]
        struct Proxy(Arc<Guest>);
        impl Proxy {
            fn call(&self) -> &'static str {
                self.0.behavior
            }
        }

        let destroyed = Arc::new(AtomicUsize::new(0));
        let old_guest = Arc::new(Guest {
            behavior: "old",
            destroyed: destroyed.clone(),
        });
        let old = Proxy(old_guest.clone());
        let held_old = old.clone();
        drop(old);
        drop(old_guest);
        let new_guest = Arc::new(Guest {
            behavior: "new",
            destroyed: destroyed.clone(),
        });
        let fresh = Proxy(new_guest.clone());
        assert_eq!(held_old.call(), "old");
        assert_eq!(fresh.call(), "new");
        assert_eq!(destroyed.load(Ordering::SeqCst), 0);
        drop(held_old);
        assert_eq!(destroyed.load(Ordering::SeqCst), 1);
        drop(fresh);
        drop(new_guest);
        assert_eq!(destroyed.load(Ordering::SeqCst), 2);
    }
}
