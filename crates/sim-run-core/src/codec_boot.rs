use crate::{CliBoot, LibSourceSpec};

/// Codec name used when `--codec` is omitted.
pub const DEFAULT_CODEC_NAME: &str = "lisp";

/// Returns the selected codec name for a boot session.
pub fn boot_codec_name(boot: &CliBoot) -> &str {
    boot.codec.as_deref().unwrap_or(DEFAULT_CODEC_NAME)
}

/// Returns the library symbol for a codec name.
pub fn codec_lib_symbol(name: &str) -> String {
    format!("codec/{name}")
}

pub(crate) fn explicit_codec_source_index(boot: &CliBoot, symbol: &str) -> Option<usize> {
    boot.loads
        .iter()
        .position(|source| source_matches_codec_symbol(source, symbol))
}

fn source_matches_codec_symbol(source: &LibSourceSpec, symbol: &str) -> bool {
    matches!(
        source,
        LibSourceSpec::Symbol(candidate) | LibSourceSpec::Host(candidate) if candidate == symbol
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_lisp_codec_symbol() {
        let boot = CliBoot::default();

        assert_eq!(boot_codec_name(&boot), "lisp");
        assert_eq!(codec_lib_symbol(boot_codec_name(&boot)), "codec/lisp");
    }

    #[test]
    fn override_selects_named_codec_symbol() {
        let boot = CliBoot {
            codec: Some("json".to_owned()),
            ..CliBoot::default()
        };

        assert_eq!(boot_codec_name(&boot), "json");
        assert_eq!(codec_lib_symbol(boot_codec_name(&boot)), "codec/json");
    }
}
