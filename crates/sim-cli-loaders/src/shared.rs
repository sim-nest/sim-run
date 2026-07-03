pub(crate) fn parse_symbol_text(value: &str) -> sim_kernel::Symbol {
    match value.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            sim_kernel::Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => sim_kernel::Symbol::new(value.to_owned()),
    }
}

pub(crate) fn expr_kind(expr: &sim_kernel::Expr) -> &'static str {
    match expr {
        sim_kernel::Expr::Nil => "nil",
        sim_kernel::Expr::Bool(_) => "bool",
        sim_kernel::Expr::Number(_) => "number",
        sim_kernel::Expr::Symbol(_) => "symbol",
        sim_kernel::Expr::Local(_) => "local",
        sim_kernel::Expr::String(_) => "string",
        sim_kernel::Expr::Bytes(_) => "bytes",
        sim_kernel::Expr::List(_) => "list",
        sim_kernel::Expr::Vector(_) => "vector",
        sim_kernel::Expr::Map(_) => "map",
        sim_kernel::Expr::Set(_) => "set",
        sim_kernel::Expr::Call { .. } => "call",
        sim_kernel::Expr::Infix { .. } => "infix",
        sim_kernel::Expr::Prefix { .. } => "prefix",
        sim_kernel::Expr::Postfix { .. } => "postfix",
        sim_kernel::Expr::Block(_) => "block",
        sim_kernel::Expr::Quote { .. } => "quote",
        sim_kernel::Expr::Annotated { .. } => "annotated",
        sim_kernel::Expr::Extension { .. } => "extension",
    }
}
