use sim_kernel::{ExportRecord, LibBootDependency, LibId, LibManifest};

use crate::LibSourceSpec;

/// The role a loaded library serves in a boot session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadReceiptRole {
    /// A library requested with `--load`.
    Library,
    /// The selected codec loaded before the rest of the boot list.
    BootCodec {
        /// Codec name selected for the boot session.
        name: String,
        /// Codec library symbol that was loaded.
        symbol: String,
    },
}

impl LoadReceiptRole {
    pub(crate) fn boot_codec(name: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self::BootCodec {
            name: name.into(),
            symbol: symbol.into(),
        }
    }
}

/// Receipt for one library loaded by the bootloader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadReceipt {
    /// Identifier assigned to the loaded library.
    pub lib_id: LibId,
    /// Role the library serves in the boot session.
    pub role: LoadReceiptRole,
    /// Source as requested on the command line.
    pub requested_source: LibSourceSpec,
    /// Source after resolution to a concrete artifact.
    pub resolved_source: LibSourceSpec,
    /// Manifest declared by the loaded library.
    pub manifest: LibManifest,
    /// Boot dependencies pulled in while loading.
    pub dependencies: Vec<LibBootDependency>,
    /// Export records published by the library.
    pub exports: Vec<ExportRecord>,
}
