//! Best-use route lookup for runtime index readers.

use sim_index_core::{IndexDoc, RouteRecord, RouteStep};

/// A ranked route for a task query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteMatch {
    /// Route id.
    pub id: String,
    /// Route title.
    pub title: String,
    /// Reader audiences for the route.
    pub audiences: Vec<String>,
    /// Deterministic match score.
    pub score: usize,
    /// Ordered route steps with resolved display metadata.
    pub steps: Vec<RouteStepMatch>,
}

/// One resolved step inside a route match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteStepMatch {
    /// Target kind.
    pub kind: String,
    /// Target id.
    pub id: String,
    /// Resolved title or path for the target.
    pub title: String,
    /// Route-author rationale for this step.
    pub why: String,
    /// Specimen path when the step points at a specimen.
    pub path: Option<String>,
    /// Whether the specimen is runnable, when the step points at a specimen.
    pub runnable: Option<bool>,
    /// Whether the specimen is checked, when the step points at a specimen.
    pub checked: Option<bool>,
    /// Tool that checked the specimen, when known.
    pub checked_by: Option<String>,
}

/// Returns ranked route matches for `task`.
pub fn route(doc: &IndexDoc, task: &str) -> Vec<RouteMatch> {
    let terms = terms(task);
    let mut rows = doc
        .routes
        .iter()
        .filter_map(|route| {
            let score = route_score(doc, route, &terms);
            (score > 0).then(|| RouteMatch {
                id: route.id.to_string(),
                title: route.title.clone(),
                audiences: route.audiences.clone(),
                score,
                steps: route
                    .steps
                    .iter()
                    .map(|step| step_match(doc, step))
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| right.score.cmp(&left.score).then(left.id.cmp(&right.id)));
    rows
}

fn route_score(doc: &IndexDoc, route: &RouteRecord, terms: &[String]) -> usize {
    let haystack = route_text(doc, route);
    if terms.is_empty() {
        return 1;
    }
    let mut score = 0;
    for term in terms {
        if haystack.contains(term) {
            score += 10;
        }
        if route.id.as_str().to_ascii_lowercase().contains(term)
            || route.title.to_ascii_lowercase().contains(term)
        {
            score += 5;
        }
    }
    score
}

fn route_text(doc: &IndexDoc, route: &RouteRecord) -> String {
    let mut parts = vec![
        route.id.as_str().to_owned(),
        route.title.clone(),
        route.audiences.join(" "),
    ];
    for step in &route.steps {
        parts.push(step.id().to_owned());
        parts.push(step.why().to_owned());
        parts.push(step_title(doc, step));
    }
    parts.join(" ").to_ascii_lowercase()
}

fn step_match(doc: &IndexDoc, step: &RouteStep) -> RouteStepMatch {
    let specimen = match step {
        RouteStep::Specimen { id, .. } => doc
            .specimens
            .iter()
            .find(|specimen| specimen.id.as_str() == id.as_str()),
        RouteStep::Feature { .. } => None,
    };
    RouteStepMatch {
        kind: step.kind().to_owned(),
        id: step.id().to_owned(),
        title: step_title(doc, step),
        why: step.why().to_owned(),
        path: specimen.map(|specimen| specimen.path.clone()),
        runnable: specimen.map(|specimen| specimen.runnable),
        checked: specimen.map(|specimen| specimen.checked),
        checked_by: specimen.and_then(|specimen| specimen.checked_by.clone()),
    }
}

fn step_title(doc: &IndexDoc, step: &RouteStep) -> String {
    match step {
        RouteStep::Feature { id, .. } => doc
            .features
            .iter()
            .find(|feature| feature.id.as_str() == id.as_str())
            .map(|feature| feature.title.clone())
            .unwrap_or_else(|| id.to_string()),
        RouteStep::Specimen { id, .. } => doc
            .specimens
            .iter()
            .find(|specimen| specimen.id.as_str() == id.as_str())
            .map(|specimen| specimen.path.clone())
            .unwrap_or_else(|| id.to_string()),
    }
}

fn terms(task: &str) -> Vec<String> {
    task.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|term| term.len() > 1 && !STOP_WORDS.contains(&term.as_str()))
        .collect()
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "for", "in", "of", "on", "or", "the", "to", "with",
];

#[cfg(test)]
mod tests {
    use sim_index_core::{
        FeatureId, FeatureRecord, IndexDoc, RouteId, RouteRecord, RouteStep, SubjectId,
        SubjectRecord, Visibility, key::CanonicalFeatureKey,
    };

    use super::*;

    #[test]
    fn route_matches_task_terms() {
        let doc = IndexDoc {
            schema: "sim.index".to_owned(),
            generated_by: "test".to_owned(),
            visibility: Visibility::Public,
            subjects: vec![SubjectRecord {
                id: SubjectId::new("crate/demo"),
                kind: "crate".to_owned(),
                title: "demo".to_owned(),
            }],
            anchors: Vec::new(),
            surfaces: Vec::new(),
            specimens: Vec::new(),
            drafts: Vec::new(),
            features: vec![FeatureRecord {
                id: FeatureId::new("feature/demo/parser"),
                key: CanonicalFeatureKey::new("crate/demo/parser"),
                subject: SubjectId::new("crate/demo"),
                title: "Parser path".to_owned(),
                summary: "Parse operator languages.".to_owned(),
                anchors: Vec::new(),
                surfaces: Vec::new(),
                specimens: Vec::new(),
                grammar_contracts: Vec::new(),
                doc_anchor: None,
            }],
            routes: vec![RouteRecord {
                id: RouteId::new("route/demo/parser"),
                title: "Write a parser".to_owned(),
                audiences: vec!["code".to_owned()],
                steps: vec![RouteStep::Feature {
                    id: FeatureId::new("feature/demo/parser"),
                    why: "This feature explains parser assembly.".to_owned(),
                }],
                doc_anchor: None,
            }],
            edges: Vec::new(),
        };

        let rows = route(&doc, "write a parser");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "route/demo/parser");
        assert_eq!(
            rows[0].steps[0].why,
            "This feature explains parser assembly."
        );
    }
}
