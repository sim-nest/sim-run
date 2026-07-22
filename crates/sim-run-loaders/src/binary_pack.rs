use sim_kernel::{Cx, Lib, LibLoader, LibSource, Result};

use crate::{
    bytes_from_source,
    expr::{expr_kind, parse_symbol_text},
    path_from_source,
    reexport::{ReexportKind, ReexportLib, ReexportSpec},
    url_from_source,
};

const BINARY_PACK_MAGIC: &[u8; 4] = b"L8PK";
const BINARY_PACK_VERSION: u32 = 1;

/// Loader for `.l8b` binary lib packs, recognized by path extension or magic.
pub struct BinaryPackLoader;

impl Default for BinaryPackLoader {
    fn default() -> Self {
        Self
    }
}

impl LibLoader for BinaryPackLoader {
    fn can_load(&self, source: &LibSource) -> bool {
        if let Ok(Some(path)) = path_from_source(source) {
            return path.extension().is_some_and(|ext| ext == "l8b");
        }
        if let Ok(Some(bytes)) = bytes_from_source(source) {
            return has_binary_pack_magic(&bytes);
        }
        false
    }

    fn load(&self, _cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        let bytes = read_pack_source(source)?;
        let pack = decode_binary_lib_pack(&bytes)?;
        Ok(Box::new(ReexportLib::new(pack.manifest, pack.exports)))
    }

    fn inspect_manifest(
        &self,
        _cx: &mut Cx,
        source: &LibSource,
    ) -> Result<Option<sim_kernel::LibManifest>> {
        let bytes = if let Some(path) = path_from_source(source)? {
            std::fs::read(&path).map_err(|err| {
                sim_kernel::Error::HostError(format!(
                    "failed to read binary lib pack {}: {err}",
                    path.display()
                ))
            })?
        } else if let Some(bytes) = bytes_from_source(source)? {
            bytes
        } else if let Some(url) = url_from_source(source)? {
            return Err(sim_kernel::Error::HostError(format!(
                "url inspection is not implemented for binary lib pack {url}"
            )));
        } else {
            return Ok(None);
        };
        Ok(Some(decode_binary_lib_pack(&bytes)?.manifest))
    }
}

/// Decoded contents of a binary lib pack: a manifest plus its re-export specs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryLibPack {
    /// Manifest of the packed lib.
    pub manifest: sim_kernel::LibManifest,
    /// Re-export entries the pack links when loaded.
    pub exports: Vec<ReexportSpec>,
}

/// Encodes a binary lib pack into its byte representation.
pub fn encode_binary_lib_pack(pack: &BinaryLibPack) -> Result<Vec<u8>> {
    let manifest_expr = manifest_to_expr(&pack.manifest);
    let exports_expr = reexports_to_expr(&pack.exports);
    let manifest = sim_codec_binary::encode_frame(&manifest_expr)?.0;
    let exports = sim_codec_binary::encode_frame(&exports_expr)?.0;

    let mut bytes = Vec::with_capacity(16 + manifest.len() + exports.len());
    bytes.extend_from_slice(BINARY_PACK_MAGIC);
    bytes.extend_from_slice(&BINARY_PACK_VERSION.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(manifest.len())
            .map_err(|_| sim_kernel::Error::HostError("manifest frame too large".to_owned()))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(exports.len())
            .map_err(|_| sim_kernel::Error::HostError("export frame too large".to_owned()))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&exports);
    Ok(bytes)
}

/// Decodes a binary lib pack from its byte representation, validating the
/// header.
pub fn decode_binary_lib_pack(bytes: &[u8]) -> Result<BinaryLibPack> {
    if !has_binary_pack_magic(bytes) {
        return Err(sim_kernel::Error::HostError(
            "invalid binary lib pack magic".to_owned(),
        ));
    }
    if bytes.len() < 16 {
        return Err(sim_kernel::Error::HostError(
            "binary lib pack header is truncated".to_owned(),
        ));
    }
    let version = read_u32(bytes, 4)?;
    if version != BINARY_PACK_VERSION {
        return Err(sim_kernel::Error::HostError(format!(
            "unsupported binary lib pack version {version}"
        )));
    }
    let manifest_len = read_u32(bytes, 8)? as usize;
    let exports_len = read_u32(bytes, 12)? as usize;
    let manifest_start: usize = 16;
    let manifest_end = manifest_start
        .checked_add(manifest_len)
        .ok_or_else(|| sim_kernel::Error::HostError("manifest frame length overflow".to_owned()))?;
    let exports_end = manifest_end
        .checked_add(exports_len)
        .ok_or_else(|| sim_kernel::Error::HostError("export frame length overflow".to_owned()))?;
    if exports_end != bytes.len() {
        return Err(sim_kernel::Error::HostError(
            "binary lib pack length does not match header".to_owned(),
        ));
    }
    let (_, manifest_expr) = sim_codec_binary::decode_frame(
        sim_kernel::CodecId(0),
        &bytes[manifest_start..manifest_end],
    )?;
    let (_, exports_expr) =
        sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes[manifest_end..exports_end])?;
    Ok(BinaryLibPack {
        manifest: expr_to_manifest(manifest_expr)?,
        exports: expr_to_reexports(exports_expr)?,
    })
}

fn read_pack_source(source: LibSource) -> Result<Vec<u8>> {
    if let Some(path) = path_from_source(&source)? {
        return std::fs::read(&path).map_err(|err| {
            sim_kernel::Error::HostError(format!(
                "failed to read binary lib pack {}: {err}",
                path.display()
            ))
        });
    }
    if let Some(bytes) = bytes_from_source(&source)? {
        return Ok(bytes);
    }
    if let Some(url) = url_from_source(&source)? {
        return Err(sim_kernel::Error::HostError(format!(
            "url loading is not implemented for binary lib pack {url}"
        )));
    }
    Err(sim_kernel::Error::HostError(
        "binary pack loader received unsupported source".to_owned(),
    ))
}

pub(super) fn manifest_to_expr(manifest: &sim_kernel::LibManifest) -> sim_kernel::Expr {
    sim_kernel::Expr::Map(vec![
        symbol_entry("id", sim_kernel::Expr::Symbol(manifest.id.clone())),
        symbol_entry(
            "version",
            sim_kernel::Expr::String(manifest.version.0.clone()),
        ),
        symbol_entry("abi-major", number_expr(manifest.abi.major)),
        symbol_entry("abi-minor", number_expr(manifest.abi.minor)),
        symbol_entry(
            "target",
            sim_kernel::Expr::String(lib_target_name(&manifest.target)),
        ),
        symbol_entry(
            "requires",
            sim_kernel::Expr::List(
                manifest
                    .requires
                    .iter()
                    .map(|dependency| {
                        sim_kernel::Expr::Map(vec![
                            symbol_entry("id", sim_kernel::Expr::Symbol(dependency.id.clone())),
                            symbol_entry(
                                "minimum-version",
                                dependency
                                    .minimum_version
                                    .as_ref()
                                    .map(|version| sim_kernel::Expr::String(version.0.clone()))
                                    .unwrap_or(sim_kernel::Expr::Nil),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        symbol_entry(
            "capabilities",
            sim_kernel::Expr::List(
                manifest
                    .capabilities
                    .iter()
                    .map(|capability| sim_kernel::Expr::String(capability.as_str().to_owned()))
                    .collect(),
            ),
        ),
        symbol_entry(
            "exports",
            sim_kernel::Expr::List(
                manifest
                    .exports
                    .iter()
                    .map(|export| {
                        sim_kernel::Expr::Map(vec![
                            symbol_entry(
                                "kind",
                                sim_kernel::Expr::String(
                                    export.kind_symbol().symbol().as_qualified_str(),
                                ),
                            ),
                            symbol_entry(
                                "symbol",
                                sim_kernel::Expr::Symbol(export.symbol().clone()),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

pub(super) fn expr_to_manifest(expr: sim_kernel::Expr) -> Result<sim_kernel::LibManifest> {
    Ok(sim_kernel::LibManifest {
        id: expect_symbol_field(&expr, "id")?,
        version: sim_kernel::Version(expect_string_field(&expr, "version")?),
        abi: sim_kernel::AbiVersion {
            major: expect_u16_field(&expr, "abi-major")?,
            minor: expect_u16_field(&expr, "abi-minor")?,
        },
        target: parse_lib_target(&expect_string_field(&expr, "target")?)?,
        requires: expect_list_field(&expr, "requires")?
            .into_iter()
            .map(|entry| {
                Ok(sim_kernel::Dependency {
                    id: expect_symbol_field(&entry, "id")?,
                    minimum_version: expect_optional_string_field(&entry, "minimum-version")?
                        .map(sim_kernel::Version),
                })
            })
            .collect::<Result<Vec<_>>>()?,
        capabilities: expect_list_field(&expr, "capabilities")?
            .into_iter()
            .map(|entry| match entry {
                sim_kernel::Expr::String(capability) => {
                    Ok(sim_kernel::CapabilityName::new(capability))
                }
                other => Err(sim_kernel::Error::Lib(format!(
                    "expected capability string, found {:?}",
                    expr_kind(&other)
                ))),
            })
            .collect::<Result<Vec<_>>>()?,
        exports: expect_list_field(&expr, "exports")?
            .into_iter()
            .map(expr_to_manifest_export)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn reexports_to_expr(exports: &[ReexportSpec]) -> sim_kernel::Expr {
    sim_kernel::Expr::List(
        exports
            .iter()
            .map(|export| {
                sim_kernel::Expr::Map(vec![
                    symbol_entry(
                        "kind",
                        sim_kernel::Expr::String(
                            match export.kind {
                                ReexportKind::Class => "class",
                                ReexportKind::Function => "function",
                                ReexportKind::Macro => "macro",
                                ReexportKind::Shape => "shape",
                                ReexportKind::Codec => "codec",
                                ReexportKind::NumberDomain => "number-domain",
                                ReexportKind::Value => "value",
                            }
                            .to_owned(),
                        ),
                    ),
                    symbol_entry("export", sim_kernel::Expr::Symbol(export.export.clone())),
                    symbol_entry("target", sim_kernel::Expr::Symbol(export.target.clone())),
                ])
            })
            .collect(),
    )
}

fn expr_to_reexports(expr: sim_kernel::Expr) -> Result<Vec<ReexportSpec>> {
    let sim_kernel::Expr::List(entries) = expr else {
        return Err(sim_kernel::Error::Lib(
            "expected binary lib pack reexports list".to_owned(),
        ));
    };
    entries
        .into_iter()
        .map(|entry| {
            let kind = match expect_string_field(&entry, "kind")?.as_str() {
                "class" => ReexportKind::Class,
                "function" => ReexportKind::Function,
                "macro" => ReexportKind::Macro,
                "shape" => ReexportKind::Shape,
                "codec" => ReexportKind::Codec,
                "number-domain" => ReexportKind::NumberDomain,
                "value" => ReexportKind::Value,
                other => {
                    return Err(sim_kernel::Error::Lib(format!(
                        "unknown reexport kind {other}"
                    )));
                }
            };
            Ok(ReexportSpec::new(
                kind,
                expect_symbol_field(&entry, "export")?,
                expect_symbol_field(&entry, "target")?,
            ))
        })
        .collect()
}

fn expr_to_manifest_export(expr: sim_kernel::Expr) -> Result<sim_kernel::Export> {
    let kind = expect_string_field(&expr, "kind")?;
    let symbol = expect_symbol_field(&expr, "symbol")?;
    match kind.as_str() {
        "class" => Ok(sim_kernel::Export::Class {
            symbol,
            class_id: None,
        }),
        "function" => Ok(sim_kernel::Export::Function {
            symbol,
            function_id: None,
        }),
        "macro" => Ok(sim_kernel::Export::Macro {
            symbol,
            macro_id: None,
        }),
        "shape" => Ok(sim_kernel::Export::Shape {
            symbol,
            shape_id: None,
        }),
        "codec" => Ok(sim_kernel::Export::Codec {
            symbol,
            codec_id: None,
        }),
        "number-domain" => Ok(sim_kernel::Export::NumberDomain {
            symbol,
            number_domain_id: None,
        }),
        "value" => Ok(sim_kernel::Export::Value { symbol }),
        other => Err(sim_kernel::Error::Lib(format!(
            "unknown manifest export kind {other}"
        ))),
    }
}

fn symbol_entry(key: &str, value: sim_kernel::Expr) -> (sim_kernel::Expr, sim_kernel::Expr) {
    (
        sim_kernel::Expr::Symbol(sim_kernel::Symbol::new(key)),
        value,
    )
}

fn number_expr(value: impl ToString) -> sim_kernel::Expr {
    sim_kernel::Expr::Number(sim_kernel::NumberLiteral {
        domain: sim_kernel::Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

fn lib_target_name(target: &sim_kernel::LibTarget) -> String {
    target.to_symbol().as_qualified_str()
}

fn parse_lib_target(name: &str) -> Result<sim_kernel::LibTarget> {
    Ok(sim_kernel::LibTarget::from_symbol(&parse_symbol_text(name)))
}

fn expect_map_field<'a>(expr: &'a sim_kernel::Expr, field: &str) -> Result<&'a sim_kernel::Expr> {
    let sim_kernel::Expr::Map(entries) = expr else {
        return Err(sim_kernel::Error::Lib(format!(
            "expected map expr for field lookup, found {:?}",
            expr_kind(expr)
        )));
    };
    entries
        .iter()
        .find_map(|(key, value)| match key {
            sim_kernel::Expr::Symbol(symbol)
                if symbol.name.as_ref() == field && symbol.namespace.is_none() =>
            {
                Some(value)
            }
            _ => None,
        })
        .ok_or_else(|| sim_kernel::Error::Lib(format!("missing field {field}")))
}

fn expect_list_field(expr: &sim_kernel::Expr, field: &str) -> Result<Vec<sim_kernel::Expr>> {
    match expect_map_field(expr, field)? {
        sim_kernel::Expr::List(items) => Ok(items.clone()),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected list field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_symbol_field(expr: &sim_kernel::Expr, field: &str) -> Result<sim_kernel::Symbol> {
    match expect_map_field(expr, field)? {
        sim_kernel::Expr::Symbol(symbol) => Ok(symbol.clone()),
        sim_kernel::Expr::String(value) => Ok(parse_symbol_text(value)),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected symbol field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_string_field(expr: &sim_kernel::Expr, field: &str) -> Result<String> {
    match expect_map_field(expr, field)? {
        sim_kernel::Expr::String(value) => Ok(value.clone()),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected string field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_optional_string_field(expr: &sim_kernel::Expr, field: &str) -> Result<Option<String>> {
    match expect_map_field(expr, field)? {
        sim_kernel::Expr::Nil => Ok(None),
        sim_kernel::Expr::String(value) => Ok(Some(value.clone())),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected optional string field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_u16_field(expr: &sim_kernel::Expr, field: &str) -> Result<u16> {
    match expect_map_field(expr, field)? {
        sim_kernel::Expr::Number(number) => number
            .canonical
            .parse::<u16>()
            .map_err(|err| sim_kernel::Error::Lib(format!("invalid {field} number: {err}"))),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected numeric field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn has_binary_pack_magic(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(BINARY_PACK_MAGIC.as_slice())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let raw = bytes.get(offset..offset + 4).ok_or_else(|| {
        sim_kernel::Error::HostError("binary lib pack header is truncated".to_owned())
    })?;
    let mut buf = [0u8; 4];
    buf.copy_from_slice(raw);
    Ok(u32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests;
