use sim_platform_core::PureBootEnvelope;

/// One content-addressed request projected from the shared pure boot envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootLoadRequest {
    /// Application or library identity.
    pub id: String,
    /// Exact content required by the bundle.
    pub content_digest: String,
}

/// Projects a capsule-composed envelope into the bootloader's ordered load
/// requests. This consumer performs no platform selection or fallback.
#[must_use]
pub fn boot_load_requests(envelope: &PureBootEnvelope) -> Vec<BootLoadRequest> {
    std::iter::once(&envelope.load_plan.application)
        .chain(envelope.load_plan.libraries.iter())
        .map(|content| BootLoadRequest {
            id: content.id.0.clone(),
            content_digest: content.content_digest.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_platform_core::{BundleContent, LibraryLoadPlan, OpenSymbol};

    #[test]
    fn bootloader_consumes_only_the_shared_envelope_load_plan() {
        let envelope = PureBootEnvelope {
            schema: OpenSymbol("boot/envelope/v1".into()),
            capsule: OpenSymbol("platform/site/model".into()),
            bootstrap: OpenSymbol("bootstrap/sim-native-abi-v1".into()),
            load_plan: LibraryLoadPlan {
                application: content("application/portable", "sha256:app"),
                libraries: vec![content("library/core", "sha256:core")],
            },
        };
        assert_eq!(
            boot_load_requests(&envelope),
            vec![
                BootLoadRequest {
                    id: "application/portable".into(),
                    content_digest: "sha256:app".into()
                },
                BootLoadRequest {
                    id: "library/core".into(),
                    content_digest: "sha256:core".into()
                },
            ]
        );
    }

    fn content(id: &str, digest: &str) -> BundleContent {
        BundleContent {
            id: OpenSymbol(id.into()),
            content_digest: digest.into(),
            capabilities: vec![],
        }
    }
}
