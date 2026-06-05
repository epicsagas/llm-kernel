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

/// Characters-per-token ratio lookup using match on Unicode code point ranges.
/// Compiles to a jump table — O(1) per character instead of linear scan.
fn char_cpt(ch: char) -> f32 {
    let cp = ch as u32;
    match cp {
        // Emoji emoticons, Misc symbols, Transport, Misc symbols
        0x1F600..=0x1F64F | 0x1F300..=0x1F5FF | 0x1F680..=0x1F6FF | 0x2600..=0x26FF => 1.0,
        // Hiragana, Katakana, CJK Unified, Hangul Syllables
        0x3040..=0x30FF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF => 1.5,
        // Arabic, Devanagari, Thai
        0x0600..=0x06FF | 0x0900..=0x097F | 0x0E00..=0x0E7F => 2.0,
        _ => DEFAULT_CPT,
    }
}

/// Default chars-per-token for Latin/basic ASCII text.
const DEFAULT_CPT: f32 = 4.0;

/// Estimate the number of tokens in a string using Unicode-script heuristics.
///
/// This is a rough estimate (±20%) suitable for budget management.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    let mut total_weight: f32 = 0.0;

    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_ascii_control() {
            continue;
        }
        total_weight += 1.0 / char_cpt(ch);
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
