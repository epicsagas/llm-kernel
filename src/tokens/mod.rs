//! Token estimation for LLM context budgeting.
//!
//! Provides a zero-dependency Unicode-script-based heuristic for estimating
//! token counts, useful for budget management without pulling in tiktoken.
//!
//! ```
//! use llm_kernel::tokens::estimate_tokens;
//!
//! let count = estimate_tokens("Hello, world! こんにちは世界");
//! assert!(count > 0);
//! ```

/// Characters-per-token ratios by Unicode script.
///
/// Derived from empirical measurements against cl100k_base encoding.
/// CJK characters are ~1.5 chars/token, while Latin text is ~4 chars/token.
type ScriptRule = (fn(char) -> bool, f32);

#[allow(clippy::type_complexity)]
const CPT_TABLE: &[ScriptRule] = &[
    (|c| (0x1F600..=0x1F64F).contains(&(c as u32)), 1.0),   // Emoji emoticons
    (|c| (0x1F300..=0x1F5FF).contains(&(c as u32)), 1.0),   // Misc symbols
    (|c| (0x1F680..=0x1F6FF).contains(&(c as u32)), 1.0),   // Transport
    (|c| (0x2600..=0x26FF).contains(&(c as u32)),   1.0),   // Misc symbols
    (|c| ('\u{3040}'..='\u{309F}').contains(&c)               // Hiragana
        || ('\u{30A0}'..='\u{30FF}').contains(&c),            // Katakana
     1.5),
    (|c| ('\u{4E00}'..='\u{9FFF}').contains(&c), 1.5),       // CJK Unified
    (|c| ('\u{AC00}'..='\u{D7AF}').contains(&c), 1.5),       // Hangul Syllables
    (|c| ('\u{0600}'..='\u{06FF}').contains(&c), 2.0),       // Arabic
    (|c| ('\u{0900}'..='\u{097F}').contains(&c), 2.0),       // Devanagari
    (|c| ('\u{0E00}'..='\u{0E7F}').contains(&c), 2.0),       // Thai
];

/// Default chars-per-token for Latin/basic ASCII text.
const DEFAULT_CPT: f32 = 4.0;

/// Estimate the number of tokens in a string using Unicode-script heuristics.
///
/// This is a rough estimate (±20%) suitable for budget management.
/// For exact counts, enable the `tiktoken` feature.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    let mut total_weight: f32 = 0.0;

    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_ascii_control() {
            continue;
        }
        let cpt = CPT_TABLE
            .iter()
            .find(|(pred, _)| pred(ch))
            .map(|(_, cpt)| *cpt)
            .unwrap_or(DEFAULT_CPT);
        total_weight += 1.0 / cpt;
    }

    if total_weight == 0.0 {
        return 0;
    }

    total_weight.round() as usize
}

/// Estimate tokens for a single string, returning at least `min`.
pub fn estimate_tokens_min(text: &str, min: usize) -> usize {
    estimate_tokens(text).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn ascii_text() {
        let tokens = estimate_tokens("Hello, world! This is a test.");
        // ~30 chars / 4 cpt ≈ 7-8 tokens
        assert!(tokens > 3 && tokens < 15, "got {tokens}");
    }

    #[test]
    fn cjk_text() {
        let tokens = estimate_tokens("こんにちは世界");
        // 7 chars / 1.5 cpt ≈ 4-5 tokens
        assert!(tokens > 2 && tokens < 10, "got {tokens}");
    }

    #[test]
    fn mixed_scripts() {
        let tokens = estimate_tokens("Hello こんにちは مرحبا");
        assert!(tokens > 0);
    }

    #[test]
    fn emoji() {
        let tokens = estimate_tokens("🎉🚀👍");
        assert!(tokens >= 2, "got {tokens}");
    }

    #[test]
    fn min_clamp() {
        assert_eq!(estimate_tokens_min("", 5), 5);
    }

    #[test]
    fn long_text_proportional() {
        let short = estimate_tokens("Hello world");
        let long = estimate_tokens("Hello world Hello world Hello world");
        assert!(long > short, "long={long} should be > short={short}");
    }
}
