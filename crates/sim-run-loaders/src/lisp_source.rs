mod compile;
#[cfg(feature = "shape")]
pub(crate) mod template;

#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
use sim_kernel::Symbol;
use sim_kernel::{Cx, Lib, LibLoader, LibSource, Result};

#[cfg(not(feature = "shape"))]
use crate::reexport::ReexportLib;
#[cfg(feature = "shape")]
use crate::reexport::SourceLib;

/// Loader that compiles `.lisp` source files into libs using a Lisp codec.
pub struct LispSourceLoader {
    codec: sim_kernel::Symbol,
}

impl Default for LispSourceLoader {
    fn default() -> Self {
        Self::new(sim_kernel::Symbol::qualified("codec", "lisp"))
    }
}

impl LispSourceLoader {
    /// Creates a loader that decodes source with the codec named `codec`.
    pub fn new(codec: sim_kernel::Symbol) -> Self {
        Self { codec }
    }
}

impl LibLoader for LispSourceLoader {
    fn can_load(&self, source: &LibSource) -> bool {
        crate::path_from_source(source).is_ok_and(|path| {
            path.is_some_and(|path| path.extension().is_some_and(|ext| ext == "lisp"))
        })
    }

    fn load(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        let Some(path) = crate::path_from_source(&source)? else {
            return Err(sim_kernel::Error::HostError(
                "lisp source loader received unsupported source".to_owned(),
            ));
        };
        let text = std::fs::read_to_string(&path).map_err(|err| {
            sim_kernel::Error::HostError(format!(
                "failed to read lisp source {}: {err}",
                path.display()
            ))
        })?;
        let expr = sim_codec::decode_with_codec(
            cx,
            &self.codec,
            sim_codec::Input::Text(text),
            sim_kernel::ReadPolicy::default(),
        )?;
        compile_lisp_source_lib(path, expr)
    }

    fn inspect_manifest(
        &self,
        cx: &mut Cx,
        source: &LibSource,
    ) -> Result<Option<sim_kernel::LibManifest>> {
        let Some(path) = crate::path_from_source(source)? else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&path).map_err(|err| {
            sim_kernel::Error::HostError(format!(
                "failed to read lisp source {}: {err}",
                path.display()
            ))
        })?;
        let expr = sim_codec::decode_with_codec(
            cx,
            &self.codec,
            sim_codec::Input::Text(text),
            sim_kernel::ReadPolicy::default(),
        )?;
        Ok(Some(
            compile::compile_lisp_source_parts(path, expr)?.manifest,
        ))
    }
}

#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
/// Compiles Lisp source text into an in-memory binary lib pack.
pub fn compile_lisp_source_text_to_pack(
    cx: &mut Cx,
    codec: &sim_kernel::Symbol,
    source_path: impl Into<std::path::PathBuf>,
    text: impl Into<String>,
) -> Result<crate::BinaryLibPack> {
    let path = source_path.into();
    let expr = sim_codec::decode_with_codec(
        cx,
        codec,
        sim_codec::Input::Text(text.into()),
        sim_kernel::ReadPolicy::default(),
    )?;
    compile_lisp_source_pack(path, expr)
}

#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
/// Compiles Lisp source text and encodes it as binary lib pack bytes.
pub fn encode_lisp_source_text_to_binary_pack(
    cx: &mut Cx,
    codec: &Symbol,
    source_path: impl Into<std::path::PathBuf>,
    text: impl Into<String>,
) -> Result<Vec<u8>> {
    let pack = compile_lisp_source_text_to_pack(cx, codec, source_path, text)?;
    crate::encode_binary_lib_pack(&pack)
}

#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
/// Reads a Lisp source file, compiles it, and writes a binary lib pack to disk.
pub fn export_lisp_source_file_to_binary_pack(
    cx: &mut Cx,
    codec: &Symbol,
    source_path: impl AsRef<std::path::Path>,
    output_path: impl AsRef<std::path::Path>,
) -> Result<()> {
    let source_path = source_path.as_ref();
    let output_path = output_path.as_ref();
    let text = std::fs::read_to_string(source_path).map_err(|err| {
        sim_kernel::Error::HostError(format!(
            "failed to read lisp source {}: {err}",
            source_path.display()
        ))
    })?;
    let bytes = encode_lisp_source_text_to_binary_pack(cx, codec, source_path.to_path_buf(), text)?;
    std::fs::write(output_path, bytes).map_err(|err| {
        sim_kernel::Error::HostError(format!(
            "failed to write binary lib pack {}: {err}",
            output_path.display()
        ))
    })?;
    Ok(())
}

fn compile_lisp_source_lib(
    path: std::path::PathBuf,
    expr: sim_kernel::Expr,
) -> Result<Box<dyn Lib>> {
    let compiled = compile::compile_lisp_source_parts(path, expr)?;
    #[cfg(feature = "shape")]
    {
        Ok(Box::new(SourceLib::new(
            compiled.manifest,
            compiled.exports,
            compiled.macros,
        )))
    }
    #[cfg(not(feature = "shape"))]
    {
        if !compiled.macros.is_empty() {
            return Err(sim_kernel::Error::Lib(
                "lisp-source defmacro requires the shape feature".to_owned(),
            ));
        }
        Ok(Box::new(ReexportLib::new(
            compiled.manifest,
            compiled.exports,
        )))
    }
}

#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
/// Compiles an already-decoded Lisp source expression into a binary lib pack.
pub fn compile_lisp_source_pack(
    path: std::path::PathBuf,
    expr: sim_kernel::Expr,
) -> Result<crate::BinaryLibPack> {
    let compiled = compile::compile_lisp_source_parts(path, expr)?;
    if !compiled.macros.is_empty() {
        return Err(sim_kernel::Error::Lib(
            "binary lib packs do not encode Lisp-authored defmacro bodies".to_owned(),
        ));
    }
    Ok(crate::BinaryLibPack {
        manifest: compiled.manifest,
        exports: compiled.exports,
    })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use sim_kernel::{Expr, LibLoader, LibTarget, Symbol};

    use super::{LispSourceLoader, compile_lisp_source_lib};

    #[test]
    fn lisp_source_loader_accepts_lisp_paths() {
        let loader = LispSourceLoader::default();
        assert!(loader.can_load(&crate::path_source(PathBuf::from("lib.lisp"))));
        assert!(!loader.can_load(&crate::bytes_source(Vec::new())));
    }

    #[cfg(feature = "codec-binary")]
    #[test]
    fn lisp_source_expr_compiles_to_binary_pack() {
        let pack =
            super::compile_lisp_source_pack(PathBuf::from("demo.lisp"), source_expr()).unwrap();

        assert_eq!(pack.manifest.id, Symbol::qualified("loader", "source-demo"));
        assert_eq!(
            pack.manifest.target,
            LibTarget::CodecSource(Symbol::qualified("codec", "lisp"))
        );
        assert_eq!(pack.exports.len(), 1);
        assert_eq!(pack.exports[0].kind(), crate::ReexportKind::Function);
    }

    #[cfg(feature = "shape")]
    #[test]
    fn lisp_source_defmacro_loads_source_macro_object() {
        let lib =
            compile_lisp_source_lib(PathBuf::from("macro.lisp"), source_defmacro_expr()).unwrap();
        let mut cx = sim_kernel::Cx::new(
            Arc::new(sim_kernel::EagerPolicy),
            Arc::new(sim_kernel::DefaultFactory),
        );

        cx.load_lib(lib.as_ref()).unwrap();

        let value = cx
            .registry()
            .macro_by_symbol(&Symbol::qualified("loader", "when"))
            .unwrap();
        let mac = value.object().downcast_ref::<crate::SourceTemplateMacro>();
        assert!(mac.is_some());
    }

    fn source_expr() -> Expr {
        Expr::List(vec![
            symbol("sim_lib"),
            Expr::List(vec![
                symbol("id"),
                Expr::String("loader/source-demo".to_owned()),
            ]),
            Expr::List(vec![symbol("version"), Expr::String("0.2.0".to_owned())]),
            Expr::List(vec![
                symbol("export"),
                symbol("function"),
                Expr::String("loader/tick".to_owned()),
                symbol("tick"),
            ]),
        ])
    }

    #[cfg(feature = "shape")]
    fn source_defmacro_expr() -> Expr {
        Expr::List(vec![
            symbol("sim_lib"),
            Expr::List(vec![
                symbol("id"),
                Expr::String("loader/source-defmacro-demo".to_owned()),
            ]),
            Expr::List(vec![symbol("version"), Expr::String("0.2.0".to_owned())]),
            Expr::List(vec![
                symbol("defmacro"),
                Expr::String("loader/when".to_owned()),
                Expr::List(vec![symbol("condition"), symbol("&rest"), symbol("body")]),
                Expr::Quote {
                    mode: sim_kernel::QuoteMode::QuasiQuote,
                    expr: Box::new(Expr::List(vec![
                        symbol("if"),
                        Expr::Quote {
                            mode: sim_kernel::QuoteMode::Unquote,
                            expr: Box::new(symbol("condition")),
                        },
                        Expr::List(vec![
                            symbol("do"),
                            Expr::Quote {
                                mode: sim_kernel::QuoteMode::Splice,
                                expr: Box::new(symbol("body")),
                            },
                        ]),
                        Expr::Nil,
                    ])),
                },
            ]),
        ])
    }

    fn symbol(name: &str) -> Expr {
        Expr::Symbol(match name.split_once('/') {
            Some((namespace, name)) => Symbol::qualified(namespace, name),
            None => Symbol::new(name),
        })
    }
}
