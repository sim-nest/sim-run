//! Deterministic projection of a [`Scene`](sim_lib_scene) to terminal text.
//!
//! Rendering is a two-step, total, side-effect-free transform. First the scene
//! is fit to the surface with [`reduce_for_caps`](sim_lib_view::codec::reduce_for_caps)
//! so a glance/compact surface receives fewer rows than a dense terminal; then
//! the reduced scene is walked into plain ASCII lines. The same `(scene, caps)`
//! always yields the same `String`, which is what makes the output usable as a
//! snapshot baseline.
//!
//! Each baseline scene kind has one stable spelling: a `scene/text` is its text,
//! a `scene/button` is `[label]`, a `scene/badge` is `<status: label>`, a
//! `scene/field` is `label: value`, and a `scene/table`/`scene/grid` is its rows
//! joined by ` | `. Container kinds (`scene/stack`, `scene/box`,
//! `scene/overlay`) emit their children in order. Any other kind degrades to a one-line `[<kind>]` marker so
//! an unrecognized node is visible rather than dropped.
//!
//! Container kinds include `scene/overlay`, so a shared command palette or
//! diagnostics overlay (see [`sim_lib_view::palette`]) renders its children
//! through the same path.

use sim_kernel::Expr;
use sim_lib_view::SurfaceCaps;

/// Renders `scene` to deterministic terminal text for the surface `caps`.
///
/// The scene is first reduced with
/// [`reduce_for_caps`](sim_lib_view::codec::reduce_for_caps) (display-density
/// projection), then walked into newline-joined ASCII lines with no trailing
/// newline. The transform is pure: equal inputs produce an equal `String`.
pub fn render_scene(scene: &Expr, caps: &SurfaceCaps) -> String {
    let reduced = sim_lib_view::codec::reduce_for_caps(scene, caps);
    render_node(&reduced).join("\n")
}

/// Walks one scene node into zero or more text lines.
fn render_node(node: &Expr) -> Vec<String> {
    let Some(kind) = sim_lib_scene::node_kind(node) else {
        // Not a kind-tagged map: render any atom as a single line.
        return vec![atom_text(node)];
    };
    match &*kind.name {
        "text" => vec![text_content(node)],
        "stack" | "box" | "overlay" => render_children(node),
        "grid" | "table" => render_rows(node),
        "field" => vec![render_field(node)],
        "button" => vec![format!("[{}]", field_text(node, "label"))],
        "badge" => vec![format!(
            "<{}: {}>",
            field_text(node, "status"),
            field_text(node, "label")
        )],
        // Known-but-unhandled or unknown kinds degrade to a visible marker.
        other => vec![format!("[{other}]")],
    }
}

/// Renders the ordered `children` of a container node, in declaration order.
fn render_children(node: &Expr) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(Expr::List(items) | Expr::Vector(items)) =
        sim_value::access::field(node, "children")
    {
        for child in items {
            lines.extend(render_node(child));
        }
    }
    lines
}

/// Renders a `scene/table` or `scene/grid`: an optional `columns` header line
/// followed by one ` | `-joined line per row.
fn render_rows(node: &Expr) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(Expr::List(cols) | Expr::Vector(cols)) = sim_value::access::field(node, "columns") {
        lines.push(join_cells(cols));
    }
    if let Some(Expr::List(rows) | Expr::Vector(rows)) = sim_value::access::field(node, "rows") {
        for row in rows {
            lines.push(render_row(row));
        }
    }
    lines
}

/// Renders one table row: a list/vector of cells, a map (values), or a lone
/// atom.
fn render_row(row: &Expr) -> String {
    match row {
        Expr::List(cells) | Expr::Vector(cells) => join_cells(cells),
        Expr::Map(entries) => entries
            .iter()
            .map(|(_, value)| atom_text(value))
            .collect::<Vec<_>>()
            .join(" | "),
        atom => atom_text(atom),
    }
}

/// Joins a row of cell expressions with the stable ` | ` separator.
fn join_cells(cells: &[Expr]) -> String {
    cells.iter().map(atom_text).collect::<Vec<_>>().join(" | ")
}

/// Renders a `scene/field` as `label: value`, dropping the prefix when the node
/// carries no `label`.
fn render_field(node: &Expr) -> String {
    let value = field_text(node, "value");
    match sim_value::access::field_str(node, "label") {
        Some(label) => format!("{label}: {value}"),
        None => value,
    }
}

/// Reads a node's text body from `text` then `content`, falling back to a
/// rendered atom of whichever field is present.
fn text_content(node: &Expr) -> String {
    for key in ["text", "content"] {
        if let Some(value) = sim_value::access::field(node, key) {
            return atom_text(value);
        }
    }
    String::new()
}

/// Reads a named field as display text, or the empty string when absent.
fn field_text(node: &Expr, name: &str) -> String {
    sim_value::access::field(node, name)
        .map(atom_text)
        .unwrap_or_default()
}

/// Renders a single value as compact, stable display text (no quoting).
fn atom_text(value: &Expr) -> String {
    match value {
        Expr::Nil => "nil".to_owned(),
        Expr::Bool(flag) => flag.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::String(text) => text.clone(),
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.as_qualified_str(),
        Expr::Bytes(bytes) => format!("#bytes({})", bytes.len()),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) => {
            items.iter().map(atom_text).collect::<Vec<_>>().join(" ")
        }
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| format!("{}={}", atom_text(key), atom_text(value)))
            .collect::<Vec<_>>()
            .join(" "),
        other => format!("<{}>", sim_value::kind::expr_kind(other)),
    }
}
