use crate::BuildFailure;
use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Symbol};
use sim_storage_port::{HostDirErrorKind, HostDirPort, NeverCancel};

/// Byte-address identity for exact immutable artifact bytes.
///
/// ```compile_fail
/// use sim_lib_hotload::ArtifactContentId;
/// use sim_kernel::ContentId;
/// fn semantic(_: ContentId) {}
/// fn crossing(location: ArtifactContentId) { semantic(location); }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactContentId(ContentId);

impl ArtifactContentId {
    /// Borrows the registered byte-address identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Semantic identity of the achieved sandbox report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxReportId(pub(crate) ContentId);

impl SandboxReportId {
    /// Borrows the canonical semantic identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Semantic identity of a complete native build receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildReceiptId(pub(crate) ContentId);

impl BuildReceiptId {
    /// Borrows the canonical semantic identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Result of publishing one verified immutable artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactCandidate {
    /// SHA-256 content identity of the admitted bytes.
    pub content: ArtifactContentId,
    /// Byte length.
    pub bytes: u64,
    /// Expected library identity retained for admission.
    pub expected_library: Symbol,
    /// Sandbox report identity.
    pub sandbox_report: SandboxReportId,
    /// Deterministic build receipt identity.
    pub build_receipt: BuildReceiptId,
    /// Whether identical verified bytes already existed.
    pub cache_hit: bool,
}

/// Immutable content-addressed writer over a preopened artifact mount.
pub struct ArtifactStore<'a> {
    port: &'a dyn HostDirPort,
}
impl<'a> ArtifactStore<'a> {
    /// Wraps one preopened artifact mount.
    pub fn new(port: &'a dyn HostDirPort) -> Self {
        Self { port }
    }
    pub(crate) fn put(&self, bytes: &[u8]) -> Result<(ArtifactContentId, bool), BuildFailure> {
        let id = content_id(bytes);
        let name = hex(&id.content_id().bytes);
        let path = vec![name];
        let hit = match self.port.read(&path) {
            Ok(existing) if existing == bytes => true,
            Ok(_) => return Err(BuildFailure::artifact("content key collision")),
            Err(e) if e.kind == HostDirErrorKind::NotFound => {
                self.port
                    .replace(&path, bytes, &NeverCancel)
                    .map_err(|e| BuildFailure::artifact(e.to_string()))?;
                false
            }
            Err(e) => return Err(BuildFailure::artifact(e.to_string())),
        };
        let verified = self
            .port
            .read(&path)
            .map_err(|e| BuildFailure::artifact(e.to_string()))?;
        if content_id(&verified) != id {
            return Err(BuildFailure::artifact("artifact re-read digest mismatch"));
        }
        Ok((id, hit))
    }
}

pub(crate) fn content_id(bytes: &[u8]) -> ArtifactContentId {
    ArtifactContentId(ContentId::from_bytes(
        Symbol::qualified("core", "sha256"),
        Sha256::digest(bytes).into(),
    ))
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
