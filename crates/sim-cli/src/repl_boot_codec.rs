use std::sync::Arc;

use sim_codec::{CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, Output};
use sim_kernel::{
    AbiVersion, CodecId, Dependency, Export, Expr, Lib, LibManifest, LibTarget, NumberLiteral,
    Result, Symbol, Version,
};

pub(crate) struct BootLispCodecLib {
    id: CodecId,
}

impl BootLispCodecLib {
    pub(crate) fn new(id: CodecId) -> Self {
        Self { id }
    }
}

impl Lib for BootLispCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: codec_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: codec_symbol(),
                codec_id: Some(self.id),
            }],
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut sim_kernel::Linker<'_>) -> Result<()> {
        let any_shape = cx.factory().nil()?;
        let runtime = CodecRuntime {
            id: self.id,
            symbol: codec_symbol(),
            decoder: Some(Arc::new(BootLispDecoder)),
            located_decoder: None,
            tree_decoder: None,
            encoder: Some(Arc::new(BootLispEncoder)),
            located_encoder: None,
            tree_encoder: None,
            expr_shape: any_shape.clone(),
            options_shape: any_shape,
            default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
        };
        linker.codec_value(codec_symbol(), sim_codec::codec_value(runtime))?;
        Ok(())
    }
}

struct BootLispDecoder;

impl Decoder for BootLispDecoder {
    fn decode(&self, cx: &mut sim_codec::ReadCx<'_>, input: Input) -> Result<Expr> {
        let text = input.into_string()?;
        Parser::new(cx.codec, &text).parse()
    }
}

struct BootLispEncoder;

impl Encoder for BootLispEncoder {
    fn encode(&self, cx: &mut sim_kernel::WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_expr(cx.codec, expr).map(Output::Text)
    }
}

struct Parser<'a> {
    codec: CodecId,
    tokens: Vec<Token<'a>>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(codec: CodecId, text: &'a str) -> Self {
        Self {
            codec,
            tokens: tokenize(text),
            index: 0,
        }
    }

    fn parse(mut self) -> Result<Expr> {
        let expr = self.parse_expr()?;
        if self.index != self.tokens.len() {
            return Err(codec_error(self.codec, "unexpected trailing input"));
        }
        Ok(expr)
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Token::LParen) => self.parse_list(),
            Some(Token::RParen) => Err(codec_error(self.codec, "unexpected ')'")),
            Some(Token::Atom(atom)) => Ok(parse_atom(atom)),
            None => Err(codec_error(self.codec, "empty input")),
        }
    }

    fn parse_list(&mut self) -> Result<Expr> {
        let mut items = Vec::new();
        while !matches!(self.peek(), Some(Token::RParen)) {
            if self.peek().is_none() {
                return Err(codec_error(self.codec, "missing ')'"));
            }
            items.push(self.parse_expr()?);
        }
        self.index += 1;
        Ok(match items.as_slice() {
            [Expr::Symbol(symbol), quoted] if symbol == &Symbol::new("quote") => Expr::Quote {
                mode: sim_kernel::QuoteMode::Quote,
                expr: Box::new(quoted.clone()),
            },
            _ => Expr::List(items),
        })
    }

    fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.index).copied()
    }

    fn next(&mut self) -> Option<Token<'a>> {
        let token = self.peek()?;
        self.index += 1;
        Some(token)
    }
}

#[derive(Clone, Copy)]
enum Token<'a> {
    LParen,
    RParen,
    Atom(&'a str),
}

fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' | ')' | ' ' | '\n' | '\r' | '\t' => {
                if let Some(atom_start) = start.take() {
                    tokens.push(Token::Atom(&text[atom_start..index]));
                }
                match ch {
                    '(' => tokens.push(Token::LParen),
                    ')' => tokens.push(Token::RParen),
                    _ => {}
                }
            }
            _ if start.is_none() => start = Some(index),
            _ => {}
        }
    }
    if let Some(atom_start) = start {
        tokens.push(Token::Atom(&text[atom_start..]));
    }
    tokens
}

fn parse_atom(atom: &str) -> Expr {
    match atom {
        "nil" => Expr::Nil,
        "true" => Expr::Bool(true),
        "false" => Expr::Bool(false),
        _ if atom.parse::<f64>().is_ok() => Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: atom.to_owned(),
        }),
        _ => Expr::Symbol(parse_symbol(atom)),
    }
}

fn parse_symbol(text: &str) -> Symbol {
    text.split_once('/')
        .map(|(namespace, name)| Symbol::qualified(namespace, name))
        .unwrap_or_else(|| Symbol::new(text))
}

fn encode_expr(codec: CodecId, expr: &Expr) -> Result<String> {
    Ok(match expr {
        Expr::Nil => "nil".to_owned(),
        Expr::Bool(true) => "true".to_owned(),
        Expr::Bool(false) => "false".to_owned(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::Symbol(symbol) => symbol.to_string(),
        Expr::String(text) => format!("{text:?}"),
        Expr::List(items) => format_list(codec, items.iter())?,
        Expr::Call { operator, args } => {
            format_list(codec, std::iter::once(operator.as_ref()).chain(args.iter()))?
        }
        Expr::Quote { mode, expr } if *mode == sim_kernel::QuoteMode::Quote => {
            format!("(quote {})", encode_expr(codec, expr)?)
        }
        _ => {
            return Err(codec_error(
                codec,
                "boot Lisp codec cannot encode expression",
            ));
        }
    })
}

fn format_list<'a>(codec: CodecId, items: impl IntoIterator<Item = &'a Expr>) -> Result<String> {
    let parts = items
        .into_iter()
        .map(|item| encode_expr(codec, item))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("({})", parts.join(" ")))
}

fn codec_symbol() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

fn codec_error(codec: CodecId, message: &str) -> sim_kernel::Error {
    sim_kernel::Error::CodecError {
        codec,
        message: message.to_owned(),
    }
}
