//! Loader-defined library source kinds and payload helpers.
//!
//! The kernel carries open source kinds as `(kind, payload)` data. This module
//! defines the source kinds understood by the standard loader family and keeps
//! their payload grammar with the loaders that interpret it.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
};

use sim_kernel::{CatalogSource, Datum, Error, LibSource, LibSourceSpec, Result, Symbol};

/// Loader-defined kind for local filesystem paths.
pub const PATH_SOURCE_KIND: &str = "path";
/// Loader-defined kind for remote URLs.
pub const URL_SOURCE_KIND: &str = "url";
/// Loader-defined kind for in-memory artifact bytes.
pub const BYTES_SOURCE_KIND: &str = "bytes";
/// Loader-defined kind for content-addressed library artifacts.
pub const CONTENT_ADDRESS_SOURCE_KIND: &str = "content-address";

/// Returns the local path source kind symbol.
pub fn path_source_kind() -> Symbol {
    Symbol::new(PATH_SOURCE_KIND)
}

/// Returns the URL source kind symbol.
pub fn url_source_kind() -> Symbol {
    Symbol::new(URL_SOURCE_KIND)
}

/// Returns the bytes source kind symbol.
pub fn bytes_source_kind() -> Symbol {
    Symbol::new(BYTES_SOURCE_KIND)
}

/// Returns the content-addressed source kind symbol.
pub fn content_address_source_kind() -> Symbol {
    Symbol::new(CONTENT_ADDRESS_SOURCE_KIND)
}

/// Builds a live kernel source for a local path.
pub fn path_source(path: impl Into<PathBuf>) -> LibSource {
    LibSource::open(path_source_kind(), path_payload(path.into()))
}

/// Builds a boot-recordable kernel source spec for a local path.
pub fn path_source_spec(path: impl Into<PathBuf>) -> LibSourceSpec {
    LibSourceSpec::open(path_source_kind(), path_payload(path.into()))
}

/// Builds a catalog source for a local path.
pub fn catalog_path_source(path: impl Into<PathBuf>) -> CatalogSource {
    CatalogSource::open(path_source_kind(), path_payload(path.into()))
}

/// Builds a live kernel source for a URL.
pub fn url_source(url: impl Into<String>) -> LibSource {
    LibSource::open(url_source_kind(), Datum::String(url.into()))
}

/// Builds a boot-recordable kernel source spec for a URL.
pub fn url_source_spec(url: impl Into<String>) -> LibSourceSpec {
    LibSourceSpec::open(url_source_kind(), Datum::String(url.into()))
}

/// Builds a catalog source for a URL.
pub fn catalog_url_source(url: impl Into<String>) -> CatalogSource {
    CatalogSource::open(url_source_kind(), Datum::String(url.into()))
}

/// Builds a live kernel source for in-memory artifact bytes.
pub fn bytes_source(bytes: impl Into<Vec<u8>>) -> LibSource {
    LibSource::open(bytes_source_kind(), Datum::Bytes(bytes.into()))
}

/// Builds a boot-recordable kernel source spec for in-memory artifact bytes.
pub fn bytes_source_spec(bytes: impl Into<Vec<u8>>) -> LibSourceSpec {
    LibSourceSpec::open(bytes_source_kind(), Datum::Bytes(bytes.into()))
}

/// Builds a catalog source for in-memory artifact bytes.
pub fn catalog_bytes_source(bytes: impl Into<Vec<u8>>) -> CatalogSource {
    CatalogSource::open(bytes_source_kind(), Datum::Bytes(bytes.into()))
}

/// Builds a live kernel source for a content-addressed artifact payload.
pub fn content_address_source(payload: Datum) -> LibSource {
    LibSource::open(content_address_source_kind(), payload)
}

/// Builds a boot-recordable kernel source spec for content-addressed artifacts.
pub fn content_address_source_spec(payload: Datum) -> LibSourceSpec {
    LibSourceSpec::open(content_address_source_kind(), payload)
}

/// Builds a catalog source for content-addressed artifacts.
pub fn catalog_content_address_source(payload: Datum) -> CatalogSource {
    CatalogSource::open(content_address_source_kind(), payload)
}

/// Returns whether a live source has the local path kind.
pub fn is_path_source(source: &LibSource) -> bool {
    open_payload(source, &path_source_kind()).is_some()
}

/// Returns whether a live source has the URL kind.
pub fn is_url_source(source: &LibSource) -> bool {
    open_payload(source, &url_source_kind()).is_some()
}

/// Returns whether a live source has the bytes kind.
pub fn is_bytes_source(source: &LibSource) -> bool {
    open_payload(source, &bytes_source_kind()).is_some()
}

/// Decodes a local path payload from a live source.
pub fn path_from_source(source: &LibSource) -> Result<Option<PathBuf>> {
    open_payload(source, &path_source_kind())
        .map(path_from_payload)
        .transpose()
}

/// Decodes a URL payload from a live source.
pub fn url_from_source(source: &LibSource) -> Result<Option<String>> {
    open_payload(source, &url_source_kind())
        .map(url_from_payload)
        .transpose()
}

/// Decodes an in-memory byte payload from a live source.
pub fn bytes_from_source(source: &LibSource) -> Result<Option<Vec<u8>>> {
    open_payload(source, &bytes_source_kind())
        .map(bytes_from_payload)
        .transpose()
}

/// Decodes the opaque content-address payload from a live source.
pub fn content_address_payload(source: &LibSource) -> Option<&Datum> {
    open_payload(source, &content_address_source_kind())
}

/// Encodes a local path as a loader-defined payload.
pub fn path_payload(path: impl AsRef<Path>) -> Datum {
    #[cfg(unix)]
    {
        Datum::Bytes(path.as_ref().as_os_str().as_bytes().to_vec())
    }

    #[cfg(not(unix))]
    {
        Datum::String(path.as_ref().to_string_lossy().into_owned())
    }
}

/// Decodes a local path from a loader-defined payload.
pub fn path_from_payload(payload: &Datum) -> Result<PathBuf> {
    match payload {
        Datum::String(path) => Ok(PathBuf::from(path)),
        #[cfg(unix)]
        Datum::Bytes(bytes) => Ok(PathBuf::from(OsString::from_vec(bytes.clone()))),
        #[cfg(not(unix))]
        Datum::Bytes(_) => Err(Error::HostError(
            "path source byte payloads are only supported on unix".to_owned(),
        )),
        other => Err(Error::TypeMismatch {
            expected: "path source string or bytes",
            found: datum_kind(other),
        }),
    }
}

/// Decodes a URL from a loader-defined payload.
pub fn url_from_payload(payload: &Datum) -> Result<String> {
    string_payload(payload)
}

/// Decodes in-memory artifact bytes from a loader-defined payload.
pub fn bytes_from_payload(payload: &Datum) -> Result<Vec<u8>> {
    bytes_payload(payload)
}

fn open_payload<'a>(source: &'a LibSource, expected_kind: &Symbol) -> Option<&'a Datum> {
    match source {
        LibSource::Open { kind, payload } if kind == expected_kind => Some(payload),
        _ => None,
    }
}

fn string_payload(payload: &Datum) -> Result<String> {
    match payload {
        Datum::String(value) => Ok(value.clone()),
        other => Err(Error::TypeMismatch {
            expected: "source string",
            found: datum_kind(other),
        }),
    }
}

fn bytes_payload(payload: &Datum) -> Result<Vec<u8>> {
    match payload {
        Datum::Bytes(value) => Ok(value.clone()),
        other => Err(Error::TypeMismatch {
            expected: "source bytes",
            found: datum_kind(other),
        }),
    }
}

fn datum_kind(datum: &Datum) -> &'static str {
    match datum {
        Datum::Nil => "datum nil",
        Datum::Bool(_) => "datum bool",
        Datum::Number(_) => "datum number",
        Datum::Symbol(_) => "datum symbol",
        Datum::String(_) => "datum string",
        Datum::Bytes(_) => "datum bytes",
        Datum::List(_) => "datum list",
        Datum::Vector(_) => "datum vector",
        Datum::Map(_) => "datum map",
        Datum::Set(_) => "datum set",
        Datum::Node { .. } => "datum node",
    }
}
