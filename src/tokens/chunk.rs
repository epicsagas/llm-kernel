//! Document chunking by sentence boundary and token budget.
//!
//! Splits text into token-budgeted chunks along sentence boundaries, with
//! optional overlap between consecutive chunks so boundary context is
//! preserved. Sentence terminators are script-aware: ASCII `.`, `!`, `?` and
//! the CJK full-stop/exclamation/question marks (U+3002, U+FF01, U+FF1F).
//! Newlines also act as unit boundaries.
//!
//! Token counts reuse [`crate::tokens::estimate_tokens`]; this module adds no
//! new dependencies.

use crate::tokens::estimate_tokens;

/// Options controlling sentence-aware chunking.
#[derive(Debug, Clone, Copy)]
pub struct ChunkOptions {
    /// Maximum estimated tokens per chunk.
    pub max_tokens: usize,
    /// Target token overlap between consecutive chunks.
    pub overlap_tokens: usize,
}

impl ChunkOptions {
    /// Create chunking options with the given token budget and overlap.
    ///
    /// `max_tokens` must be greater than zero; `overlap_tokens` may be zero to
    /// disable overlap. If `overlap_tokens` is greater than or equal to
    /// `max_tokens` it is clamped down to `max_tokens - 1` so progress is still
    /// guaranteed.
    pub fn new(max_tokens: usize, overlap_tokens: usize) -> Self {
        let max_tokens = max_tokens.max(1);
        // Clamp overlap so it never fully equals the budget, otherwise the
        // overlap window alone could saturate a chunk and stall progress.
        let overlap_tokens = overlap_tokens.min(max_tokens.saturating_sub(1));
        Self {
            max_tokens,
            overlap_tokens,
        }
    }
}

impl Default for ChunkOptions {
    /// Default options: 512 tokens per chunk with 64 tokens of overlap.
    ///
    /// A sensible middle ground for short-context retrieval: chunks are small
    /// enough to feed most embedding models, while the overlap keeps adjacent
    /// chunks sharing context across their boundary.
    fn default() -> Self {
        Self::new(512, 64)
    }
}

/// Returns `true` if `ch` is a sentence terminator.
///
/// Covers ASCII `.`, `!`, `?` and the CJK full-width forms U+3002 (。),
/// U+FF01 (！), and U+FF1F (？).
fn is_terminator(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '\u{3002}' | '\u{FF01}' | '\u{FF1F}')
}

/// Splits `text` into trimmed, non-empty sentence units.
///
/// A unit is the text up to and including the next terminator (or the text
/// bounded by a newline). Terminators stay attached to their sentence; lines
/// separated by newlines become their own units.
fn segment(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            // A newline ends the current unit without keeping the newline.
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                units.push(trimmed.to_string());
            }
            current.clear();
            continue;
        }
        current.push(ch);
        if is_terminator(ch) {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                units.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        units.push(trimmed.to_string());
    }

    units
}

/// Split text into token-budgeted chunks along sentence boundaries.
///
/// The algorithm packs sentence units greedily into a chunk until adding the
/// next unit would exceed [`ChunkOptions::max_tokens`]. When a new chunk
/// starts, trailing units from the just-pushed chunk are carried over until
/// their combined estimate reaches [`ChunkOptions::overlap_tokens`], so
/// consecutive chunks share boundary context.
///
/// A single unit that on its own exceeds the budget is emitted as its own
/// chunk — content is never dropped. Empty or whitespace-only input yields an
/// empty vector.
pub fn chunk_text(text: &str, opts: &ChunkOptions) -> Vec<String> {
    let units = segment(text);
    if units.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<String> = Vec::new();
    // Inclusive index range [start..=end] of units packed into the active chunk.
    // `end` is the unit currently being tested for inclusion.
    let mut start: usize = 0;
    let mut end: usize = 0;

    while end < units.len() {
        let running: String = units[start..=end].join(" ");
        let running_tokens = estimate_tokens(&running);

        if running_tokens <= opts.max_tokens {
            // Fits — extend by one more unit on the next iteration.
            end += 1;
            continue;
        }

        // Exceeds budget. A lone overlong unit is emitted on its own.
        if start == end {
            chunks.push(running);
            end += 1;
            start = end;
            continue;
        }

        // The window [start..end) fits; emit it without `end`.
        let chunk_str: String = units[start..end].join(" ");
        chunks.push(chunk_str);

        // Seed the next chunk with the overlap window of the just-pushed chunk,
        // then resume testing at the overflowing unit `end`.
        let next_start = overlap_start(&units, start, end, opts.overlap_tokens);
        // Guard against stalls: if the overlap-seeded window `[next_start..=end]`
        // is identical to the window we just rejected (`[start..=end]`), the
        // overlap is not helping — start fresh at `end` instead so `end` always
        // advances. This happens when a single trailing unit dominates the
        // overlap budget.
        start = if next_start <= start { end } else { next_start };
    }

    // Emit any trailing packed units.
    if start < units.len() {
        let chunk_str: String = units[start..].join(" ");
        let trimmed = chunk_str.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    chunks
}

/// Picks the inclusive start index for the next chunk so that trailing units of
/// the just-pushed chunk `[prev_start..prev_end)` are carried as overlap.
///
/// Walks backward from `prev_end` collecting units until their combined
/// estimate exceeds `overlap_tokens`. Returns the earliest index whose slice
/// still fits the budget, guaranteeing at least `prev_end - 1` (one unit of
/// overlap) when `overlap_tokens > 0`.
fn overlap_start(
    units: &[String],
    prev_start: usize,
    prev_end: usize,
    overlap_tokens: usize,
) -> usize {
    if overlap_tokens == 0 || prev_end == 0 {
        return prev_end;
    }
    // Walk backward from prev_end, accumulating units into the overlap window.
    let mut acc = String::new();
    let mut new_start = prev_end;
    for i in (prev_start..prev_end).rev() {
        let candidate = if acc.is_empty() {
            units[i].clone()
        } else {
            format!("{} {}", units[i], acc)
        };
        if estimate_tokens(&candidate) > overlap_tokens {
            break;
        }
        acc = candidate;
        new_start = i;
    }
    // Guarantee at least one unit of overlap when overlap_tokens > 0, but never
    // move the start earlier than prev_start (would duplicate the whole chunk).
    if new_start == prev_end {
        new_start = prev_end.saturating_sub(1).max(prev_start);
    }
    new_start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_chunks() {
        let opts = ChunkOptions::new(100, 0);
        assert!(chunk_text("", &opts).is_empty());
    }

    #[test]
    fn whitespace_only_yields_no_chunks() {
        let opts = ChunkOptions::new(100, 0);
        assert!(chunk_text("   \n\t  \n", &opts).is_empty());
    }

    #[test]
    fn latin_packs_within_budget() {
        let text = "The quick brown fox jumps. A lazy dog sleeps under the tree. \
                    Birds sing in the morning light. Rain falls softly on the roof. \
                    Children play in the garden all day.";
        let opts = ChunkOptions::new(20, 8);
        let chunks = chunk_text(text, &opts);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        for (i, c) in chunks.iter().enumerate() {
            let est = estimate_tokens(c);
            assert!(
                est <= opts.max_tokens,
                "chunk {i} has {est} tokens > max {}",
                opts.max_tokens
            );
        }
    }

    #[test]
    fn latin_chunks_share_overlap() {
        let text = "The quick brown fox jumps. A lazy dog sleeps under the tree. \
                    Birds sing in the morning light. Rain falls softly on the roof. \
                    Children play in the garden all day.";
        let opts = ChunkOptions::new(20, 8);
        let chunks = chunk_text(text, &opts);
        assert!(chunks.len() > 1);
        for w in chunks.windows(2) {
            // Extract the last sentence of the first chunk and confirm it is
            // carried into the next chunk as overlap context.
            let last_sentence = w[0]
                .split(|c: char| matches!(c, '.' | '!' | '?'))
                .filter(|s| !s.trim().is_empty())
                .next_back()
                .unwrap_or("")
                .trim();
            assert!(
                !last_sentence.is_empty(),
                "expected a trailing sentence to carry into overlap"
            );
            assert!(
                w[1].contains(last_sentence),
                "overlap missing: '{last_sentence}' not in next chunk"
            );
        }
    }

    #[test]
    fn cjk_fullstop_splits_and_packs() {
        // Five short clauses separated by the CJK full-stop U+3002.
        let text =
            "今日は晴れます。明日は雨が降る。明後日は風が強い。夜は涼しいです。朝はとても寒い。";
        let opts = ChunkOptions::new(10, 4);
        let chunks = chunk_text(text, &opts);
        assert!(
            chunks.len() > 1,
            "expected multiple CJK chunks, got {}",
            chunks.len()
        );
        // Reassembling the chunks should preserve all five clauses.
        let joined = chunks.join("");
        for clause in [
            "今日は晴れます",
            "明日は雨が降る",
            "明後日は風が強い",
            "夜は涼しいです",
            "朝はとても寒い",
        ] {
            assert!(joined.contains(clause), "clause '{clause}' was dropped");
        }
        for (i, c) in chunks.iter().enumerate() {
            let est = estimate_tokens(c);
            assert!(
                est <= opts.max_tokens,
                "CJK chunk {i} has {est} tokens > max {}",
                opts.max_tokens
            );
        }
    }

    #[test]
    fn cjk_exclamation_and_question_terminate() {
        // U+FF01 (！) and U+FF1F (？) should also act as terminators.
        let text = "行きます！何をしますか？帰りましょう。";
        let opts = ChunkOptions::new(8, 0);
        let chunks = chunk_text(text, &opts);
        // With no overlap, all content is preserved across chunks.
        let joined = chunks.join("");
        for clause in ["行きます", "何をしますか", "帰りましょう"] {
            assert!(joined.contains(clause), "clause '{clause}' was dropped");
        }
    }

    #[test]
    fn single_overlong_unit_emitted_unchanged() {
        // A long run with no terminators and a tiny budget.
        let text = "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789";
        let opts = ChunkOptions::new(3, 0);
        let chunks = chunk_text(text, &opts);
        assert_eq!(
            chunks.len(),
            1,
            "a lone overlong unit must be a single chunk"
        );
        assert_eq!(chunks[0], text, "content must be preserved verbatim");
    }

    #[test]
    fn default_options_are_sensible() {
        let opts = ChunkOptions::default();
        assert_eq!(opts.max_tokens, 512);
        assert_eq!(opts.overlap_tokens, 64);
        assert!(opts.overlap_tokens < opts.max_tokens);
    }

    #[test]
    fn overlap_clamped_below_max() {
        // overlap equal to max must be clamped so progress is still possible.
        let opts = ChunkOptions::new(10, 10);
        assert!(opts.overlap_tokens < opts.max_tokens);
        let text = "One. Two. Three. Four. Five. Six. Seven. Eight.";
        let chunks = chunk_text(text, &opts);
        assert!(chunks.len() > 1, "clamped overlap must still split");
    }

    #[test]
    fn newlines_split_units() {
        let text = "first line\nsecond line\nthird line";
        let opts = ChunkOptions::new(5, 0);
        let chunks = chunk_text(text, &opts);
        assert!(chunks.len() > 1, "newlines should produce multiple units");
        let joined = chunks.join(" ");
        for line in ["first line", "second line", "third line"] {
            assert!(joined.contains(line), "line '{line}' was dropped");
        }
    }
}
