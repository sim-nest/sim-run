// conformance: preflight bounds and retains candidate test evidence deterministically.

use sim_kernel::{Symbol, TestReport};

/// Hard bounds for candidate-declared conformance execution and retained evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightLimits {
    /// Maximum tests a candidate may register.
    pub max_tests: usize,
    /// Maximum events retained from any one test report.
    pub max_events_per_test: usize,
    /// Maximum UTF-8 characters retained from test detail.
    pub max_detail_chars: usize,
}

impl Default for PreflightLimits {
    fn default() -> Self {
        Self {
            max_tests: 64,
            max_events_per_test: 256,
            max_detail_chars: 2048,
        }
    }
}

/// Limits actually reached while exercising a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AchievedLimits {
    /// Number of candidate tests run.
    pub tests_run: usize,
    /// Largest event count in one report.
    pub max_events_observed: usize,
    /// Largest retained detail length in characters.
    pub max_detail_chars_observed: usize,
}

/// Stable result retained for one candidate-declared test.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateTestResult {
    /// Test symbol.
    pub symbol: Symbol,
    /// Whether the test passed without being skipped.
    pub passed: bool,
    /// Bounded diagnostic detail.
    pub detail: Option<String>,
}

pub(crate) fn bounded_result(
    report: TestReport,
    limits: PreflightLimits,
) -> Result<CandidateTestResult, String> {
    if report.events.len() > limits.max_events_per_test {
        return Err(format!("test {} exceeded event limit", report.name));
    }
    let detail = report
        .detail
        .map(|value| value.chars().take(limits.max_detail_chars).collect());
    Ok(CandidateTestResult {
        symbol: report.name,
        passed: report.passed && !report.skipped,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_skipped_and_over_limit_reports_refuse() {
        let limits = PreflightLimits {
            max_tests: 1,
            max_events_per_test: 0,
            max_detail_chars: 3,
        };
        let mut report =
            TestReport::from_result(Symbol::new("self-test"), false, Some("failure".into()));
        assert!(!bounded_result(report.clone(), limits).unwrap().passed);
        report.skipped = true;
        report.passed = true;
        assert!(!bounded_result(report.clone(), limits).unwrap().passed);
        assert_eq!(
            bounded_result(report, limits).unwrap().detail.as_deref(),
            Some("fai")
        );
    }
}
