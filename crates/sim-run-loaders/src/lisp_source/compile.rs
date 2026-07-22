use sim_kernel::Result;

#[cfg(feature = "shape")]
use crate::reexport::SourceMacroSpec;
use crate::{
    expr::{expr_kind, parse_symbol_text},
    reexport::{ReexportKind, ReexportSpec},
};

pub(crate) struct CompiledSourceLib {
    pub(crate) manifest: sim_kernel::LibManifest,
    pub(crate) exports: Vec<ReexportSpec>,
    #[cfg(feature = "shape")]
    pub(crate) macros: Vec<SourceMacroSpec>,
    #[cfg(not(feature = "shape"))]
    pub(crate) macros: Vec<()>,
}

pub(crate) fn compile_lisp_source_parts(
    path: std::path::PathBuf,
    expr: sim_kernel::Expr,
) -> Result<CompiledSourceLib> {
    let entries = match expr {
        sim_kernel::Expr::List(entries) => entries,
        other => {
            return Err(sim_kernel::Error::Lib(format!(
                "expected sim_lib manifest dialect form, found {:?}",
                expr_kind(&other)
            )));
        }
    };
    let Some((head, tail)) = entries.split_first() else {
        return Err(sim_kernel::Error::Lib(
            "empty lisp source lib form".to_owned(),
        ));
    };
    expect_symbol(head, "sim_lib")?;

    let mut id = sim_kernel::Symbol::new(default_lib_name(&path));
    let mut version = "0.1.0".to_owned();
    let mut requires = Vec::new();
    let mut capabilities = Vec::new();
    let mut exports = Vec::new();
    #[cfg(feature = "shape")]
    let mut macros = Vec::new();
    #[cfg(not(feature = "shape"))]
    let macros = Vec::new();

    for entry in tail {
        let items = expect_list(entry, "sim_lib clause")?;
        let Some((tag, values)) = items.split_first() else {
            return Err(sim_kernel::Error::Lib("empty sim_lib clause".to_owned()));
        };
        let tag = expect_any_symbol(tag)?;
        match tag.as_qualified_str().as_str() {
            "id" => {
                let [symbol] = values else {
                    return Err(sim_kernel::Error::Lib(
                        "(id <symbol>) expects exactly one symbol".to_owned(),
                    ));
                };
                id = expect_symbolish(symbol)?;
            }
            "version" => {
                let [text] = values else {
                    return Err(sim_kernel::Error::Lib(
                        "(version <string>) expects exactly one string".to_owned(),
                    ));
                };
                version = expect_string(text)?;
            }
            "require" => match values {
                [dep] => requires.push(sim_kernel::Dependency {
                    id: expect_symbolish(dep)?,
                    minimum_version: None,
                }),
                [dep, min] => requires.push(sim_kernel::Dependency {
                    id: expect_symbolish(dep)?,
                    minimum_version: Some(sim_kernel::Version(expect_string(min)?)),
                }),
                _ => {
                    return Err(sim_kernel::Error::Lib(
                        "(require <symbol> [<version>]) expects one or two values".to_owned(),
                    ));
                }
            },
            "capability" => {
                let [name] = values else {
                    return Err(sim_kernel::Error::Lib(
                        "(capability <string>) expects exactly one string".to_owned(),
                    ));
                };
                capabilities.push(sim_kernel::CapabilityName::new(expect_string(name)?));
            }
            "export" => exports.push(parse_source_export(values)?),
            "defmacro" => {
                #[cfg(feature = "shape")]
                {
                    macros.push(parse_source_defmacro(values)?);
                }
                #[cfg(not(feature = "shape"))]
                {
                    let _ = values;
                    return Err(sim_kernel::Error::Lib(
                        "lisp-source defmacro requires the shape feature".to_owned(),
                    ));
                }
            }
            other => {
                return Err(sim_kernel::Error::Lib(format!(
                    "unknown sim_lib clause {other}"
                )));
            }
        }
    }

    #[cfg(feature = "shape")]
    {
        for mac in &macros {
            exports.push(ReexportSpec::new(
                ReexportKind::Macro,
                mac.symbol.clone(),
                mac.symbol.clone(),
            ));
        }
    }

    let manifest = sim_kernel::LibManifest {
        id,
        version: sim_kernel::Version(version),
        abi: sim_kernel::AbiVersion { major: 0, minor: 1 },
        target: sim_kernel::LibTarget::CodecSource(sim_kernel::Symbol::qualified("codec", "lisp")),
        requires,
        capabilities,
        exports: exports
            .iter()
            .map(|export| match export.kind {
                ReexportKind::Class => sim_kernel::Export::Class {
                    symbol: export.export.clone(),
                    class_id: None,
                },
                ReexportKind::Function => sim_kernel::Export::Function {
                    symbol: export.export.clone(),
                    function_id: None,
                },
                ReexportKind::Macro => sim_kernel::Export::Macro {
                    symbol: export.export.clone(),
                    macro_id: None,
                },
                ReexportKind::Shape => sim_kernel::Export::Shape {
                    symbol: export.export.clone(),
                    shape_id: None,
                },
                ReexportKind::Codec => sim_kernel::Export::Codec {
                    symbol: export.export.clone(),
                    codec_id: None,
                },
                ReexportKind::NumberDomain => sim_kernel::Export::NumberDomain {
                    symbol: export.export.clone(),
                    number_domain_id: None,
                },
                ReexportKind::Value => sim_kernel::Export::Value {
                    symbol: export.export.clone(),
                },
            })
            .collect(),
    };
    Ok(CompiledSourceLib {
        manifest,
        exports,
        macros,
    })
}

fn parse_source_export(values: &[sim_kernel::Expr]) -> Result<ReexportSpec> {
    let [kind, export, target] = values else {
        return Err(sim_kernel::Error::Lib(
            "(export <kind> <symbol> <target-symbol>) expects exactly three values".to_owned(),
        ));
    };
    let kind = match kind {
        sim_kernel::Expr::String(text) => text.clone(),
        _ => expect_any_symbol(kind)?.as_qualified_str(),
    };
    let kind = match kind.as_str() {
        "class" => ReexportKind::Class,
        "function" => ReexportKind::Function,
        "macro" => ReexportKind::Macro,
        "shape" => ReexportKind::Shape,
        "codec" => ReexportKind::Codec,
        "number-domain" => ReexportKind::NumberDomain,
        "value" => ReexportKind::Value,
        other => {
            return Err(sim_kernel::Error::Lib(format!(
                "unknown export kind {other}"
            )));
        }
    };
    Ok(ReexportSpec::new(
        kind,
        expect_symbolish(export)?,
        expect_symbolish(target)?,
    ))
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
fn parse_source_defmacro(values: &[sim_kernel::Expr]) -> Result<SourceMacroSpec> {
    let [symbol, params, template] = values else {
        return Err(sim_kernel::Error::Lib(
            "(defmacro <symbol> (<params...> [&rest rest]) <template>) expects exactly three values"
                .to_owned(),
        ));
    };
    let symbol = expect_symbolish(symbol)?;
    let params = expect_list(params, "defmacro parameter list")?;
    let (fixed_params, rest_param) = parse_macro_params(params)?;
    Ok(SourceMacroSpec {
        symbol,
        fixed_params,
        rest_param,
        template: template.clone(),
    })
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
fn parse_macro_params(
    params: &[sim_kernel::Expr],
) -> Result<(Vec<sim_kernel::Symbol>, Option<sim_kernel::Symbol>)> {
    let mut fixed = Vec::new();
    let mut rest = None;
    let mut index = 0;
    while index < params.len() {
        let symbol = expect_symbolish(&params[index])?;
        let is_amp = symbol.namespace.is_none() && symbol.name.as_ref() == "&";
        let is_rest_keyword = symbol.namespace.is_none() && symbol.name.as_ref() == "&rest";
        if is_rest_keyword || is_amp {
            let rest_index = if is_rest_keyword {
                index
            } else {
                if index + 2 >= params.len() {
                    return Err(sim_kernel::Error::Lib(
                        "defmacro parameter list must use a single trailing &rest name".to_owned(),
                    ));
                }
                let rest_keyword = expect_symbolish(&params[index + 1])?;
                if rest_keyword.namespace.is_some() || rest_keyword.name.as_ref() != "rest" {
                    return Err(sim_kernel::Error::Lib(
                        "defmacro '&' marker must be followed by rest".to_owned(),
                    ));
                }
                index + 1
            };
            if rest.is_some() || rest_index + 2 != params.len() {
                return Err(sim_kernel::Error::Lib(
                    "defmacro parameter list must use a single trailing &rest name".to_owned(),
                ));
            }
            rest = Some(expect_symbolish(&params[rest_index + 1])?);
            index = rest_index + 2;
            continue;
        }
        if rest.is_some() {
            return Err(sim_kernel::Error::Lib(
                "defmacro fixed parameters cannot appear after &rest".to_owned(),
            ));
        }
        fixed.push(symbol);
        index += 1;
    }
    Ok((fixed, rest))
}

fn expect_list<'a>(expr: &'a sim_kernel::Expr, context: &str) -> Result<&'a [sim_kernel::Expr]> {
    let sim_kernel::Expr::List(items) = expr else {
        return Err(sim_kernel::Error::Lib(format!(
            "expected {context} to be a list"
        )));
    };
    Ok(items)
}

fn expect_symbol(expr: &sim_kernel::Expr, expected: &str) -> Result<()> {
    let symbol = expect_any_symbol(expr)?;
    if symbol.as_qualified_str() == expected {
        Ok(())
    } else {
        Err(sim_kernel::Error::Lib(format!(
            "expected symbol {expected}, found {}",
            symbol.as_qualified_str()
        )))
    }
}

fn expect_any_symbol(expr: &sim_kernel::Expr) -> Result<sim_kernel::Symbol> {
    match expr {
        sim_kernel::Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(sim_kernel::Error::Lib(format!(
            "expected symbol, found {:?}",
            expr_kind(expr)
        ))),
    }
}

fn expect_string(expr: &sim_kernel::Expr) -> Result<String> {
    match expr {
        sim_kernel::Expr::String(value) => Ok(value.clone()),
        _ => Err(sim_kernel::Error::Lib(format!(
            "expected string, found {:?}",
            expr_kind(expr)
        ))),
    }
}

fn expect_symbolish(expr: &sim_kernel::Expr) -> Result<sim_kernel::Symbol> {
    match expr {
        sim_kernel::Expr::Symbol(symbol) => Ok(symbol.clone()),
        sim_kernel::Expr::String(value) => Ok(parse_symbol_text(value)),
        _ => Err(sim_kernel::Error::Lib(format!(
            "expected symbol or string, found {:?}",
            expr_kind(expr)
        ))),
    }
}

fn default_lib_name(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("source-lib")
        .to_owned()
}
