use sim_kernel::{AbiVersion, CapabilityName, Dependency, Export, Expr, LibManifest, Result};

use crate::shared::{expr_kind, parse_symbol_text};

pub(crate) fn manifest_to_expr(manifest: &LibManifest) -> Expr {
    Expr::Map(vec![
        symbol_entry("id", Expr::Symbol(manifest.id.clone())),
        symbol_entry("version", Expr::String(manifest.version.0.clone())),
        symbol_entry("abi-major", number_expr(manifest.abi.major)),
        symbol_entry("abi-minor", number_expr(manifest.abi.minor)),
        symbol_entry("target", Expr::String(lib_target_name(&manifest.target))),
        symbol_entry(
            "requires",
            Expr::List(
                manifest
                    .requires
                    .iter()
                    .map(|dependency| {
                        Expr::Map(vec![
                            symbol_entry("id", Expr::Symbol(dependency.id.clone())),
                            symbol_entry(
                                "minimum-version",
                                dependency
                                    .minimum_version
                                    .as_ref()
                                    .map(|version| Expr::String(version.0.clone()))
                                    .unwrap_or(Expr::Nil),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        symbol_entry(
            "capabilities",
            Expr::List(
                manifest
                    .capabilities
                    .iter()
                    .map(|capability| Expr::String(capability.as_str().to_owned()))
                    .collect(),
            ),
        ),
        symbol_entry(
            "exports",
            Expr::List(
                manifest
                    .exports
                    .iter()
                    .map(|export| {
                        Expr::Map(vec![
                            symbol_entry(
                                "kind",
                                Expr::String(export.kind_symbol().symbol().as_qualified_str()),
                            ),
                            symbol_entry("symbol", Expr::Symbol(export.symbol().clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

pub(crate) fn expr_to_manifest(expr: Expr) -> Result<LibManifest> {
    Ok(LibManifest {
        id: expect_symbol_field(&expr, "id")?,
        version: sim_kernel::Version(expect_string_field(&expr, "version")?),
        abi: AbiVersion {
            major: expect_u16_field(&expr, "abi-major")?,
            minor: expect_u16_field(&expr, "abi-minor")?,
        },
        target: parse_lib_target(&expect_string_field(&expr, "target")?)?,
        requires: expect_list_field(&expr, "requires")?
            .into_iter()
            .map(|entry| {
                Ok(Dependency {
                    id: expect_symbol_field(&entry, "id")?,
                    minimum_version: expect_optional_string_field(&entry, "minimum-version")?
                        .map(sim_kernel::Version),
                })
            })
            .collect::<Result<Vec<_>>>()?,
        capabilities: expect_list_field(&expr, "capabilities")?
            .into_iter()
            .map(|entry| match entry {
                Expr::String(capability) => Ok(CapabilityName::new(capability)),
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

fn expr_to_manifest_export(expr: Expr) -> Result<Export> {
    let kind = expect_string_field(&expr, "kind")?;
    let symbol = expect_symbol_field(&expr, "symbol")?;
    match kind.as_str() {
        "class" => Ok(Export::Class {
            symbol,
            class_id: None,
        }),
        "function" => Ok(Export::Function {
            symbol,
            function_id: None,
        }),
        "macro" => Ok(Export::Macro {
            symbol,
            macro_id: None,
        }),
        "shape" => Ok(Export::Shape {
            symbol,
            shape_id: None,
        }),
        "codec" => Ok(Export::Codec {
            symbol,
            codec_id: None,
        }),
        "number-domain" => Ok(Export::NumberDomain {
            symbol,
            number_domain_id: None,
        }),
        "site" => Ok(Export::Site {
            symbol,
            runtime_id: None,
        }),
        "value" => Ok(Export::Value { symbol }),
        other => Err(sim_kernel::Error::Lib(format!(
            "unknown manifest export kind {other}"
        ))),
    }
}

fn symbol_entry(key: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(sim_kernel::Symbol::new(key)), value)
}

fn number_expr(value: impl ToString) -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
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

fn expect_map_field<'a>(expr: &'a Expr, field: &str) -> Result<&'a Expr> {
    let Expr::Map(entries) = expr else {
        return Err(sim_kernel::Error::Lib(format!(
            "expected map expr for field lookup, found {:?}",
            expr_kind(expr)
        )));
    };
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Expr::Symbol(symbol) if symbol.name.as_ref() == field && symbol.namespace.is_none() => {
                Some(value)
            }
            _ => None,
        })
        .ok_or_else(|| sim_kernel::Error::Lib(format!("missing field {field}")))
}

fn expect_list_field(expr: &Expr, field: &str) -> Result<Vec<Expr>> {
    match expect_map_field(expr, field)? {
        Expr::List(items) => Ok(items.clone()),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected list field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_symbol_field(expr: &Expr, field: &str) -> Result<sim_kernel::Symbol> {
    match expect_map_field(expr, field)? {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        Expr::String(value) => Ok(parse_symbol_text(value)),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected symbol field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_string_field(expr: &Expr, field: &str) -> Result<String> {
    match expect_map_field(expr, field)? {
        Expr::String(value) => Ok(value.clone()),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected string field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_optional_string_field(expr: &Expr, field: &str) -> Result<Option<String>> {
    match expect_map_field(expr, field)? {
        Expr::Nil => Ok(None),
        Expr::String(value) => Ok(Some(value.clone())),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected optional string field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}

fn expect_u16_field(expr: &Expr, field: &str) -> Result<u16> {
    match expect_map_field(expr, field)? {
        Expr::Number(number) => number
            .canonical
            .parse::<u16>()
            .map_err(|err| sim_kernel::Error::Lib(format!("invalid {field} number: {err}"))),
        other => Err(sim_kernel::Error::Lib(format!(
            "expected numeric field {field}, found {:?}",
            expr_kind(other)
        ))),
    }
}
