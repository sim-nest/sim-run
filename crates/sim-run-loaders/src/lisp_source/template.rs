use sim_kernel::{Error, Expr, QuoteMode, Result};
use sim_shape::Bindings;

pub(crate) fn instantiate_macro_template(template: &Expr, captures: &Bindings) -> Result<Expr> {
    match template {
        Expr::Quote {
            mode: QuoteMode::QuasiQuote,
            expr,
        } => match instantiate_quasiquote(expr, captures, 1)? {
            TemplatePart::One(expr) => Ok(expr),
            TemplatePart::Many(_) => Err(Error::Eval(
                "top-level macro template cannot splice multiple expressions".to_owned(),
            )),
        },
        Expr::Quote {
            mode: QuoteMode::Unquote | QuoteMode::Splice,
            ..
        } => Err(Error::Eval(
            "macro template cannot start with unquote or splice outside quasiquote".to_owned(),
        )),
        other => Ok(other.clone()),
    }
}

enum TemplatePart {
    One(Expr),
    Many(Vec<Expr>),
}

fn instantiate_quasiquote(expr: &Expr, captures: &Bindings, depth: usize) -> Result<TemplatePart> {
    match expr {
        Expr::Quote { mode, expr } => match mode {
            QuoteMode::QuasiQuote => Ok(TemplatePart::One(Expr::Quote {
                mode: QuoteMode::QuasiQuote,
                expr: Box::new(expect_one(instantiate_quasiquote(
                    expr,
                    captures,
                    depth + 1,
                )?)?),
            })),
            QuoteMode::Unquote => {
                if depth == 1 {
                    Ok(TemplatePart::One(resolve_capture_expr(expr, captures)?))
                } else {
                    Ok(TemplatePart::One(Expr::Quote {
                        mode: QuoteMode::Unquote,
                        expr: Box::new(expect_one(instantiate_quasiquote(
                            expr,
                            captures,
                            depth - 1,
                        )?)?),
                    }))
                }
            }
            QuoteMode::Splice => {
                if depth == 1 {
                    Ok(TemplatePart::Many(resolve_capture_many(expr, captures)?))
                } else {
                    Ok(TemplatePart::One(Expr::Quote {
                        mode: QuoteMode::Splice,
                        expr: Box::new(expect_one(instantiate_quasiquote(
                            expr,
                            captures,
                            depth - 1,
                        )?)?),
                    }))
                }
            }
            QuoteMode::Quote | QuoteMode::Syntax => Ok(TemplatePart::One(Expr::Quote {
                mode: *mode,
                expr: Box::new(expect_one(instantiate_quasiquote(expr, captures, depth)?)?),
            })),
        },
        Expr::List(items) => Ok(TemplatePart::One(Expr::List(instantiate_sequence(
            items, captures, depth,
        )?))),
        Expr::Vector(items) => Ok(TemplatePart::One(Expr::Vector(instantiate_sequence(
            items, captures, depth,
        )?))),
        Expr::Set(items) => Ok(TemplatePart::One(Expr::Set(instantiate_sequence(
            items, captures, depth,
        )?))),
        Expr::Block(items) => Ok(TemplatePart::One(Expr::Block(instantiate_sequence(
            items, captures, depth,
        )?))),
        Expr::Map(entries) => Ok(TemplatePart::One(Expr::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        expect_one(instantiate_quasiquote(key, captures, depth)?)?,
                        expect_one(instantiate_quasiquote(value, captures, depth)?)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        Expr::Call { operator, args } => Ok(TemplatePart::One(Expr::Call {
            operator: Box::new(expect_one(instantiate_quasiquote(
                operator, captures, depth,
            )?)?),
            args: instantiate_sequence(args, captures, depth)?,
        })),
        Expr::Infix {
            operator,
            left,
            right,
        } => Ok(TemplatePart::One(Expr::Infix {
            operator: operator.clone(),
            left: Box::new(expect_one(instantiate_quasiquote(left, captures, depth)?)?),
            right: Box::new(expect_one(instantiate_quasiquote(right, captures, depth)?)?),
        })),
        Expr::Prefix { operator, arg } => Ok(TemplatePart::One(Expr::Prefix {
            operator: operator.clone(),
            arg: Box::new(expect_one(instantiate_quasiquote(arg, captures, depth)?)?),
        })),
        Expr::Postfix { operator, arg } => Ok(TemplatePart::One(Expr::Postfix {
            operator: operator.clone(),
            arg: Box::new(expect_one(instantiate_quasiquote(arg, captures, depth)?)?),
        })),
        Expr::Annotated { expr, annotations } => Ok(TemplatePart::One(Expr::Annotated {
            expr: Box::new(expect_one(instantiate_quasiquote(expr, captures, depth)?)?),
            annotations: annotations
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        expect_one(instantiate_quasiquote(value, captures, depth)?)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        })),
        Expr::Extension { tag, payload } => Ok(TemplatePart::One(Expr::Extension {
            tag: tag.clone(),
            payload: Box::new(expect_one(instantiate_quasiquote(
                payload, captures, depth,
            )?)?),
        })),
        other => Ok(TemplatePart::One(other.clone())),
    }
}

fn instantiate_sequence(items: &[Expr], captures: &Bindings, depth: usize) -> Result<Vec<Expr>> {
    let mut out = Vec::new();
    for item in items {
        match instantiate_quasiquote(item, captures, depth)? {
            TemplatePart::One(expr) => out.push(expr),
            TemplatePart::Many(exprs) => out.extend(exprs),
        }
    }
    Ok(out)
}

fn expect_one(part: TemplatePart) -> Result<Expr> {
    match part {
        TemplatePart::One(expr) => Ok(expr),
        TemplatePart::Many(_) => Err(Error::Eval(
            "splice is only valid inside sequence positions".to_owned(),
        )),
    }
}

fn resolve_capture_expr(expr: &Expr, captures: &Bindings) -> Result<Expr> {
    let Expr::Symbol(name) = expr else {
        return Err(Error::Eval(
            "macro unquote expects a captured symbol reference".to_owned(),
        ));
    };
    captures
        .exprs()
        .iter()
        .find_map(|(capture, expr)| (capture == name).then_some(expr.clone()))
        .ok_or_else(|| Error::Eval(format!("macro capture {name} is not bound")))
}

fn resolve_capture_many(expr: &Expr, captures: &Bindings) -> Result<Vec<Expr>> {
    let Expr::Symbol(name) = expr else {
        return Err(Error::Eval(
            "macro splice expects a captured symbol reference".to_owned(),
        ));
    };
    let values = captures
        .exprs()
        .iter()
        .filter_map(|(capture, expr)| (capture == name).then_some(expr.clone()))
        .collect::<Vec<_>>();
    if values.is_empty() {
        Err(Error::Eval(format!("macro capture {name} is not bound")))
    } else {
        Ok(values)
    }
}
