//! Source-fact filtering and stable runtime projections.

use std::collections::BTreeSet;

use sim_index_core::{
    AnchorId, DeclarationRole, DiscoveredAnchor, IndexDoc, ProtocolRelation, ProtocolResolution,
    UnresolvedReason,
};

use crate::query::{Hit, Query, terms_match};

/// Stable declaration projection used by runtime search output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationHit {
    /// Declaration role.
    pub kind: String,
    /// Canonical module path.
    pub module_path: String,
    /// Stable source location.
    pub location: String,
}

/// Stable protocol-relation projection used by runtime search output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolRelationHit {
    /// Implementing type spelling.
    pub implementor: String,
    /// Protocol spelling at the implementation site.
    pub source_spelling: String,
    /// `resolved` or `unresolved`.
    pub resolution: String,
    /// Resolved protocol identity when available.
    pub protocol: Option<String>,
    /// Stable unresolved reason when resolution failed.
    pub unresolved_reason: Option<String>,
    /// Candidate protocol identities for an unresolved relation.
    pub candidates: Vec<String>,
}

pub(crate) fn filter_active(query: &Query) -> bool {
    query.declaration_kind.is_some()
        || query.implements.is_some()
        || query.resolved.is_some()
        || query.feature.is_some()
}

pub(crate) fn selected_anchors(
    doc: &IndexDoc,
    query: &Query,
    terms: &[String],
) -> BTreeSet<AnchorId> {
    let role = query.declaration_kind.as_deref().and_then(parse_role);
    doc.anchors
        .iter()
        .filter(|anchor| {
            let declarations = doc
                .declarations
                .iter()
                .filter(|fact| fact.anchor == anchor.id);
            let mut relations = doc
                .protocol_relations
                .iter()
                .filter(|relation| relation.anchor == anchor.id);
            let declaration_match = query.declaration_kind.is_none()
                || role.is_some_and(|role| declarations.clone().any(|fact| fact.role == role));
            let relation_match = (query.implements.is_none() && query.resolved.is_none())
                || relations
                    .clone()
                    .any(|relation| relation_matches(relation, query));
            let feature_match = query.feature.as_ref().is_none_or(|expected| {
                doc.features.iter().any(|feature| {
                    feature.anchors.contains(&anchor.id)
                        && (feature.id.as_str() == expected || feature.key.as_str() == expected)
                })
            });
            let text_match = terms.is_empty()
                || terms_match(
                    &format!("{} {} {}", anchor.id, anchor.subject, anchor.kind),
                    terms,
                )
                || declarations.clone().any(|fact| {
                    terms_match(
                        &format!(
                            "{} {} {}",
                            fact.role.as_str(),
                            fact.module_path,
                            fact.location.file
                        ),
                        terms,
                    )
                })
                || relations.any(|relation| relation_text_matches(relation, terms));
            declaration_match && relation_match && feature_match && text_match
        })
        .map(|anchor| anchor.id.clone())
        .collect()
}

pub(crate) fn anchor_hit(doc: &IndexDoc, anchor: &DiscoveredAnchor) -> Hit {
    let declarations = doc
        .declarations
        .iter()
        .filter(|fact| fact.anchor == anchor.id)
        .map(|fact| DeclarationHit {
            kind: fact.role.as_str().to_owned(),
            module_path: fact.module_path.clone(),
            location: format!(
                "{}#declaration-{}",
                fact.location.file, fact.location.declaration
            ),
        })
        .collect();
    let protocol_relations = doc
        .protocol_relations
        .iter()
        .filter(|relation| relation.anchor == anchor.id)
        .map(relation_hit)
        .collect();
    let title = doc
        .protocol_relations
        .iter()
        .filter(|relation| relation.anchor == anchor.id)
        .map(relation_summary)
        .chain(
            doc.declarations
                .iter()
                .filter(|fact| fact.anchor == anchor.id)
                .map(|fact| format!("{} declaration {}", fact.role.as_str(), fact.module_path)),
        )
        .collect::<Vec<_>>()
        .join("; ");
    Hit {
        kind: "anchor".to_owned(),
        id: anchor.id.to_string(),
        title,
        summary: anchor.kind.clone(),
        owner: anchor.subject.to_string(),
        surfaces: Vec::new(),
        declarations,
        protocol_relations,
    }
}

fn parse_role(value: &str) -> Option<DeclarationRole> {
    Some(match value {
        "const" => DeclarationRole::Const,
        "enum" => DeclarationRole::Enum,
        "function" => DeclarationRole::Function,
        "module" => DeclarationRole::Module,
        "re-export" => DeclarationRole::ReExport,
        "static" => DeclarationRole::Static,
        "struct" => DeclarationRole::Struct,
        "trait" => DeclarationRole::Trait,
        "type-alias" => DeclarationRole::TypeAlias,
        _ => return None,
    })
}

fn relation_matches(relation: &ProtocolRelation, query: &Query) -> bool {
    query.implements.as_ref().is_none_or(|expected| {
        relation.source_spelling == *expected
            || match &relation.resolution {
                ProtocolResolution::Resolved { protocol } => protocol == expected,
                ProtocolResolution::Unresolved { candidates, .. } => candidates.contains(expected),
            }
    }) && query.resolved.is_none_or(|expected| {
        expected == matches!(relation.resolution, ProtocolResolution::Resolved { .. })
    })
}

fn relation_text_matches(relation: &ProtocolRelation, terms: &[String]) -> bool {
    let mut text = format!("{} {}", relation.implementor, relation.source_spelling);
    match &relation.resolution {
        ProtocolResolution::Resolved { protocol } => text.push_str(&format!(" {protocol}")),
        ProtocolResolution::Unresolved { candidates, .. } => {
            text.push_str(&format!(" {}", candidates.join(" ")))
        }
    }
    terms_match(&text, terms)
}

fn relation_hit(relation: &ProtocolRelation) -> ProtocolRelationHit {
    match &relation.resolution {
        ProtocolResolution::Resolved { protocol } => ProtocolRelationHit {
            implementor: relation.implementor.clone(),
            source_spelling: relation.source_spelling.clone(),
            resolution: "resolved".to_owned(),
            protocol: Some(protocol.clone()),
            unresolved_reason: None,
            candidates: Vec::new(),
        },
        ProtocolResolution::Unresolved { reason, candidates } => ProtocolRelationHit {
            implementor: relation.implementor.clone(),
            source_spelling: relation.source_spelling.clone(),
            resolution: "unresolved".to_owned(),
            protocol: None,
            unresolved_reason: Some(reason_name(*reason).to_owned()),
            candidates: candidates.clone(),
        },
    }
}

fn relation_summary(relation: &ProtocolRelation) -> String {
    match &relation.resolution {
        ProtocolResolution::Resolved { protocol } => format!(
            "resolved protocol {} implements {protocol}",
            relation.implementor
        ),
        ProtocolResolution::Unresolved { reason, .. } => format!(
            "unresolved protocol edge {} to {} ({})",
            relation.implementor,
            relation.source_spelling,
            reason_name(*reason)
        ),
    }
}

fn reason_name(reason: UnresolvedReason) -> &'static str {
    match reason {
        UnresolvedReason::AmbiguousGlobImport => "ambiguous-glob-import",
        UnresolvedReason::AmbiguousName => "ambiguous-name",
        UnresolvedReason::ExternalMetadataAbsent => "external-metadata-absent",
    }
}

#[cfg(test)]
mod tests {
    use sim_index_core::{
        CanonicalFeatureKey, DeclarationFact, FeatureId, FeatureRecord, ProtocolRelation,
        SourceLocation, SubjectId, SubjectRecord, SyntaxBound,
    };

    use super::*;
    use crate::{Query, find};

    #[test]
    fn source_fact_filter_returns_control_plane_row_set() {
        let anchor = AnchorId::new("anchor/rust/demo/protocol");
        let mut doc = IndexDoc::public("test");
        doc.subjects.push(SubjectRecord {
            id: SubjectId::new("crate/demo"),
            kind: "crate".to_owned(),
            title: "demo".to_owned(),
        });
        doc.anchors.push(DiscoveredAnchor {
            id: anchor.clone(),
            subject: SubjectId::new("crate/demo"),
            kind: "rust-item".to_owned(),
        });
        doc.features.push(FeatureRecord {
            key: CanonicalFeatureKey::new("crate/demo/runtime"),
            id: FeatureId::new("feature/demo/runtime"),
            subject: SubjectId::new("crate/demo"),
            title: "Demo runtime".to_owned(),
            summary: "Runtime protocol".to_owned(),
            anchors: vec![anchor.clone()],
            surfaces: Vec::new(),
            specimens: Vec::new(),
            grammar_contracts: Vec::new(),
            doc_anchor: None,
        });
        doc.declarations.push(DeclarationFact {
            anchor: anchor.clone(),
            role: DeclarationRole::Trait,
            module_path: "demo::Protocol".to_owned(),
            generics: String::new(),
            members: Vec::new(),
            location: SourceLocation {
                file: "src/lib.rs".to_owned(),
                declaration: 0,
            },
            syntax_bound: SyntaxBound {
                max_bytes: 4096,
                truncated: false,
            },
        });
        doc.protocol_relations.push(ProtocolRelation {
            anchor,
            implementor: "Demo".to_owned(),
            source_spelling: "Protocol".to_owned(),
            body_fingerprint: "body".to_owned(),
            body_bound: SyntaxBound {
                max_bytes: 4096,
                truncated: false,
            },
            resolution: ProtocolResolution::Resolved {
                protocol: "demo::Protocol".to_owned(),
            },
        });

        let rows = find(
            &doc,
            &Query {
                implements: Some("demo::Protocol".to_owned()),
                resolved: Some(true),
                feature: Some("feature/demo/runtime".to_owned()),
                ..Query::default()
            },
        );
        assert_eq!(
            rows.iter()
                .map(|row| (row.kind.as_str(), row.id.as_str()))
                .collect::<Vec<_>>(),
            [
                ("anchor", "anchor/rust/demo/protocol"),
                ("feature", "feature/demo/runtime"),
                ("package", "crate/demo")
            ]
        );
        assert_eq!(rows[0].declarations[0].kind, "trait");
        assert_eq!(rows[0].protocol_relations[0].resolution, "resolved");
    }
}
