//! Error classification via regex rules.
//!
//! Categorizes errors into broad buckets for pattern detection
//! and telemetry without exposing raw error messages.

use std::sync::LazyLock;

/// Failure category for error classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCategory {
    TypeError,
    SyntaxError,
    TestFail,
    LintFail,
    BuildFail,
    PermissionDenied,
    Timeout,
    NotFound,
    RuntimeError,
    Unknown,
}

impl FailureCategory {
    /// All variants as a static slice.
    pub fn all() -> &'static [FailureCategory] {
        &[
            Self::TypeError,
            Self::SyntaxError,
            Self::TestFail,
            Self::LintFail,
            Self::BuildFail,
            Self::PermissionDenied,
            Self::Timeout,
            Self::NotFound,
            Self::RuntimeError,
            Self::Unknown,
        ]
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::TypeError => "type_error",
            Self::SyntaxError => "syntax_error",
            Self::TestFail => "test_fail",
            Self::LintFail => "lint_fail",
            Self::BuildFail => "build_fail",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
            Self::NotFound => "not_found",
            Self::RuntimeError => "runtime_error",
            Self::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for FailureCategory {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "type_error" => Ok(Self::TypeError),
            "syntax_error" => Ok(Self::SyntaxError),
            "test_fail" => Ok(Self::TestFail),
            "lint_fail" => Ok(Self::LintFail),
            "build_fail" => Ok(Self::BuildFail),
            "permission_denied" => Ok(Self::PermissionDenied),
            "timeout" => Ok(Self::Timeout),
            "not_found" => Ok(Self::NotFound),
            "runtime_error" => Ok(Self::RuntimeError),
            "unknown" => Ok(Self::Unknown),
            _ => Err(()),
        }
    }
}

struct ClassificationRule {
    category: FailureCategory,
    pattern: regex::Regex,
}

static RULES: LazyLock<Vec<ClassificationRule>> = LazyLock::new(|| {
    let raw: &[(&str, FailureCategory)] = &[
        // Compiler diagnostic codes (most specific, check first)
        (r"(?i)error\[E\d{4}\]", FailureCategory::LintFail),
        // Type errors
        (
            r"(?i)cannot find (value|function|type|module|struct|field|method)",
            FailureCategory::TypeError,
        ),
        (r"(?i)mismatched types?", FailureCategory::TypeError),
        (r"(?i)type mismatch", FailureCategory::TypeError),
        (r"(?i)\bundefined\b", FailureCategory::TypeError),
        (
            r"(?i)is not (a |an )?(function|defined|iterable)",
            FailureCategory::TypeError,
        ),
        (r"(?i)expected .+, found", FailureCategory::TypeError),
        (r"(?i)no method named", FailureCategory::TypeError),
        (
            r"(?i)no (associated |)item named",
            FailureCategory::TypeError,
        ),
        (r"(?i)borrow of moved value", FailureCategory::TypeError),
        (r"(?i)use of moved value", FailureCategory::TypeError),
        (r"(?i)cannot borrow", FailureCategory::TypeError),
        // Syntax errors
        (r"(?i)syntax error", FailureCategory::SyntaxError),
        (r"(?i)unexpected token", FailureCategory::SyntaxError),
        (
            r"(?i)unexpected end of (file|input)",
            FailureCategory::SyntaxError,
        ),
        (r"(?i)parse error", FailureCategory::SyntaxError),
        (r"(?i)missing ';' or '}'", FailureCategory::SyntaxError),
        (r"(?i)expected .+ but found", FailureCategory::SyntaxError),
        (r"(?i)unclosed delimiter", FailureCategory::SyntaxError),
        // Test failures
        (r"(?i)test .* failed", FailureCategory::TestFail),
        (r"(?i)assertion.*failed", FailureCategory::TestFail),
        (r"(?i)panic!.*at", FailureCategory::TestFail),
        (r"(?i)test result: FAILED", FailureCategory::TestFail),
        (r"(?i)1 failed", FailureCategory::TestFail),
        (r"(?i)\bFAIL\b", FailureCategory::TestFail),
        // Lint failures
        (r"(?i)clippy::", FailureCategory::LintFail),
        (r"(?i)warning: .*denied", FailureCategory::LintFail),
        (
            r"(?i)unused (import|variable|function)",
            FailureCategory::LintFail,
        ),
        // Build failures
        (r"(?i)compilation failed", FailureCategory::BuildFail),
        (r"(?i)build failed", FailureCategory::BuildFail),
        (r"(?i)cargo build.*failed", FailureCategory::BuildFail),
        (r"(?i)could not compile", FailureCategory::BuildFail),
        (r"(?i)linking.*failed", FailureCategory::BuildFail),
        (r"(?i)fatal error:", FailureCategory::BuildFail),
        // Permission denied
        (r"(?i)permission denied", FailureCategory::PermissionDenied),
        (r"(?i)access denied", FailureCategory::PermissionDenied),
        (r"(?i)EACCES", FailureCategory::PermissionDenied),
        (r"(?i)EPERM", FailureCategory::PermissionDenied),
        (
            r"(?i)operation not permitted",
            FailureCategory::PermissionDenied,
        ),
        // Timeout
        (r"(?i)timed? ?out", FailureCategory::Timeout),
        (r"(?i)deadline exceeded", FailureCategory::Timeout),
        (r"(?i)ETIMEDOUT", FailureCategory::Timeout),
        // Not found
        (r"(?i)not found", FailureCategory::NotFound),
        (r"(?i)ENOENT", FailureCategory::NotFound),
        (r"(?i)no such file", FailureCategory::NotFound),
        (r"(?i)404", FailureCategory::NotFound),
        // Runtime errors (catch-all for execution errors)
        (r"(?i)runtime error", FailureCategory::RuntimeError),
        (r"(?i)segmentation fault", FailureCategory::RuntimeError),
        (r"(?i)stack overflow", FailureCategory::RuntimeError),
        (r"(?i)out of memory", FailureCategory::RuntimeError),
        (r"(?i)OOM", FailureCategory::RuntimeError),
        (r"(?i)panic!?", FailureCategory::RuntimeError),
    ];

    raw.iter()
        .map(|(pattern, category)| ClassificationRule {
            category: *category,
            pattern: regex::Regex::new(pattern).expect("invalid classification regex"),
        })
        .collect()
});

/// Classify an error message into a failure category.
///
/// Uses the first matching rule from a priority-ordered rule set.
/// Returns `Unknown` if no rule matches.
///
/// A fast byte-prefix path checks for compiler diagnostic codes
/// (`error[EXXXX]`) before invoking the regex engine.
pub fn classify_failure(error_message: &str) -> FailureCategory {
    // Fast path: compiler diagnostic codes like "error[E0412]"
    if let Some(rest) = error_message.strip_prefix("error[")
        && let Some(bracket_end) = rest.find(']')
    {
        let code = &rest[..bracket_end];
        if code.len() == 5
            && code.as_bytes()[0] == b'E'
            && code[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return FailureCategory::LintFail;
        }
    }
    for rule in RULES.iter() {
        if rule.pattern.is_match(error_message) {
            return rule.category;
        }
    }
    FailureCategory::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_error_undefined() {
        assert_eq!(
            classify_failure("Cannot find value `foo`"),
            FailureCategory::TypeError
        );
    }

    #[test]
    fn type_error_mismatched() {
        assert_eq!(
            classify_failure("mismatched types: expected `i32`, found `String`"),
            FailureCategory::TypeError
        );
    }

    #[test]
    fn syntax_error() {
        assert_eq!(
            classify_failure("syntax error: unexpected token `}`"),
            FailureCategory::SyntaxError
        );
    }

    #[test]
    fn test_fail() {
        assert_eq!(
            classify_failure("test test_insert failed"),
            FailureCategory::TestFail
        );
    }

    #[test]
    fn test_assertion() {
        assert_eq!(
            classify_failure("assertion `left == right` failed"),
            FailureCategory::TestFail
        );
    }

    #[test]
    fn lint_fail() {
        assert_eq!(
            classify_failure("error[E0412]: cannot find type"),
            FailureCategory::LintFail
        );
    }

    #[test]
    fn build_fail() {
        assert_eq!(
            classify_failure("could not compile `my-crate`"),
            FailureCategory::BuildFail
        );
    }

    #[test]
    fn permission_denied() {
        assert_eq!(
            classify_failure("Permission denied (os error 13)"),
            FailureCategory::PermissionDenied
        );
    }

    #[test]
    fn timeout() {
        assert_eq!(
            classify_failure("request timed out after 30s"),
            FailureCategory::Timeout
        );
    }

    #[test]
    fn not_found() {
        assert_eq!(
            classify_failure("No such file or directory"),
            FailureCategory::NotFound
        );
    }

    #[test]
    fn runtime_panic() {
        assert_eq!(
            classify_failure("thread 'main' panicked at 'overflow'"),
            FailureCategory::RuntimeError
        );
    }

    #[test]
    fn unknown_for_gibberish() {
        assert_eq!(
            classify_failure("everything is fine"),
            FailureCategory::Unknown
        );
    }

    #[test]
    fn category_labels() {
        assert_eq!(FailureCategory::TypeError.label(), "type_error");
        assert_eq!(FailureCategory::Unknown.label(), "unknown");
    }

    #[test]
    fn all_categories_count() {
        assert_eq!(FailureCategory::all().len(), 10);
    }

    #[test]
    fn from_str_roundtrip() {
        use std::str::FromStr;
        for cat in FailureCategory::all() {
            let label = cat.label();
            let back = FailureCategory::from_str(label).unwrap();
            assert_eq!(back, *cat, "roundtrip failed for {label}");
        }
        assert!(FailureCategory::from_str("not_a_real_category").is_err());
    }
}
