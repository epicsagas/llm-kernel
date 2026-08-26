//! L1 — deterministic content scan.
//!
//! Single pass over static compiled rules: credentials, Korean PII, and
//! machine-local filesystem paths. Returns byte spans, categories, severity,
//! and an overall [`Sensitivity`] grade. Infallible — no I/O, no model.
//!
//! ```
//! use llm_kernel::dlp::{scan, Sensitivity};
//!
//! let report = scan("Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345");
//! assert!(report.sensitivity >= Sensitivity::Confidential);
//! assert!(!report.redact_spans.is_empty());
//! ```

use crate::provider::policy::Sensitivity;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Byte-offset range `[start, end)` into the scanned text.
///
/// **Byte** offsets, not char indices: the `regex` crate reports byte offsets
/// and Korean text is multibyte. Slice with `&text[span.start..span.end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    /// Byte offset of the first byte of the match.
    pub start: usize,
    /// Byte offset one past the last byte of the match.
    pub end: usize,
}

/// Severity of a single finding.
///
/// Variant order is the ordering (`Low < Medium < High < Critical`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational.
    Low,
    /// Likely personal or machine-revealing, not a credential.
    Medium,
    /// Sensitive personal data or a probable credential.
    High,
    /// Structurally unmistakable credential or strong PII.
    Critical,
}

/// Coarse finding category (drives severity floor and sensitivity).
///
/// Fine-grained detector identity is [`Finding::rule`]. New variants may be
/// added in any minor release.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    /// Credentials: API keys, tokens, private keys, DB connection strings.
    Secret,
    /// Korean PII: RRN (주민등록번호), bank accounts, mobile numbers.
    KoreanPii,
    /// Machine-local filesystem paths.
    FileSystemPath,
}

/// One detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Coarse category.
    pub category: FindingCategory,
    /// Detector label (e.g. `"rrn_kr"`, `"github_token"`) — audit identity,
    /// never contains matched text.
    pub rule: String,
    /// Severity of this finding.
    pub severity: Severity,
    /// Byte span of the matched text.
    pub span: Span,
}

/// Result of [`scan`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ScanReport {
    /// All detections, ascending by span start.
    pub findings: Vec<Finding>,
    /// Sorted, deduplicated spans to redact.
    pub redact_spans: Vec<Span>,
    /// Severity floor of the findings.
    pub sensitivity: Sensitivity,
}

struct Rule {
    category: FindingCategory,
    label: &'static str,
    severity: Severity,
    pattern: Regex,
    /// True when the redactable span is capture group 1 (context-anchored
    /// patterns where the leading context must not be consumed — the `regex`
    /// crate has no lookbehind and `~` has no word boundary).
    group1_span: bool,
}

// (label, category, severity, pattern, group1_span)
//
// Deferred detector families (one-line adds when needed): email, ssn_us,
// card_pan, Korean 사업자번호 (business registration), source-code/infra,
// healthcare PHI, finance MNPI.
const TABLE: &[(&str, FindingCategory, Severity, &str, bool)] = &[
    (
        "bearer_header",
        FindingCategory::Secret,
        Severity::Critical,
        r"(?i)\bauthorization\s*:\s*bearer\s+[A-Za-z0-9._\-]{20,}",
        false,
    ),
    (
        "key_value_assignment",
        FindingCategory::Secret,
        Severity::High,
        // Group 1 (the value) is the redactable span. Optional quotes on
        // either side of the separator let the rule fire inside JSON bodies
        // (`"api_key": "…"`), and the value charset excludes quotes/braces
        // so a redacted span never removes JSON structure characters
        // (claudy DLP proxy contract: byte identity outside the secret).
        r#"(?i)(?:password|passwd|token|key|secret|api_key|apikey|access_token|private_key)\s*["']?\s*[=:]\s*["']?([^\s"'{}]{8,})"#,
        true,
    ),
    (
        "anthropic_key",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bsk-ant-[A-Za-z0-9_-]{16,}\b",
        false,
    ),
    (
        "private_key_header",
        FindingCategory::Secret,
        Severity::Critical,
        r"(?i)-----BEGIN\s+(?:RSA|EC|DSA|OPENSSH|PGP)?\s*PRIVATE KEY",
        false,
    ),
    (
        "aws_access_key_id",
        FindingCategory::Secret,
        Severity::Critical,
        r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
        false,
    ),
    (
        "aws_secret_key",
        FindingCategory::Secret,
        Severity::Critical,
        r#"(?i)aws_secret_access_key\s*[=:]\s*["']?[A-Za-z0-9/+=]{16,}"#,
        false,
    ),
    (
        "github_token",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
        false,
    ),
    (
        "openai_style_key",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bsk-(?:proj-)?[A-Za-z0-9_-]{16,}\b",
        false,
    ),
    (
        "stripe_secret_key",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bsk_live_[A-Za-z0-9]{16,}\b",
        false,
    ),
    (
        "figma_token",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bfigd_[A-Za-z0-9]{20,}\b",
        false,
    ),
    (
        "slack_token",
        FindingCategory::Secret,
        Severity::Critical,
        r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
        false,
    ),
    (
        "db_connection_string",
        FindingCategory::Secret,
        Severity::Critical,
        r"(?i)\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^\s:@]+:[^\s@]+@",
        false,
    ),
    (
        "bank_account_kr",
        FindingCategory::KoreanPii,
        Severity::High,
        r"(?i)(?:계좌|account)\s*(?:번호|no\.?|number)?\s*[:：]?\s*\d{2,6}-\d{2,6}-\d{2,8}",
        false,
    ),
    (
        "phone_kr",
        FindingCategory::KoreanPii,
        Severity::Medium,
        r"\b01[016789]-\d{3,4}-\d{4}\b",
        false,
    ),
    (
        "home_path_posix",
        FindingCategory::FileSystemPath,
        Severity::Medium,
        r"/(?:Users|home)/[A-Za-z0-9_.][A-Za-z0-9_./-]*",
        false,
    ),
    (
        "home_path_windows",
        FindingCategory::FileSystemPath,
        Severity::Medium,
        r"(?i)\b[a-z]:\\users\\[A-Za-z0-9_.][A-Za-z0-9_.\\-]*",
        false,
    ),
    (
        "tilde_path",
        FindingCategory::FileSystemPath,
        Severity::Medium,
        r#"(?:^|[\s"'`(=:])(~/[A-Za-z0-9_./-]+)"#,
        true,
    ),
];

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    TABLE
        .iter()
        .map(|&(label, category, severity, pattern, group1_span)| Rule {
            category,
            label,
            severity,
            pattern: Regex::new(pattern)
                .unwrap_or_else(|e| panic!("invalid rule /{pattern}/: {e}")),
            group1_span,
        })
        .collect()
});

// RRN (주민등록번호) shape — gated by a checksum so same-shaped order numbers
// and dates do not flag.
static RRN_SHAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{6}-[1-4]\d{6}\b").expect("RRN_SHAPE is valid"));

/// Korean RRN checksum: weights 2..9 then 2..5 over the first 12 digits;
/// check digit = `(11 - sum % 11) % 10`.
fn rrn_checksum_valid(digits: &[u8; 13]) -> bool {
    const WEIGHTS: [u32; 12] = [2, 3, 4, 5, 6, 7, 8, 9, 2, 3, 4, 5];
    let sum: u32 = digits
        .iter()
        .take(12)
        .zip(WEIGHTS)
        .map(|(d, w)| u32::from(*d) * w)
        .sum();
    ((11 - sum % 11) % 10) == u32::from(digits[12])
}

fn severity_to_sensitivity(severity: Severity) -> Sensitivity {
    match severity {
        Severity::Critical => Sensitivity::Restricted,
        Severity::High => Sensitivity::Confidential,
        Severity::Medium | Severity::Low => Sensitivity::Internal,
    }
}

/// Deterministically scan `context` for secrets, Korean PII, and filesystem
/// paths. Infallible: static compiled rules, no I/O.
pub fn scan(context: &str) -> ScanReport {
    let mut findings: Vec<Finding> = Vec::new();

    for rule in RULES.iter() {
        for caps in rule.pattern.captures_iter(context) {
            let m = if rule.group1_span {
                caps.get(1)
                    .expect("group1_span rule always captures group 1")
            } else {
                caps.get(0).expect("capture 0 always present")
            };
            findings.push(Finding {
                category: rule.category,
                rule: rule.label.to_string(),
                severity: rule.severity,
                span: Span {
                    start: m.start(),
                    end: m.end(),
                },
            });
        }
    }

    for m in RRN_SHAPE.find_iter(context) {
        let text = m.as_str();
        let mut digits = [0u8; 13];
        let mut i = 0;
        for b in text.bytes() {
            if b.is_ascii_digit() {
                digits[i] = b - b'0';
                i += 1;
            }
        }
        if i == 13 && rrn_checksum_valid(&digits) {
            findings.push(Finding {
                category: FindingCategory::KoreanPii,
                rule: "rrn_kr".to_string(),
                severity: Severity::Critical,
                span: Span {
                    start: m.start(),
                    end: m.end(),
                },
            });
        }
    }

    findings.sort_by_key(|f| f.span.start);

    let mut redact_spans: Vec<Span> = findings.iter().map(|f| f.span).collect();
    redact_spans.sort_unstable();
    redact_spans.dedup();

    let sensitivity = findings
        .iter()
        .map(|f| f.severity)
        .max()
        .map_or(Sensitivity::Public, severity_to_sensitivity);

    ScanReport {
        findings,
        redact_spans,
        sensitivity,
    }
}

/// Replace every span with `****`, multibyte-safe.
///
/// Overlapping spans are merged before splicing. Spans must come from
/// [`scan`] on the same `text` (regex byte offsets are guaranteed char
/// boundaries).
///
/// # Panics
///
/// Panics if a span is out of bounds or not on a UTF-8 char boundary of
/// `text`.
pub fn apply_redactions(text: &str, spans: &[Span]) -> String {
    let mut sorted = spans.to_vec();
    sorted.sort_unstable();

    let mut merged: Vec<Span> = Vec::with_capacity(sorted.len());
    for s in sorted {
        match merged.last_mut() {
            Some(last) if s.start <= last.end => last.end = last.end.max(s.end),
            _ => merged.push(s),
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for s in merged {
        if s.start > pos {
            out.push_str(&text[pos..s.start]);
        }
        out.push_str("****");
        pos = pos.max(s.end);
    }
    if pos < text.len() {
        out.push_str(&text[pos..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_hit(text: &str, label: &str) -> Vec<Finding> {
        scan(text)
            .findings
            .into_iter()
            .filter(|f| f.rule == label)
            .collect()
    }

    /// Build a checksum-valid RRN from 12 digits ("YYMMDD" + "SNNNNNN" minus
    /// the check digit), so tests never hardcode a real-format constant.
    fn make_valid_rrn(first12: &str) -> String {
        assert_eq!(first12.len(), 12);
        let digits: Vec<u8> = first12.bytes().map(|b| b - b'0').collect();
        let mut arr = [0u8; 13];
        arr[..12].copy_from_slice(&digits);
        let mut s = String::new();
        s.push_str(&first12[..6]);
        s.push('-');
        s.push_str(&first12[6..]);
        s.push_str(&rrn_check_digit(&digits).to_string());
        arr[12] = rrn_check_digit(&digits);
        assert!(rrn_checksum_valid(&arr));
        s
    }

    fn rrn_check_digit(first12: &[u8]) -> u8 {
        const WEIGHTS: [u32; 12] = [2, 3, 4, 5, 6, 7, 8, 9, 2, 3, 4, 5];
        let sum: u32 = first12
            .iter()
            .zip(WEIGHTS)
            .map(|(d, w)| u32::from(*d) * w)
            .sum();
        ((11 - sum % 11) % 10) as u8
    }

    #[test]
    fn bearer_header_detected() {
        let hits = rules_hit(
            "Authorization: Bearer abcdefghijklmnopqrstuvwxyz",
            "bearer_header",
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].severity, Severity::Critical);
    }

    #[test]
    fn bearer_short_token_ignored() {
        assert!(rules_hit("Authorization: Bearer abc", "bearer_header").is_empty());
    }

    #[test]
    fn key_value_assignment_requires_long_value() {
        let hits = rules_hit("password=hunter2secret", "key_value_assignment");
        assert_eq!(hits.len(), 1);
        assert!(rules_hit("key=mode", "key_value_assignment").is_empty());
        assert!(rules_hit("api_key: short", "key_value_assignment").is_empty());
    }

    #[test]
    fn private_key_header_detected() {
        let hits = rules_hit("-----BEGIN RSA PRIVATE KEY-----", "private_key_header");
        assert_eq!(hits.len(), 1);
        assert!(rules_hit("-----BEGIN CERTIFICATE-----", "private_key_header").is_empty());
    }

    #[test]
    fn aws_access_key_detected_with_length_bound() {
        assert_eq!(
            rules_hit("AKIAIOSFODNN7EXAMPLE", "aws_access_key_id").len(),
            1
        );
        assert!(rules_hit("AKIAIOSFODNN7EXAMPL", "aws_access_key_id").is_empty());
    }

    #[test]
    fn aws_secret_key_detected() {
        assert_eq!(
            rules_hit(
                "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                "aws_secret_key"
            )
            .len(),
            1
        );
    }

    #[test]
    fn github_token_detected() {
        assert_eq!(
            rules_hit("ghp_0123456789abcdefghijklmnopqrstuvwxyzAB", "github_token").len(),
            1
        );
        assert!(rules_hit("ghp_short", "github_token").is_empty());
    }

    #[test]
    fn openai_style_key_with_proj_prefix_detected() {
        // Regression: a charset without `-` misses modern `sk-proj-…` keys.
        assert_eq!(
            rules_hit("sk-proj-abc123def456ghi789jkl", "openai_style_key").len(),
            1
        );
        assert_eq!(
            rules_hit("sk-abcdef0123456789abcdef", "openai_style_key").len(),
            1
        );
    }

    #[test]
    fn anthropic_key_detected() {
        assert_eq!(
            rules_hit("sk-ant-api03-0123456789abcdefGHIJKL", "anthropic_key").len(),
            1
        );
    }

    #[test]
    fn key_value_span_excludes_json_structure_chars() {
        // claudy DLP proxy contract: redacting a span inside a JSON body must
        // never consume quotes/braces — byte identity outside the secret.
        let json = r#"{"api_key": "abc123def456ghi789"}"#;
        let report = scan(json);
        let finding = report
            .findings
            .iter()
            .find(|f| f.rule == "key_value_assignment")
            .expect("key_value finding");
        let spanned = &json[finding.span.start..finding.span.end];
        assert!(!spanned.contains('"'), "span ate a quote: {spanned}");
        assert_eq!(spanned, "abc123def456ghi789");
        let redacted = apply_redactions(json, &report.redact_spans);
        let value: serde_json::Value =
            serde_json::from_str(&redacted).expect("redacted JSON still parses");
        assert_eq!(value["api_key"], "****");
    }

    #[test]
    fn stripe_slack_figma_tokens_detected() {
        // Built by concatenation so the literal never lands in the git blob
        // (GitHub push protection flags `sk_live_…`-shaped strings even as
        // test fixtures).
        let stripe_key = ["sk_", "live_", "0123456789abcdefGHIJ"].concat();
        assert_eq!(rules_hit(&stripe_key, "stripe_secret_key").len(), 1);
        assert_eq!(
            rules_hit("xoxb-1234567890abcdefWXYZ", "slack_token").len(),
            1
        );
        assert_eq!(
            rules_hit("figd_0123456789abcdefghij", "figma_token").len(),
            1
        );
    }

    #[test]
    fn db_connection_string_detected() {
        assert_eq!(
            rules_hit(
                "postgres://admin:hunter2@db.example/prod",
                "db_connection_string"
            )
            .len(),
            1
        );
        // No credentials in the URI → no finding.
        assert!(rules_hit("postgres://db.example/prod", "db_connection_string").is_empty());
    }

    #[test]
    fn rrn_valid_checksum_detected() {
        let rrn = make_valid_rrn("900101123456");
        let hits = rules_hit(&rrn, "rrn_kr");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].severity, Severity::Critical);
        assert_eq!(scan(&rrn).sensitivity, Sensitivity::Restricted);
    }

    #[test]
    fn rrn_invalid_checksum_ignored() {
        // Same shape, fails the checksum gate.
        assert!(rules_hit("900101-1234567", "rrn_kr").is_empty());
    }

    #[test]
    fn rrn_multibyte_span_is_exact() {
        let rrn = make_valid_rrn("900101123456");
        let text = format!("주민번호는 {rrn} 입니다");
        let report = scan(&text);
        let f = report
            .findings
            .iter()
            .find(|f| f.rule == "rrn_kr")
            .expect("rrn finding");
        assert_eq!(&text[f.span.start..f.span.end], rrn);
    }

    #[test]
    fn bank_account_kr_detected() {
        assert_eq!(
            rules_hit("계좌 번호: 123-456-789012", "bank_account_kr").len(),
            1
        );
        assert_eq!(
            rules_hit("account no. 301-0123-4567", "bank_account_kr").len(),
            1
        );
    }

    #[test]
    fn phone_kr_both_forms_detected() {
        assert_eq!(rules_hit("010-1234-5678", "phone_kr").len(), 1);
        assert_eq!(rules_hit("010-123-4567", "phone_kr").len(), 1);
        assert!(rules_hit("01012345678", "phone_kr").is_empty());
    }

    #[test]
    fn filesystem_paths_detected() {
        assert_eq!(
            rules_hit("/Users/hackme/notes.md", "home_path_posix").len(),
            1
        );
        assert_eq!(rules_hit("/home/user/.env", "home_path_posix").len(), 1);
        assert_eq!(
            rules_hit("C:\\Users\\kim\\doc.txt", "home_path_windows").len(),
            1
        );
    }

    #[test]
    fn tilde_path_span_excludes_leading_context() {
        let text = "see ~/secret.md now";
        let hits = rules_hit(text, "tilde_path");
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].span.start..hits[0].span.end], "~/secret.md");
        // Line-start anchor also matches.
        assert_eq!(rules_hit("~/notes.md", "tilde_path").len(), 1);
        // Bare `~` with nothing after it does not match.
        assert!(rules_hit("cd ~ then", "tilde_path").is_empty());
    }

    #[test]
    fn clean_text_is_public() {
        let report = scan("just a normal sentence about the weather");
        assert_eq!(report.sensitivity, Sensitivity::Public);
        assert!(report.findings.is_empty());
        assert!(report.redact_spans.is_empty());
    }

    #[test]
    fn empty_text_is_public() {
        let report = scan("");
        assert_eq!(report.sensitivity, Sensitivity::Public);
    }

    #[test]
    fn sensitivity_floor_follows_max_severity() {
        // phone_kr (Medium) → Internal
        assert_eq!(
            scan("call me at 010-1234-5678").sensitivity,
            Sensitivity::Internal
        );
        // github_token (Critical) → Restricted
        assert_eq!(
            scan("ghp_0123456789abcdefghijklmnopqrstuvwxyzAB").sensitivity,
            Sensitivity::Restricted
        );
    }

    #[test]
    fn redact_spans_sorted_and_deduped() {
        // bearer_header and key_value_assignment can both hit the same region.
        let text =
            "Authorization: Bearer abcdefghijklmnopqrstuvwxyz token=abcdefghijklmnopqrstuvwxyz";
        let report = scan(text);
        let mut spans = report.redact_spans.clone();
        spans.sort_unstable();
        spans.dedup();
        assert_eq!(report.redact_spans, spans);
        assert!(!report.redact_spans.is_empty());
    }

    #[test]
    fn apply_redactions_multibyte_safe() {
        let rrn = make_valid_rrn("900101123456");
        let text = format!("주민번호는 {rrn} 입니다");
        let report = scan(&text);
        let redacted = apply_redactions(&text, &report.redact_spans);
        assert_eq!(redacted, format!("주민번호는 **** 입니다"));
    }

    #[test]
    fn apply_redactions_merges_overlapping_spans() {
        let text = "abcdefghij";
        // Overlapping spans covering [1,4) and [2,6).
        let spans = vec![Span { start: 2, end: 6 }, Span { start: 1, end: 4 }];
        assert_eq!(apply_redactions(text, &spans), "a****ghij");
    }

    #[test]
    fn findings_sorted_ascending() {
        let text = "path ~/a.md and 010-1234-5678 and ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
        let report = scan(text);
        let starts: Vec<usize> = report.findings.iter().map(|f| f.span.start).collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted);
    }
}
