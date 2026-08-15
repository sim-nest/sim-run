//! Pure query helpers over the SIM Index graph.

use sim_index_core::{DiscoveredSpecimen, FeatureRecord, IndexDoc, RouteRecord, RouteStep};

use crate::IndexError;
pub use crate::source_facts::{DeclarationHit, ProtocolRelationHit};

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
    /// Public declaration role filter.
    pub declaration_kind: Option<String>,
    /// Implemented protocol spelling or resolved identity filter.
    pub implements: Option<String>,
    /// Protocol resolution filter (`true` for resolved).
    pub resolved: Option<bool>,
    /// Feature id or canonical key owning the source fact.
    pub feature: Option<String>,
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
    /// Declaration facts attached to an anchor result.
    pub declarations: Vec<DeclarationHit>,
    /// Protocol relations attached to an anchor result.
    pub protocol_relations: Vec<ProtocolRelationHit>,
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
    let fact_filter_active = crate::source_facts::filter_active(query);
    let selected_anchors = crate::source_facts::selected_anchors(doc, query, &terms);

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
        if (!fact_filter_active && terms_match(&text, &terms))
            || feature
                .anchors
                .iter()
                .any(|anchor| selected_anchors.contains(anchor))
        {
            hits.push(Hit {
                kind: "feature".to_owned(),
                id: feature.id.to_string(),
                title: feature.title.clone(),
                summary: feature.summary.clone(),
                owner: feature.subject.to_string(),
                surfaces: feature.surfaces.iter().map(ToString::to_string).collect(),
                declarations: Vec::new(),
                protocol_relations: Vec::new(),
            });
        }
    }

    if query.audience.is_none() {
        for subject in &doc.subjects {
            if !subject_matches_filters(subject.id.as_str(), query) {
                continue;
            }
            let text = [subject.id.as_str(), &subject.kind, &subject.title].join(" ");
            if (!fact_filter_active && terms_match(&text, &terms))
                || doc.anchors.iter().any(|anchor| {
                    anchor.subject == subject.id && selected_anchors.contains(&anchor.id)
                })
            {
                hits.push(Hit {
                    kind: if fact_filter_active && subject.kind == "crate" {
                        "package".to_owned()
                    } else {
                        subject.kind.clone()
                    },
                    id: subject.id.to_string(),
                    title: subject.title.clone(),
                    summary: subject.kind.clone(),
                    owner: subject.id.to_string(),
                    surfaces: surfaces_for_subject(doc, subject.id.as_str()),
                    declarations: Vec::new(),
                    protocol_relations: Vec::new(),
                });
            }
        }

        for anchor in &doc.anchors {
            if !selected_anchors.contains(&anchor.id) {
                continue;
            }
            hits.push(crate::source_facts::anchor_hit(doc, anchor));
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
                    declarations: Vec::new(),
                    protocol_relations: Vec::new(),
                });
            }
        }
    }

    for specimen in &doc.specimens {
        if !specimen_matches_audience(doc, specimen.id.as_str(), query.audience.as_deref()) {
            continue;
        }
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
        let linked_to_selected = doc.features.iter().any(|feature| {
            feature
                .specimens
                .iter()
                .any(|id| id.as_str() == specimen.id.as_str())
                && feature
                    .anchors
                    .iter()
                    .any(|id| selected_anchors.contains(id))
        });
        if (!fact_filter_active
            && (terms_match(&text, &terms)
                || specimen_linked_feature_matches(doc, specimen.id.as_str(), query, &terms)))
            || linked_to_selected
        {
            hits.push(Hit {
                kind: "specimen".to_owned(),
                id: specimen.id.to_string(),
                title: specimen.id.to_string(),
                summary: specimen.path.clone(),
                owner: specimen.subject.to_string(),
                surfaces: Vec::new(),
                declarations: Vec::new(),
                protocol_relations: Vec::new(),
            });
        }
    }

    for route in &doc.routes {
        if !route_matches_audience(route, query.audience.as_deref()) {
            continue;
        }
        let mut text = format!("{} {}", route.id.as_str(), route.title);
        for audience in &route.audiences {
            text.push(' ');
            text.push_str(audience);
        }
        for step in &route.steps {
            text.push(' ');
            text.push_str(step.id());
            text.push(' ');
            text.push_str(step.why());
        }
        if !fact_filter_active && terms_match(&text, &terms) {
            hits.push(Hit {
                kind: "route".to_owned(),
                id: route.id.to_string(),
                title: route.title.clone(),
                summary: format!("{} steps", route.steps.len()),
                owner: "route".to_owned(),
                surfaces: Vec::new(),
                declarations: Vec::new(),
                protocol_relations: Vec::new(),
            });
        }
    }

    if fact_filter_active {
        hits.sort_by(|left, right| (&left.kind, &left.id).cmp(&(&right.kind, &right.id)));
    } else {
        hits.sort_by(|left, right| left.id.cmp(&right.id));
    }
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

pub(crate) fn terms_match(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let text = text.to_lowercase();
    terms.iter().all(|term| text.contains(term))
}

fn matches_feature_filters(doc: &IndexDoc, feature: &FeatureRecord, query: &Query) -> bool {
    if !feature_matches_audience(doc, feature.id.as_str(), query.audience.as_deref()) {
        return false;
    }
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

fn feature_matches_audience(doc: &IndexDoc, feature_id: &str, audience: Option<&str>) -> bool {
    let Some(audience) = audience else {
        return true;
    };
    doc.routes.iter().any(|route| {
        route.audiences.iter().any(|item| item == audience)
            && route.steps.iter().any(|step| match step {
                RouteStep::Feature { id, .. } => id.as_str() == feature_id,
                RouteStep::Specimen { .. } => false,
            })
    })
}

fn specimen_matches_audience(doc: &IndexDoc, specimen_id: &str, audience: Option<&str>) -> bool {
    let Some(audience) = audience else {
        return true;
    };
    doc.routes.iter().any(|route| {
        route.audiences.iter().any(|item| item == audience)
            && route.steps.iter().any(|step| match step {
                RouteStep::Feature { .. } => false,
                RouteStep::Specimen { id, .. } => id.as_str() == specimen_id,
            })
    })
}

fn route_matches_audience(route: &RouteRecord, audience: Option<&str>) -> bool {
    audience.is_none_or(|expected| route.audiences.iter().any(|item| item == expected))
}

fn specimen_linked_feature_matches(
    doc: &IndexDoc,
    specimen_id: &str,
    query: &Query,
    terms: &[String],
) -> bool {
    doc.features.iter().any(|feature| {
        feature
            .specimens
            .iter()
            .any(|id| id.as_str() == specimen_id)
            && matches_feature_filters(doc, feature, query)
            && terms_match(
                &[
                    feature.id.as_str(),
                    feature.key.as_str(),
                    feature.subject.as_str(),
                    &feature.title,
                    &feature.summary,
                ]
                .join(" "),
                terms,
            )
    })
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

#[cfg(test)]
mod tests {
    use sim_index_core::{
        CanonicalFeatureKey, DiscoveredSpecimen, DiscoveredSurface, FeatureId, FeatureRecord,
        IndexDoc, RouteId, RouteRecord, RouteStep, SpecimenId, SubjectId, SubjectRecord, SurfaceId,
        Visibility,
    };

    use super::*;

    #[test]
    fn audience_filter_matches_control_plane_route_reachability() {
        let framework_feature = FeatureId::new("feature/demo/expression-tree");
        let code_feature = FeatureId::new("feature/demo/expression-tree-command");
        let specimen = SpecimenId::new("recipe/demo/open");
        let doc = IndexDoc {
            schema: "sim.index".to_owned(),
            generated_by: "test".to_owned(),
            visibility: Visibility::Public,
            subjects: vec![SubjectRecord {
                id: SubjectId::new("crate/expression-tree"),
                kind: "crate".to_owned(),
                title: "expression-tree".to_owned(),
            }],
            anchors: Vec::new(),
            surfaces: vec![DiscoveredSurface {
                id: SurfaceId::new("cli/expression-tree"),
                subject: SubjectId::new("crate/expression-tree"),
                kind: "cli".to_owned(),
            }],
            specimens: vec![DiscoveredSpecimen {
                id: specimen.clone(),
                subject: SubjectId::new("crate/expression-tree"),
                kind: "recipe".to_owned(),
                path: "recipes/open/recipe.toml".to_owned(),
                language: None,
                runnable: true,
                checked: true,
                checked_by: Some("cargo test".to_owned()),
                doc_anchor: None,
            }],
            drafts: Vec::new(),
            features: vec![
                feature(
                    framework_feature.clone(),
                    "Loadable expression-tree framework",
                    vec![specimen.clone()],
                ),
                feature(code_feature.clone(), "Expression-tree command", Vec::new()),
            ],
            routes: vec![
                RouteRecord {
                    id: RouteId::new("route/open-expression-tree"),
                    title: "Open an expression tree".to_owned(),
                    audiences: vec!["framework".to_owned()],
                    steps: vec![
                        RouteStep::Feature {
                            id: framework_feature,
                            why: "Use the framework.".to_owned(),
                        },
                        RouteStep::Specimen {
                            id: specimen,
                            why: "Run the checked example.".to_owned(),
                        },
                    ],
                    doc_anchor: None,
                },
                RouteRecord {
                    id: RouteId::new("route/start-expression-tree-command"),
                    title: "Start an expression tree command".to_owned(),
                    audiences: vec!["code".to_owned()],
                    steps: vec![RouteStep::Feature {
                        id: code_feature,
                        why: "Use the command.".to_owned(),
                    }],
                    doc_anchor: None,
                },
            ],
            edges: Vec::new(),
            declarations: Vec::new(),
            protocol_relations: Vec::new(),
        };
        let rows = find(
            &doc,
            &Query {
                terms: vec!["expression-tree".to_owned()],
                audience: Some("framework".to_owned()),
                ..Query::default()
            },
        );
        let ids = rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();

        assert_eq!(
            ids,
            [
                "feature/demo/expression-tree",
                "recipe/demo/open",
                "route/open-expression-tree",
            ]
        );
    }

    fn feature(id: FeatureId, title: &str, specimens: Vec<SpecimenId>) -> FeatureRecord {
        FeatureRecord {
            key: CanonicalFeatureKey::new(format!("crate/expression-tree/{}", id.as_str())),
            id,
            subject: SubjectId::new("crate/expression-tree"),
            title: title.to_owned(),
            summary: "Expression-tree capability.".to_owned(),
            anchors: Vec::new(),
            surfaces: Vec::new(),
            specimens,
            grammar_contracts: Vec::new(),
            doc_anchor: None,
        }
    }
}
