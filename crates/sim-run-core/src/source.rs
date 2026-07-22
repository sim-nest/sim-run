use std::{ffi::OsString, fmt, path::PathBuf, str::FromStr};

use crate::{CliError, CratesIoSpec};
use sim_kernel::{Datum, LibSourceSpec as KernelLibSourceSpec, Symbol};

/// Library source syntax accepted by the command line.
///
/// Each variant maps to a `kind:value` spelling parsed via [`FromStr`].
///
/// # Examples
///
/// ```
/// use sim_run_core::LibSourceSpec;
///
/// assert_eq!(
///     "symbol:codec/lisp".parse::<LibSourceSpec>().unwrap(),
///     LibSourceSpec::Symbol("codec/lisp".to_owned()),
/// );
/// assert!("codec/lisp".parse::<LibSourceSpec>().is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibSourceSpec {
    /// A `symbol:NAME` source resolved through the kernel loader.
    Symbol(String),
    /// A `path:PATH` source read from the local filesystem.
    Path(PathBuf),
    /// A `url:URL` source fetched from a remote location.
    Url(String),
    /// A `bytes:TEXT` source holding inline library bytes.
    Bytes(Vec<u8>),
    /// An open loader-defined source carried as opaque kernel data.
    Open {
        /// Loader-defined source kind.
        kind: Symbol,
        /// Opaque payload interpreted by the loader that claims `kind`.
        payload: Datum,
    },
    /// A `host:NAME` source provided by the host environment.
    Host(String),
    /// A `crates.io:NAME@REQ` source resolved outside the kernel.
    CratesIo(CratesIoSpec),
}

impl LibSourceSpec {
    pub(crate) fn to_kernel_data_source(&self) -> Option<KernelLibSourceSpec> {
        match self {
            Self::Symbol(symbol) => Some(KernelLibSourceSpec::Symbol(symbol_from_text(symbol))),
            Self::Path(path) => Some(sim_run_loaders::path_source_spec(path.clone())),
            Self::Url(url) => Some(sim_run_loaders::url_source_spec(url.clone())),
            Self::Bytes(bytes) => Some(sim_run_loaders::bytes_source_spec(bytes.clone())),
            Self::Open { kind, payload } => Some(KernelLibSourceSpec::Open {
                kind: kind.clone(),
                payload: payload.clone(),
            }),
            Self::Host(_) | Self::CratesIo(_) => None,
        }
    }

    pub(crate) fn from_kernel_data_source(source: KernelLibSourceSpec) -> Self {
        match source {
            KernelLibSourceSpec::Symbol(symbol) => Self::Symbol(symbol.to_string()),
            KernelLibSourceSpec::Open { kind, payload }
                if kind == sim_run_loaders::path_source_kind() =>
            {
                sim_run_loaders::path_from_payload(&payload)
                    .map(Self::Path)
                    .unwrap_or(Self::Open { kind, payload })
            }
            KernelLibSourceSpec::Open { kind, payload }
                if kind == sim_run_loaders::url_source_kind() =>
            {
                sim_run_loaders::url_from_payload(&payload)
                    .map(Self::Url)
                    .unwrap_or(Self::Open { kind, payload })
            }
            KernelLibSourceSpec::Open { kind, payload }
                if kind == sim_run_loaders::bytes_source_kind() =>
            {
                sim_run_loaders::bytes_from_payload(&payload)
                    .map(Self::Bytes)
                    .unwrap_or(Self::Open { kind, payload })
            }
            KernelLibSourceSpec::Open { kind, payload } => Self::Open { kind, payload },
        }
    }
}

pub(crate) fn parse_source_os(source: OsString) -> Result<LibSourceSpec, CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let bytes = source.as_os_str().as_bytes();
        if let Some(rest) = bytes.strip_prefix(b"path:") {
            if rest.is_empty() {
                return Err(CliError::new("path: source value is empty"));
            }
            return Ok(LibSourceSpec::Path(PathBuf::from(OsString::from_vec(
                rest.to_vec(),
            ))));
        }
    }

    let source = source
        .into_string()
        .map_err(|_| CliError::new("non-UTF-8 library source requires path:"))?;
    source.parse()
}

impl fmt::Display for LibSourceSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol(symbol) => write!(f, "symbol:{symbol}"),
            Self::Path(path) => write!(f, "path:{}", path.display()),
            Self::Url(url) => write!(f, "url:{url}"),
            Self::Bytes(bytes) => write!(f, "bytes:{} bytes", bytes.len()),
            Self::Open { kind, .. } => write!(f, "open:{kind}"),
            Self::Host(name) => write!(f, "host:{name}"),
            Self::CratesIo(spec) => write!(f, "crates.io:{spec}"),
        }
    }
}

pub(crate) fn symbol_from_text(text: &str) -> Symbol {
    match text.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => Symbol::new(text.to_owned()),
    }
}

impl FromStr for LibSourceSpec {
    type Err = CliError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let Some((kind, rest)) = source.split_once(':') else {
            return Err(CliError::new("library source must use kind:value syntax"));
        };
        if rest.is_empty() {
            return Err(CliError::new(format!("{kind}: source value is empty")));
        }
        match kind {
            "symbol" => Ok(Self::Symbol(rest.to_owned())),
            "path" => Ok(Self::Path(PathBuf::from(rest))),
            "url" => Ok(Self::Url(rest.to_owned())),
            "bytes" => Ok(Self::Bytes(rest.as_bytes().to_vec())),
            "host" => Ok(Self::Host(rest.to_owned())),
            "crates.io" => Ok(Self::CratesIo(rest.parse::<CratesIoSpec>()?)),
            _ => Err(CliError::new(format!(
                "unsupported library source kind: {kind}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    #[test]
    fn parses_supported_source_specs() {
        assert_eq!(
            "symbol:codec/lisp".parse::<LibSourceSpec>().unwrap(),
            LibSourceSpec::Symbol("codec/lisp".to_owned())
        );
        assert_eq!(
            "path:./libs/demo.wasm".parse::<LibSourceSpec>().unwrap(),
            LibSourceSpec::Path(PathBuf::from("./libs/demo.wasm"))
        );
        assert_eq!(
            "url:https://example.invalid/demo.wasm"
                .parse::<LibSourceSpec>()
                .unwrap(),
            LibSourceSpec::Url("https://example.invalid/demo.wasm".to_owned())
        );
        assert_eq!(
            "bytes:abc".parse::<LibSourceSpec>().unwrap(),
            LibSourceSpec::Bytes(b"abc".to_vec())
        );
        assert_eq!(
            "host:test/demo".parse::<LibSourceSpec>().unwrap(),
            LibSourceSpec::Host("test/demo".to_owned())
        );
        assert_eq!(
            "crates.io:sim-codec-lisp@0.1.0"
                .parse::<LibSourceSpec>()
                .unwrap(),
            LibSourceSpec::CratesIo("sim-codec-lisp@0.1.0".parse().unwrap())
        );
    }

    #[test]
    fn source_parse_errors_are_typed() {
        assert_eq!(
            "codec/lisp"
                .parse::<LibSourceSpec>()
                .unwrap_err()
                .to_string(),
            "library source must use kind:value syntax"
        );
        assert_eq!(
            "symbol:".parse::<LibSourceSpec>().unwrap_err().to_string(),
            "symbol: source value is empty"
        );
        assert_eq!(
            "crate:sim"
                .parse::<LibSourceSpec>()
                .unwrap_err()
                .to_string(),
            "unsupported library source kind: crate"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_path_os_bytes_survive_non_utf8() {
        let parsed = parse_source_os(OsString::from_vec(
            b"path:/tmp/sim-run-\xff-provider.so".to_vec(),
        ))
        .unwrap();

        let LibSourceSpec::Path(path) = parsed else {
            panic!("expected path source");
        };
        assert_eq!(
            path.as_os_str().as_bytes(),
            b"/tmp/sim-run-\xff-provider.so"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_text_source_fails_closed() {
        let err = parse_source_os(OsString::from_vec(b"symbol:codec/\xff".to_vec())).unwrap_err();

        assert_eq!(err.to_string(), "non-UTF-8 library source requires path:");
    }
}
