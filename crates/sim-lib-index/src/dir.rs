//! Immutable Table/Dir view over an [`IndexDoc`](sim_index_core::IndexDoc).

use std::sync::Arc;

use sim_index_core::{DiscoveredSpecimen, IndexDoc};
use sim_kernel::{
    Cx, Dir, Error, Expr, Object, ObjectCompat, Result, Symbol, Table, Value,
    id::CORE_TABLE_CLASS_ID, object::ClassRef,
};

/// Immutable directory wrapper for the embedded SIM Index graph.
#[derive(Clone)]
pub struct IndexDir {
    doc: Arc<IndexDoc>,
}

impl IndexDir {
    /// Builds a directory over `doc`.
    pub fn new(doc: IndexDoc) -> Self {
        Self { doc: Arc::new(doc) }
    }

    /// Borrows the wrapped document.
    pub fn doc(&self) -> &IndexDoc {
        &self.doc
    }
}

impl Object for IndexDir {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "index-dir[subjects={}, features={}, specimens={}]",
            self.doc.subjects.len(),
            self.doc.features.len(),
            self.doc.specimens.len()
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IndexDir {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "Table");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().class_stub(CORE_TABLE_CLASS_ID, symbol)
    }

    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table_expr(cx)
    }

    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }

    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for IndexDir {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("index", "Dir")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        collection(cx, &self.doc, &key.to_string())?.map_or_else(|| cx.factory().nil(), Ok)
    }

    fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> Result<()> {
        Err(immutable_error())
    }

    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(collection_name(&key.to_string()).is_some())
    }

    fn del(&self, _cx: &mut Cx, _key: Symbol) -> Result<Value> {
        Err(immutable_error())
    }

    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(collection_names().map(Symbol::new).collect())
    }

    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        collection_names()
            .map(|name| Ok((Symbol::new(name), collection(cx, &self.doc, name)?.unwrap())))
            .collect()
    }

    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(collection_names().count())
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        Err(immutable_error())
    }
}

impl Dir for IndexDir {
    fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
        Err(immutable_error())
    }

    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        collection(cx, &self.doc, &name.to_string())
    }

    fn rmdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
        Err(immutable_error())
    }

    fn is_dir(&self, _cx: &mut Cx, name: Symbol) -> Result<bool> {
        Ok(collection_name(&name.to_string()).is_some())
    }
}

fn collection_names() -> impl Iterator<Item = &'static str> {
    [
        "subjects",
        "anchors",
        "surfaces",
        "specimens",
        "features",
        "routes",
        "edges",
    ]
    .into_iter()
}

fn collection_name(name: &str) -> Option<&'static str> {
    collection_names().find(|candidate| *candidate == name)
}

fn collection(cx: &mut Cx, doc: &IndexDoc, name: &str) -> Result<Option<Value>> {
    let Some(name) = collection_name(name) else {
        return Ok(None);
    };
    let entries = match name {
        "subjects" => doc
            .subjects
            .iter()
            .map(|subject| {
                row(
                    subject.id.as_str(),
                    vec![
                        ("id", string(cx, subject.id.as_str())?),
                        ("kind", string(cx, &subject.kind)?),
                        ("title", string(cx, &subject.title)?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        "anchors" => doc
            .anchors
            .iter()
            .map(|anchor| {
                row(
                    anchor.id.as_str(),
                    vec![
                        ("id", string(cx, anchor.id.as_str())?),
                        ("subject", string(cx, anchor.subject.as_str())?),
                        ("kind", string(cx, &anchor.kind)?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        "surfaces" => doc
            .surfaces
            .iter()
            .map(|surface| {
                row(
                    surface.id.as_str(),
                    vec![
                        ("id", string(cx, surface.id.as_str())?),
                        ("subject", string(cx, surface.subject.as_str())?),
                        ("kind", string(cx, &surface.kind)?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        "specimens" => doc
            .specimens
            .iter()
            .map(|specimen| specimen_row(cx, specimen))
            .collect::<Result<Vec<_>>>()?,
        "features" => doc
            .features
            .iter()
            .map(|feature| {
                row(
                    feature.id.as_str(),
                    vec![
                        ("id", string(cx, feature.id.as_str())?),
                        ("subject", string(cx, feature.subject.as_str())?),
                        ("title", string(cx, &feature.title)?),
                        ("summary", string(cx, &feature.summary)?),
                        ("surfaces", string_list(cx, feature.surfaces.iter())?),
                        ("specimens", string_list(cx, feature.specimens.iter())?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        "routes" => doc
            .routes
            .iter()
            .map(|route| {
                row(
                    route.id.as_str(),
                    vec![
                        ("id", string(cx, route.id.as_str())?),
                        ("title", string(cx, &route.title)?),
                        ("steps", string(cx, &route.steps.len().to_string())?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        "edges" => doc
            .edges
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                row(
                    &index.to_string(),
                    vec![
                        ("from", string(cx, &edge.from)?),
                        ("rel", string(cx, &edge.rel)?),
                        ("to", string(cx, &edge.to)?),
                    ],
                    cx,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        _ => Vec::new(),
    };
    Ok(Some(cx.factory().table(entries)?))
}

fn specimen_row(cx: &mut Cx, specimen: &DiscoveredSpecimen) -> Result<(Symbol, Value)> {
    row(
        specimen.id.as_str(),
        vec![
            ("id", string(cx, specimen.id.as_str())?),
            ("subject", string(cx, specimen.subject.as_str())?),
            ("kind", string(cx, &specimen.kind)?),
            ("path", string(cx, &specimen.path)?),
            (
                "language",
                optional_string(cx, specimen.language.as_deref())?,
            ),
            ("runnable", cx.factory().bool(specimen.runnable)?),
            ("checked", cx.factory().bool(specimen.checked)?),
            (
                "checked_by",
                optional_string(cx, specimen.checked_by.as_deref())?,
            ),
        ],
        cx,
    )
}

fn row(id: &str, entries: Vec<(&str, Value)>, cx: &mut Cx) -> Result<(Symbol, Value)> {
    let row_entries = entries
        .into_iter()
        .map(|(key, value)| (Symbol::new(key), value))
        .collect();
    Ok((Symbol::new(id), cx.factory().table(row_entries)?))
}

fn string(cx: &mut Cx, value: &str) -> Result<Value> {
    cx.factory().string(value.to_owned())
}

fn optional_string(cx: &mut Cx, value: Option<&str>) -> Result<Value> {
    match value {
        Some(value) => string(cx, value),
        None => cx.factory().nil(),
    }
}

fn string_list<'a, I, T>(cx: &mut Cx, values: I) -> Result<Value>
where
    I: IntoIterator<Item = &'a T>,
    T: ToString + 'a,
{
    let values = values
        .into_iter()
        .map(|value| string(cx, &value.to_string()))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values)
}

fn immutable_error() -> Error {
    Error::Eval("SIM Index Dir is immutable".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_index_core::Visibility;
    use sim_kernel::{DefaultFactory, NoopEvalPolicy};

    use super::*;

    // conformance: index Table/Dir backend exposes immutable generated index rows.
    #[test]
    fn table_dir_exposes_known_collections() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let dir = IndexDir::new(IndexDoc {
            schema: "sim.index".to_owned(),
            generated_by: "test".to_owned(),
            visibility: Visibility::Public,
            subjects: Vec::new(),
            anchors: Vec::new(),
            surfaces: Vec::new(),
            specimens: Vec::new(),
            drafts: Vec::new(),
            features: Vec::new(),
            routes: Vec::new(),
            edges: Vec::new(),
        });

        assert_eq!(dir.len(&mut cx).unwrap(), 7);
        assert!(dir.has(&mut cx, Symbol::new("features")).unwrap());
        assert!(dir.is_dir(&mut cx, Symbol::new("specimens")).unwrap());
        assert!(!dir.has(&mut cx, Symbol::new("missing")).unwrap());
        assert_eq!(
            dir.display(&mut cx).unwrap(),
            "index-dir[subjects=0, features=0, specimens=0]"
        );
        let nil = cx.factory().nil().unwrap();
        assert!(dir.set(&mut cx, Symbol::new("features"), nil).is_err());
    }
}
