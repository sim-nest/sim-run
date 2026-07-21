//! Stable text and JSON rendering for index queries.

use serde_json::{Value as JsonValue, json};
use sim_index_core::{DiscoveredSpecimen, IndexDoc, RouteStep};

use crate::{
    Collection, Hit, IndexCommand, IndexError, OutputMode, Query, Trace, examples, find, trace,
};

/// Renders `command` over `doc`.
pub fn render_command(command: &IndexCommand, doc: &IndexDoc) -> Result<String, IndexError> {
    match command {
        IndexCommand::Help => Ok(HELP.to_owned()),
        IndexCommand::List { collection, output } => render_list(doc, *collection, *output),
        IndexCommand::Show { id, output } => render_show(doc, id, *output),
        IndexCommand::Find { query, output } => render_find(doc, query, *output),
        IndexCommand::Trace { id, output } => render_trace(doc, id, *output),
        IndexCommand::Examples { feature, output } => render_examples(doc, feature, *output),
    }
}

const HELP: &str = "\
Usage: sim index <verb> [OPTIONS]

Verbs:
  list [subjects|anchors|surfaces|specimens|features|routes|edges|all] [--json]
  show <id> [--json]
  find <term...> [--audience X] [--surface-kind X] [--language X] [--json]
  trace <id> [--json]
  examples <feature-id> [--json]
";

fn render_list(
    doc: &IndexDoc,
    collection: Collection,
    output: OutputMode,
) -> Result<String, IndexError> {
    if output == OutputMode::Json {
        return pretty_json(&json!({
            "collection": collection.as_str(),
            "counts": counts(doc),
            "rows": collection_rows(doc, collection)
        }));
    }
    let mut out = String::new();
    match collection {
        Collection::All => {
            out.push_str("collection\tcount\n");
            for (name, count) in count_rows(doc) {
                out.push_str(&format!("{name}\t{count}\n"));
            }
        }
        Collection::Subjects => push_subjects(doc, &mut out),
        Collection::Anchors => push_anchors(doc, &mut out),
        Collection::Surfaces => push_surfaces(doc, &mut out),
        Collection::Specimens => push_specimens(&doc.specimens, &mut out),
        Collection::Features => push_features(doc, &mut out),
        Collection::Routes => push_routes(doc, &mut out),
        Collection::Edges => push_edges(doc, &mut out),
    }
    Ok(out)
}

fn render_show(doc: &IndexDoc, id: &str, output: OutputMode) -> Result<String, IndexError> {
    let row =
        show_row(doc, id).ok_or_else(|| IndexError::new(format!("index id not found: {id}")))?;
    if output == OutputMode::Json {
        return pretty_json(&row);
    }
    let mut out = String::new();
    if let JsonValue::Object(map) = row {
        for (key, value) in map {
            out.push_str(&format!("{key}\t{}\n", scalar_text(&value)));
        }
    }
    Ok(out)
}

fn render_find(doc: &IndexDoc, query: &Query, output: OutputMode) -> Result<String, IndexError> {
    let matches = find(doc, query);
    if output == OutputMode::Json {
        return pretty_json(&json!({
            "query": {
                "terms": query.terms,
                "audience": query.audience,
                "surface_kind": query.surface_kind,
                "language": query.language,
                "grammar": query.grammar,
                "repo": query.repo,
                "package": query.package,
                "anchor": query.anchor
            },
            "matches": hits_json(&matches)
        }));
    }
    let mut out = String::from("kind\tid\ttitle\towner\tsurfaces\n");
    for hit in matches {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            hit.kind,
            hit.id,
            hit.title,
            hit.owner,
            hit.surfaces.join(",")
        ));
    }
    Ok(out)
}

fn render_trace(doc: &IndexDoc, id: &str, output: OutputMode) -> Result<String, IndexError> {
    let trace = trace(doc, id)?;
    if output == OutputMode::Json {
        return pretty_json(&trace_json(&trace));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "id\t{}\nkind\t{}\ntitle\t{}\n",
        trace.id, trace.kind, trace.title
    ));
    push_joined(&mut out, "owners", &trace.owners);
    push_joined(&mut out, "surfaces", &trace.surfaces);
    push_joined(&mut out, "specimens", &trace.specimens);
    push_joined(&mut out, "anchors", &trace.anchors);
    for (rel, to) in &trace.outgoing {
        out.push_str(&format!("outgoing\t{rel}\t{to}\n"));
    }
    for (rel, from) in &trace.incoming {
        out.push_str(&format!("incoming\t{rel}\t{from}\n"));
    }
    Ok(out)
}

fn render_examples(
    doc: &IndexDoc,
    feature: &str,
    output: OutputMode,
) -> Result<String, IndexError> {
    let rows = examples(doc, feature)?;
    if output == OutputMode::Json {
        return pretty_json(&json!({
            "feature": feature,
            "examples": specimens_json(&rows)
        }));
    }
    let mut out = String::from("id\tpath\tchecked_by\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            row.id,
            row.path,
            row.checked_by.as_deref().unwrap_or("")
        ));
    }
    Ok(out)
}

fn push_subjects(doc: &IndexDoc, out: &mut String) {
    out.push_str("id\tkind\ttitle\n");
    for row in &doc.subjects {
        out.push_str(&format!("{}\t{}\t{}\n", row.id, row.kind, row.title));
    }
}

fn push_anchors(doc: &IndexDoc, out: &mut String) {
    out.push_str("id\tsubject\tkind\n");
    for row in &doc.anchors {
        out.push_str(&format!("{}\t{}\t{}\n", row.id, row.subject, row.kind));
    }
}

fn push_surfaces(doc: &IndexDoc, out: &mut String) {
    out.push_str("id\tsubject\tkind\n");
    for row in &doc.surfaces {
        out.push_str(&format!("{}\t{}\t{}\n", row.id, row.subject, row.kind));
    }
}

fn push_specimens(rows: &[DiscoveredSpecimen], out: &mut String) {
    out.push_str("id\tsubject\tpath\tchecked_by\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.id,
            row.subject,
            row.path,
            row.checked_by.as_deref().unwrap_or("")
        ));
    }
}

fn push_features(doc: &IndexDoc, out: &mut String) {
    out.push_str("id\tsubject\ttitle\tsurfaces\n");
    for row in &doc.features {
        let surfaces = row
            .surfaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.id, row.subject, row.title, surfaces
        ));
    }
}

fn push_routes(doc: &IndexDoc, out: &mut String) {
    out.push_str("id\ttitle\tsteps\n");
    for row in &doc.routes {
        out.push_str(&format!("{}\t{}\t{}\n", row.id, row.title, row.steps.len()));
    }
}

fn push_edges(doc: &IndexDoc, out: &mut String) {
    out.push_str("from\trel\tto\n");
    for row in &doc.edges {
        out.push_str(&format!("{}\t{}\t{}\n", row.from, row.rel, row.to));
    }
}

fn push_joined(out: &mut String, key: &str, values: &[String]) {
    if !values.is_empty() {
        out.push_str(&format!("{key}\t{}\n", values.join(",")));
    }
}

fn pretty_json(value: &JsonValue) -> Result<String, IndexError> {
    serde_json::to_string_pretty(value)
        .map(|mut text| {
            text.push('\n');
            text
        })
        .map_err(|err| IndexError::new(format!("render JSON: {err}")))
}

fn counts(doc: &IndexDoc) -> JsonValue {
    json!({
        "subjects": doc.subjects.len(),
        "anchors": doc.anchors.len(),
        "surfaces": doc.surfaces.len(),
        "specimens": doc.specimens.len(),
        "features": doc.features.len(),
        "routes": doc.routes.len(),
        "edges": doc.edges.len()
    })
}

fn count_rows(doc: &IndexDoc) -> Vec<(&'static str, usize)> {
    vec![
        ("subjects", doc.subjects.len()),
        ("anchors", doc.anchors.len()),
        ("surfaces", doc.surfaces.len()),
        ("specimens", doc.specimens.len()),
        ("features", doc.features.len()),
        ("routes", doc.routes.len()),
        ("edges", doc.edges.len()),
    ]
}

fn collection_rows(doc: &IndexDoc, collection: Collection) -> JsonValue {
    match collection {
        Collection::All => counts(doc),
        Collection::Subjects => json!(
            doc.subjects
                .iter()
                .map(|row| {
                    json!({"id": row.id.to_string(), "kind": row.kind, "title": row.title})
                })
                .collect::<Vec<_>>()
        ),
        Collection::Anchors => json!(doc.anchors.iter().map(|row| {
            json!({"id": row.id.to_string(), "subject": row.subject.to_string(), "kind": row.kind})
        }).collect::<Vec<_>>()),
        Collection::Surfaces => json!(doc.surfaces.iter().map(|row| {
            json!({"id": row.id.to_string(), "subject": row.subject.to_string(), "kind": row.kind})
        }).collect::<Vec<_>>()),
        Collection::Specimens => specimens_json(&doc.specimens),
        Collection::Features => {
            json!(doc.features.iter().map(|row| {
            json!({
                "id": row.id.to_string(),
                "title": row.title,
                "summary": row.summary,
                "owner": row.subject.to_string(),
                "surfaces": row.surfaces.iter().map(ToString::to_string).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>())
        }
        Collection::Routes => json!(
            doc.routes
                .iter()
                .map(|row| {
                    json!({
                        "id": row.id.to_string(),
                        "title": row.title,
                        "steps": route_steps_json(&row.steps)
                    })
                })
                .collect::<Vec<_>>()
        ),
        Collection::Edges => json!(
            doc.edges
                .iter()
                .map(|row| { json!({"from": row.from, "rel": row.rel, "to": row.to}) })
                .collect::<Vec<_>>()
        ),
    }
}

fn show_row(doc: &IndexDoc, id: &str) -> Option<JsonValue> {
    doc.features.iter().find(|row| row.id.as_str() == id).map(|row| {
        json!({
            "kind": "feature",
            "id": row.id.to_string(),
            "title": row.title,
            "summary": row.summary,
            "owner": row.subject.to_string(),
            "surfaces": row.surfaces.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "specimens": row.specimens.iter().map(ToString::to_string).collect::<Vec<_>>()
        })
    }).or_else(|| {
        doc.subjects.iter().find(|row| row.id.as_str() == id).map(|row| {
            json!({"kind": row.kind, "id": row.id.to_string(), "title": row.title})
        })
    }).or_else(|| {
        doc.surfaces.iter().find(|row| row.id.as_str() == id).map(|row| {
            json!({"kind": "surface", "id": row.id.to_string(), "owner": row.subject.to_string(), "surface_kind": row.kind})
        })
    }).or_else(|| {
        doc.specimens.iter().find(|row| row.id.as_str() == id).map(|row| {
            json!({"kind": "specimen", "id": row.id.to_string(), "owner": row.subject.to_string(), "path": row.path, "checked_by": row.checked_by})
        })
    }).or_else(|| {
        doc.routes.iter().find(|row| row.id.as_str() == id).map(|row| {
            json!({"kind": "route", "id": row.id.to_string(), "title": row.title, "steps": route_steps_json(&row.steps)})
        })
    }).or_else(|| {
        doc.anchors.iter().find(|row| row.id.as_str() == id).map(|row| {
            json!({"kind": "anchor", "id": row.id.to_string(), "owner": row.subject.to_string(), "anchor_kind": row.kind})
        })
    })
}

fn hits_json(rows: &[Hit]) -> JsonValue {
    json!(
        rows.iter()
            .map(|row| {
                json!({
                    "kind": row.kind,
                    "id": row.id,
                    "title": row.title,
                    "summary": row.summary,
                    "owner": row.owner,
                    "surfaces": row.surfaces
                })
            })
            .collect::<Vec<_>>()
    )
}

fn trace_json(row: &Trace) -> JsonValue {
    json!({
        "id": row.id,
        "kind": row.kind,
        "title": row.title,
        "owners": row.owners,
        "outgoing": row.outgoing.iter().map(|(rel, to)| json!({"rel": rel, "to": to})).collect::<Vec<_>>(),
        "incoming": row.incoming.iter().map(|(rel, from)| json!({"rel": rel, "from": from})).collect::<Vec<_>>(),
        "surfaces": row.surfaces,
        "specimens": row.specimens,
        "anchors": row.anchors
    })
}

fn specimens_json(rows: &[DiscoveredSpecimen]) -> JsonValue {
    json!(
        rows.iter()
            .map(|row| {
                json!({
                    "id": row.id.to_string(),
                    "subject": row.subject.to_string(),
                    "kind": row.kind,
                    "path": row.path,
                    "language": row.language,
                    "runnable": row.runnable,
                    "checked": row.checked,
                    "checked_by": row.checked_by
                })
            })
            .collect::<Vec<_>>()
    )
}

fn route_steps_json(rows: &[RouteStep]) -> Vec<JsonValue> {
    rows.iter()
        .map(|row| match row {
            RouteStep::Feature(id) => json!({"kind": "feature", "id": id.to_string()}),
            RouteStep::Specimen(id) => json!({"kind": "specimen", "id": id.to_string()}),
        })
        .collect()
}

fn scalar_text(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Array(values) => values.iter().map(scalar_text).collect::<Vec<_>>().join(","),
        JsonValue::Null => String::new(),
        value => value.to_string(),
    }
}
