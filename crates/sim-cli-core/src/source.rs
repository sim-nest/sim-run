use std::{fmt, path::PathBuf, str::FromStr};

use crate::{CliError, CratesIoSpec};
use sim_kernel::{LibSourceSpec as KernelLibSourceSpec, Symbol};

/// Library source syntax accepted by the command line.
///
/// Each variant maps to a `kind:value` spelling parsed via [`FromStr`].
///
/// # Examples
///
/// ```
/// use sim_cli_core::LibSourceSpec;
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
    /// A `host:NAME` source provided by the host environment.
    Host(String),
    /// A `crates.io:NAME@REQ` source resolved outside the kernel.
    CratesIo(CratesIoSpec),
}

impl LibSourceSpec {
    pub(crate) fn to_kernel_data_source(&self) -> Option<KernelLibSourceSpec> {
        match self {
            Self::Symbol(symbol) => Some(KernelLibSourceSpec::Symbol(symbol_from_text(symbol))),
            Self::Path(path) => Some(KernelLibSourceSpec::Path(path.clone())),
            Self::Url(url) => Some(KernelLibSourceSpec::Url(url.clone())),
            Self::Bytes(bytes) => Some(KernelLibSourceSpec::Bytes(bytes.clone())),
            Self::Host(_) | Self::CratesIo(_) => None,
        }
    }

    pub(crate) fn from_kernel_data_source(source: KernelLibSourceSpec) -> Self {
        match source {
            KernelLibSourceSpec::Symbol(symbol) => Self::Symbol(symbol.to_string()),
            KernelLibSourceSpec::Path(path) => Self::Path(path),
            KernelLibSourceSpec::Url(url) => Self::Url(url),
            KernelLibSourceSpec::Bytes(bytes) => Self::Bytes(bytes),
        }
    }
}

impl fmt::Display for LibSourceSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol(symbol) => write!(f, "symbol:{symbol}"),
            Self::Path(path) => write!(f, "path:{}", path.display()),
            Self::Url(url) => write!(f, "url:{url}"),
            Self::Bytes(bytes) => write!(f, "bytes:{} bytes", bytes.len()),
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
}
