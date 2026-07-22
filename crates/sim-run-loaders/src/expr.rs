#[cfg(any(feature = "codec-binary", feature = "codec-lisp"))]
pub(crate) fn parse_symbol_text(value: &str) -> sim_kernel::Symbol {
    match value.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            sim_kernel::Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => sim_kernel::Symbol::new(value.to_owned()),
    }
}

#[cfg(any(feature = "codec-binary", feature = "codec-lisp"))]
pub(crate) use sim_value::kind::expr_kind;
