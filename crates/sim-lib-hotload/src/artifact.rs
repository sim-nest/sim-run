use crate::BuildFailure;
use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Symbol};
use sim_storage_port::{HostDirErrorKind, HostDirPort, NeverCancel};

/// Result of publishing one verified immutable artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactCandidate {
    /// SHA-256 content identity of the admitted bytes.
    pub content: ContentId,
    /// Byte length.
    pub bytes: u64,
    /// Expected library identity retained for admission.
    pub expected_library: Symbol,
    /// Sandbox report identity.
    pub sandbox_report: ContentId,
    /// Deterministic build receipt identity.
    pub build_receipt: ContentId,
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
    pub(crate) fn put(&self, bytes: &[u8]) -> Result<(ContentId, bool), BuildFailure> {
        let id = content_id(bytes);
        let name = hex(&id.bytes);
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

pub(crate) fn content_id(bytes: &[u8]) -> ContentId {
    ContentId::from_bytes(
        Symbol::qualified("core", "sha256"),
        Sha256::digest(bytes).into(),
    )
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
