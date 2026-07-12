use std::{ffi::OsString, path::PathBuf};

use crate::{ConfigLoadOptions, LibSourceSpec, boot_codec_name, codec_lib_symbol};

/// Parsed bootloader controls and payload data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliBoot {
    /// Codec name selected with `--codec`, or `None` for the default.
    pub codec: Option<String>,
    /// Library sources to load, in `--load` order.
    pub loads: Vec<LibSourceSpec>,
    /// Optional native audio provider source requested by the operator.
    pub native_audio_provider: Option<Box<LibSourceSpec>>,
    /// Runtime configuration source options.
    pub config: ConfigLoadOptions,
    /// Whether `--list` requested a loaded-lib listing.
    pub list: bool,
    /// Symbol passed to `--inspect`, if any.
    pub inspect: Option<String>,
    /// Payload data preserved for the loaded-lib handoff.
    pub payload: Payload,
}

impl CliBoot {
    /// Builds the data envelope handed to loaded libraries.
    pub fn envelope(&self) -> CliEnvelope {
        let codec_name = boot_codec_name(self);
        CliEnvelope {
            codec: codec_lib_symbol(codec_name),
            verb: self
                .payload
                .args
                .first()
                .map(|arg| arg.to_string_lossy().into_owned()),
            args: self.payload.args.clone(),
            eval: self.payload.eval.clone(),
            script: self.payload.script.clone(),
            stdin: self.payload.stdin.clone(),
        }
    }
}

/// Payload preserved for loaded-lib behavior.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    /// Trailing positional arguments handed to the loaded entrypoint.
    pub args: Vec<OsString>,
    /// Eval text carried from `--eval`.
    pub eval: Option<String>,
    /// Script path carried from `--script`.
    pub script: Option<PathBuf>,
    /// Stdin text carried from `--stdin`.
    pub stdin: Option<String>,
}

/// Data envelope supplied to the selected loaded-lib entrypoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliEnvelope {
    /// Codec library symbol selected for the boot session.
    pub codec: String,
    /// First payload argument, exposed as the loaded-lib verb.
    pub verb: Option<String>,
    /// Full payload argument list.
    pub args: Vec<OsString>,
    /// Eval text carried from `--eval`.
    pub eval: Option<String>,
    /// Script path carried from `--script`.
    pub script: Option<PathBuf>,
    /// Stdin text carried from `--stdin`.
    pub stdin: Option<String>,
}
