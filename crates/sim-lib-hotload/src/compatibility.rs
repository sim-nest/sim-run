// conformance: compatibility policy rejects removed and changed managed exports.

use std::{collections::BTreeSet, fmt};

use sim_kernel::{ExportKind, LibManifest, Symbol};

/// Export compatibility applied to an already-managed generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityPolicy {
    /// Candidate exports must equal the current export surface.
    Exact,
    /// Candidate exports may add names but may not remove or change existing names.
    Additive,
}

/// Deterministic compatibility evidence retained by an admission receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityReport {
    /// Policy applied; absent for an initial generation.
    pub policy: Option<CompatibilityPolicy>,
    /// Sorted candidate export surface.
    pub candidate_exports: Vec<(ExportKind, Symbol)>,
    /// Sorted exports added by an additive replacement.
    pub added_exports: Vec<(ExportKind, Symbol)>,
}

pub(crate) fn compare(
    expected_library: &Symbol,
    candidate: &LibManifest,
    current: Option<&LibManifest>,
    policy: CompatibilityPolicy,
) -> Result<CompatibilityReport, String> {
    if &candidate.id != expected_library {
        return Err(format!(
            "candidate manifest id {} does not match expected {}",
            candidate.id, expected_library
        ));
    }
    let candidate_exports = exports(candidate);
    let Some(current) = current else {
        return Ok(CompatibilityReport {
            policy: None,
            candidate_exports,
            added_exports: Vec::new(),
        });
    };
    if candidate.abi.major != current.abi.major {
        return Err("candidate ABI major differs from current generation".into());
    }
    if sorted_debug(&candidate.capabilities) != sorted_debug(&current.capabilities) {
        return Err("candidate capability set differs from current generation".into());
    }
    if sorted_debug(&candidate.requires) != sorted_debug(&current.requires) {
        return Err("candidate dependency requirements differ from current generation".into());
    }
    let old = exports(current).into_iter().collect::<BTreeSet<_>>();
    let new = candidate_exports.iter().cloned().collect::<BTreeSet<_>>();
    for (kind, symbol) in &old {
        match new
            .iter()
            .find(|(_, candidate_symbol)| candidate_symbol == symbol)
        {
            None => {
                return Err(format!(
                    "candidate removed export {:?}:{symbol}",
                    kind.symbol()
                ));
            }
            Some((candidate_kind, _)) if candidate_kind != kind => {
                return Err(format!(
                    "candidate changed kind of export {symbol} from {:?} to {:?}",
                    kind.symbol(),
                    candidate_kind.symbol()
                ));
            }
            Some(_) => {}
        }
    }
    let added_exports = new.difference(&old).cloned().collect::<Vec<_>>();
    if policy == CompatibilityPolicy::Exact && !added_exports.is_empty() {
        return Err(format!(
            "exact compatibility forbids added export {:?}:{}",
            added_exports[0].0.symbol(),
            added_exports[0].1
        ));
    }
    Ok(CompatibilityReport {
        policy: Some(policy),
        candidate_exports,
        added_exports,
    })
}

fn sorted_debug<T: fmt::Debug>(values: &[T]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn exports(manifest: &LibManifest) -> Vec<(ExportKind, Symbol)> {
    let mut exports = manifest
        .exports
        .iter()
        .map(|export| {
            let record = export.declared_record();
            (record.kind, record.symbol)
        })
        .collect::<Vec<_>>();
    exports.sort();
    exports
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::{AbiVersion, CapabilityName, Dependency, Export, LibTarget, Version};

    fn manifest(id: &str, exports: Vec<Export>) -> LibManifest {
        LibManifest {
            id: Symbol::new(id),
            version: Version("1.0.0".into()),
            abi: AbiVersion { major: 1, minor: 0 },
            target: LibTarget::Native,
            requires: vec![Dependency {
                id: Symbol::new("dep"),
                minimum_version: None,
            }],
            capabilities: vec![CapabilityName::new("read")],
            exports,
        }
    }

    fn value(name: &str) -> Export {
        Export::Value {
            symbol: Symbol::new(name),
        }
    }

    #[test]
    fn initial_generation_requires_the_expected_manifest_id() {
        assert!(
            compare(
                &Symbol::new("wanted"),
                &manifest("wrong", vec![]),
                None,
                CompatibilityPolicy::Exact
            )
            .is_err()
        );
        assert!(
            compare(
                &Symbol::new("wanted"),
                &manifest("wanted", vec![]),
                None,
                CompatibilityPolicy::Exact
            )
            .is_ok()
        );
    }

    #[test]
    fn replacement_refuses_removed_changed_and_exact_extra_exports() {
        let current = manifest("lib", vec![value("answer")]);
        assert!(
            compare(
                &current.id,
                &manifest("lib", vec![]),
                Some(&current),
                CompatibilityPolicy::Exact
            )
            .unwrap_err()
            .contains("removed")
        );
        let changed = Export::Function {
            symbol: Symbol::new("answer"),
            function_id: None,
        };
        assert!(
            compare(
                &current.id,
                &manifest("lib", vec![changed]),
                Some(&current),
                CompatibilityPolicy::Exact
            )
            .unwrap_err()
            .contains("changed kind")
        );
        assert!(
            compare(
                &current.id,
                &manifest("lib", vec![value("answer"), value("extra")]),
                Some(&current),
                CompatibilityPolicy::Exact
            )
            .unwrap_err()
            .contains("forbids added")
        );
    }

    #[test]
    fn replacement_refuses_abi_capability_and_dependency_drift() {
        let current = manifest("lib", vec![value("answer")]);

        let mut changed = current.clone();
        changed.abi.major += 1;
        assert!(
            compare(
                &current.id,
                &changed,
                Some(&current),
                CompatibilityPolicy::Additive
            )
            .unwrap_err()
            .contains("ABI major")
        );

        changed = current.clone();
        changed.capabilities.push(CapabilityName::new("write"));
        assert!(
            compare(
                &current.id,
                &changed,
                Some(&current),
                CompatibilityPolicy::Additive
            )
            .unwrap_err()
            .contains("capability")
        );

        changed = current.clone();
        changed.requires.clear();
        assert!(
            compare(
                &current.id,
                &changed,
                Some(&current),
                CompatibilityPolicy::Additive
            )
            .unwrap_err()
            .contains("dependency")
        );
    }

    #[test]
    fn additive_replacement_reports_sorted_additions() {
        let current = manifest("lib", vec![value("answer")]);
        let report = compare(
            &current.id,
            &manifest("lib", vec![value("z"), value("answer"), value("a")]),
            Some(&current),
            CompatibilityPolicy::Additive,
        )
        .unwrap();
        assert_eq!(
            report
                .added_exports
                .iter()
                .map(|(_, symbol)| symbol.to_string())
                .collect::<Vec<_>>(),
            vec!["a", "z"]
        );
    }
}
