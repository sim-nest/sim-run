//! Pure query helpers over the SIM Index graph.

use sim_index_core::{DiscoveredSpecimen, FeatureRecord, IndexDoc};

use crate::IndexError;

/// Search terms and optional structured filters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Query {
    /// Free-text terms.
    pub terms: Vec<String>,
    /// Advisory audience filter carried for agents.
    pub audience: Option<String>,
    /// Surface kind filter.
    pub surface_kind: Option<String>,
    /// Specimen language filter.
    pub language: Option<String>,
    /// Grammar id filter.
    pub grammar: Option<String>,
    /// Owning repo subject filter.
    pub repo: Option<String>,
    /// Owning package or crate subject filter.
    pub package: Option<String>,
    /// Required anchor id filter.
    pub anchor: Option<String>,
}

impl Query {
    /// Returns true when no structured filter is present.
    pub fn is_unfiltered(&self) -> bool {
        self.audience.is_none()
            && self.surface_kind.is_none()
            && self.language.is_none()
            && self.grammar.is_none()
            && self.repo.is_none()
            && self.package.is_none()
            && self.anchor.is_none()
    }
}

/// One search result row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hit {
    /// Record kind.
    pub kind: String,
    /// Stable id.
    pub id: String,
    /// Human title.
    pub title: String,
    /// Short summary.
    pub summary: String,
    /// Owning subject.
    pub owner: String,
    /// Claimed or discovered surface ids.
    pub surfaces: Vec<String>,
}

/// A traced graph neighborhood for one id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trace {
    /// Traced id.
    pub id: String,
    /// Record kind.
    pub kind: String,
    /// Human title.
    pub title: String,
    /// Containing subjects or features.
    pub owners: Vec<String>,
    /// Outgoing relation rows.
    pub outgoing: Vec<(String, String)>,
    /// Incoming relation rows.
    pub incoming: Vec<(String, String)>,
    /// Attached surfaces.
    pub surfaces: Vec<String>,
    /// Attached examples.
    pub specimens: Vec<String>,
    /// Attached anchors.
    pub anchors: Vec<String>,
}

/// Searches the index for feature, subject, surface, specimen, and route rows.
pub fn find(doc: &IndexDoc, query: &Query) -> Vec<Hit> {
    let terms: Vec<String> = query.terms.iter().map(|term| term.to_lowercase()).collect();
    let mut hits = Vec::new();

    for feature in &doc.features {
        if !matches_feature_filters(doc, feature, query) {
            continue;
        }
        let text = [
            feature.id.as_str(),
            &feature.title,
            &feature.summary,
            feature.subject.as_str(),
        ]
        .join(" ");
        if terms_match(&text, &terms) {
            hits.push(Hit {
                kind: "feature".to_owned(),
                id: feature.id.to_string(),
                title: feature.title.clone(),
                summary: feature.summary.clone(),
                owner: feature.subject.to_string(),
                surfaces: feature.surfaces.iter().map(ToString::to_string).collect(),
            });
        }
    }

    for subject in &doc.subjects {
        if !subject_matches_filters(subject.id.as_str(), query) {
            continue;
        }
        let text = [subject.id.as_str(), &subject.kind, &subject.title].join(" ");
        if terms_match(&text, &terms) {
            hits.push(Hit {
                kind: subject.kind.clone(),
                id: subject.id.to_string(),
                title: subject.title.clone(),
                summary: subject.kind.clone(),
                owner: subject.id.to_string(),
                surfaces: surfaces_for_subject(doc, subject.id.as_str()),
            });
        }
    }

    for surface in &doc.surfaces {
        if query
            .surface_kind
            .as_deref()
            .is_some_and(|kind| kind != surface.kind)
        {
            continue;
        }
        if !subject_matches_filters(surface.subject.as_str(), query) {
            continue;
        }
        let text = [surface.id.as_str(), &surface.kind, surface.subject.as_str()].join(" ");
        if terms_match(&text, &terms) {
            hits.push(Hit {
                kind: "surface".to_owned(),
                id: surface.id.to_string(),
                title: surface.id.to_string(),
                summary: surface.kind.clone(),
                owner: surface.subject.to_string(),
                surfaces: vec![surface.id.to_string()],
            });
        }
    }

    for specimen in &doc.specimens {
        if query
            .language
            .as_deref()
            .is_some_and(|language| specimen.language.as_deref() != Some(language))
        {
            continue;
        }
        if !subject_matches_filters(specimen.subject.as_str(), query) {
            continue;
        }
        let text = [
            specimen.id.as_str(),
            &specimen.kind,
            &specimen.path,
            specimen.subject.as_str(),
        ]
        .join(" ");
        if terms_match(&text, &terms) {
            hits.push(Hit {
                kind: "specimen".to_owned(),
                id: specimen.id.to_string(),
                title: specimen.id.to_string(),
                summary: specimen.path.clone(),
                owner: specimen.subject.to_string(),
                surfaces: Vec::new(),
            });
        }
    }

    for route in &doc.routes {
        let mut text = format!("{} {}", route.id.as_str(), route.title);
        for step in &route.steps {
            text.push(' ');
            text.push_str(&format!("{step:?}"));
        }
        if terms_match(&text, &terms) {
            hits.push(Hit {
                kind: "route".to_owned(),
                id: route.id.to_string(),
                title: route.title.clone(),
                summary: format!("{} steps", route.steps.len()),
                owner: "route".to_owned(),
                surfaces: Vec::new(),
            });
        }
    }

    hits.sort_by(|left, right| left.id.cmp(&right.id));
    hits
}

/// Lists examples attached to `feature`.
pub fn examples(doc: &IndexDoc, feature: &str) -> Result<Vec<DiscoveredSpecimen>, IndexError> {
    let feature = doc
        .features
        .iter()
        .find(|candidate| candidate.id.as_str() == feature)
        .ok_or_else(|| IndexError::new(format!("feature not found: {feature}")))?;
    Ok(feature
        .specimens
        .iter()
        .filter_map(|id| {
            doc.specimens
                .iter()
                .find(|specimen| specimen.id.as_str() == id.as_str())
                .cloned()
        })
        .collect())
}

/// Traces one id through adjacent graph rows.
pub fn trace(doc: &IndexDoc, id: &str) -> Result<Trace, IndexError> {
    let target = describe_target(doc, id)
        .ok_or_else(|| IndexError::new(format!("index id not found: {id}")))?;
    let mut owners = doc
        .edges
        .iter()
        .filter(|edge| edge.rel == "contains" && edge.to == id)
        .map(|edge| edge.from.clone())
        .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    let mut outgoing = doc
        .edges
        .iter()
        .filter(|edge| edge.from == id)
        .map(|edge| (edge.rel.clone(), edge.to.clone()))
        .collect::<Vec<_>>();
    outgoing.sort();
    let mut incoming = doc
        .edges
        .iter()
        .filter(|edge| edge.to == id)
        .map(|edge| (edge.rel.clone(), edge.from.clone()))
        .collect::<Vec<_>>();
    incoming.sort();
    Ok(Trace {
        id: id.to_owned(),
        kind: target.kind,
        title: target.title,
        owners,
        outgoing,
        incoming,
        surfaces: target.surfaces,
        specimens: target.specimens,
        anchors: target.anchors,
    })
}

struct TargetDescription {
    kind: String,
    title: String,
    surfaces: Vec<String>,
    specimens: Vec<String>,
    anchors: Vec<String>,
}

impl TargetDescription {
    fn new(
        kind: impl Into<String>,
        title: impl Into<String>,
        surfaces: Vec<String>,
        specimens: Vec<String>,
        anchors: Vec<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            surfaces,
            specimens,
            anchors,
        }
    }
}

fn describe_target(doc: &IndexDoc, id: &str) -> Option<TargetDescription> {
    if let Some(feature) = doc
        .features
        .iter()
        .find(|feature| feature.id.as_str() == id)
    {
        return Some(TargetDescription::new(
            "feature",
            feature.title.clone(),
            feature.surfaces.iter().map(ToString::to_string).collect(),
            feature.specimens.iter().map(ToString::to_string).collect(),
            feature.anchors.iter().map(ToString::to_string).collect(),
        ));
    }
    if let Some(subject) = doc
        .subjects
        .iter()
        .find(|subject| subject.id.as_str() == id)
    {
        return Some(TargetDescription::new(
            subject.kind.clone(),
            subject.title.clone(),
            surfaces_for_subject(doc, id),
            specimens_for_subject(doc, id),
            anchors_for_subject(doc, id),
        ));
    }
    if let Some(surface) = doc
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == id)
    {
        return Some(TargetDescription::new(
            "surface",
            surface.id.to_string(),
            vec![surface.id.to_string()],
            Vec::new(),
            Vec::new(),
        ));
    }
    if let Some(specimen) = doc
        .specimens
        .iter()
        .find(|specimen| specimen.id.as_str() == id)
    {
        return Some(TargetDescription::new(
            "specimen",
            specimen.id.to_string(),
            Vec::new(),
            vec![specimen.id.to_string()],
            specimen
                .doc_anchor
                .iter()
                .map(ToString::to_string)
                .collect(),
        ));
    }
    if let Some(anchor) = doc.anchors.iter().find(|anchor| anchor.id.as_str() == id) {
        return Some(TargetDescription::new(
            "anchor",
            anchor.id.to_string(),
            Vec::new(),
            Vec::new(),
            vec![anchor.id.to_string()],
        ));
    }
    doc.routes
        .iter()
        .find(|route| route.id.as_str() == id)
        .map(|route| {
            TargetDescription::new(
                "route",
                route.title.clone(),
                Vec::new(),
                Vec::new(),
                route.doc_anchor.iter().map(ToString::to_string).collect(),
            )
        })
}

fn terms_match(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let text = text.to_lowercase();
    terms.iter().all(|term| text.contains(term))
}

fn matches_feature_filters(doc: &IndexDoc, feature: &FeatureRecord, query: &Query) -> bool {
    if !subject_matches_filters(feature.subject.as_str(), query) {
        return false;
    }
    if query
        .anchor
        .as_deref()
        .is_some_and(|anchor| !feature.anchors.iter().any(|id| id.as_str() == anchor))
    {
        return false;
    }
    if query.surface_kind.as_deref().is_some_and(|kind| {
        !feature.surfaces.iter().any(|id| {
            doc.surfaces
                .iter()
                .any(|surface| surface.id.as_str() == id.as_str() && surface.kind == kind)
        })
    }) {
        return false;
    }
    if query.language.as_deref().is_some_and(|language| {
        !feature.specimens.iter().any(|id| {
            doc.specimens.iter().any(|specimen| {
                specimen.id.as_str() == id.as_str()
                    && specimen.language.as_deref() == Some(language)
            })
        })
    }) {
        return false;
    }
    if query.grammar.as_deref().is_some_and(|grammar| {
        !feature
            .grammar_contracts
            .iter()
            .any(|contract| contract.id == grammar)
    }) {
        return false;
    }
    true
}

fn subject_matches_filters(subject: &str, query: &Query) -> bool {
    if query
        .package
        .as_deref()
        .is_some_and(|package| !subject.ends_with(package) && subject != package)
    {
        return false;
    }
    if query
        .repo
        .as_deref()
        .is_some_and(|repo| subject != format!("repo/{repo}") && subject != repo)
    {
        return false;
    }
    true
}

fn surfaces_for_subject(doc: &IndexDoc, subject: &str) -> Vec<String> {
    doc.surfaces
        .iter()
        .filter(|surface| surface.subject.as_str() == subject)
        .map(|surface| surface.id.to_string())
        .collect()
}

fn specimens_for_subject(doc: &IndexDoc, subject: &str) -> Vec<String> {
    doc.specimens
        .iter()
        .filter(|specimen| specimen.subject.as_str() == subject)
        .map(|specimen| specimen.id.to_string())
        .collect()
}

fn anchors_for_subject(doc: &IndexDoc, subject: &str) -> Vec<String> {
    doc.anchors
        .iter()
        .filter(|anchor| anchor.subject.as_str() == subject)
        .map(|anchor| anchor.id.to_string())
        .collect()
}
