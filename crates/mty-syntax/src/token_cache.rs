//! Incremental token cache for LSP-style edits (v0.8, parse-throughput target).
//!
//! The LSP and the formatter sit on top of `mty_syntax::lex`. Every
//! keystroke today triggers a *full* re-lex of the buffer — fine for a
//! 1 KLOC file, painful for the 10 KLOC fixture the parse_throughput
//! bench exercises. v0.8 introduces a simple token cache:
//!
//! - On first lex, we cache the full token stream + the source.
//! - On a subsequent edit `[start, end) → replacement`, we:
//!   1. Find the first cached token whose span ends at or after
//!      `start` (the "left edge" of the dirty region).
//!   2. Find the first cached token whose span starts at or after
//!      `end` (the "right edge").
//!   3. Re-lex the slice `[left_edge.start, end + delta_len)` of the
//!      new source.
//!   4. Splice the new tokens into the cache, shifting the right-side
//!      tokens by `replacement.len() - (end - start)`.
//!
//! The cache stores token kinds + spans (not the borrowed `&str`) so
//! callers can rebuild a fresh `Vec<LexedToken>` against the new source
//! buffer cheaply.
//!
//! ## Determinism
//!
//! A full re-lex of the post-edit buffer MUST produce the same token
//! stream as the cache's `apply_edit + full_tokens`. The
//! `incremental_matches_fresh` test exercises this property over a
//! handful of edit shapes. Larger property tests live in the
//! lex_throughput bench.

use crate::lexer::lex;
use crate::SyntaxKind;

/// A position-independent record of one lexed token. Unlike
/// [`crate::lexer::LexedToken`] we don't hold a borrowed slice so the
/// cache survives across edits (the source buffer changes underneath).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedToken {
    pub kind: SyntaxKind,
    pub start: usize,
    pub end: usize,
}

impl CachedToken {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// A cached token stream + source snapshot.
///
/// `TokenCache::lex` performs a full lex and is the starting point for
/// any subsequent incremental re-lex.
#[derive(Debug, Clone)]
pub struct TokenCache {
    src: String,
    tokens: Vec<CachedToken>,
}

impl TokenCache {
    /// Build the cache from scratch by fully lexing `src`.
    pub fn lex(src: impl Into<String>) -> Self {
        let src: String = src.into();
        let toks: Vec<CachedToken> = lex(&src)
            .into_iter()
            .filter(|t| t.kind != SyntaxKind::EOF)
            .map(|t| CachedToken {
                kind: t.kind,
                start: t.start,
                end: t.end,
            })
            .collect();
        Self { src, tokens: toks }
    }

    /// Borrow the current source snapshot.
    pub fn source(&self) -> &str {
        &self.src
    }

    /// Borrow the current cached token stream.
    pub fn tokens(&self) -> &[CachedToken] {
        &self.tokens
    }

    /// Apply an incremental edit `[start, end) → replacement` and
    /// re-lex only the affected region.
    ///
    /// Returns the number of *tokens* that were re-lexed (useful for
    /// benchmarks + telemetry). A full re-lex returns
    /// `self.tokens.len()`; a typical edit re-lexes 1-3.
    pub fn apply_edit(&mut self, start: usize, end: usize, replacement: &str) -> usize {
        assert!(start <= end, "edit range start > end");
        assert!(end <= self.src.len(), "edit range past source end");

        // Build the new source buffer (out-of-place; a real LSP would
        // own a rope here).
        let mut new_src = String::with_capacity(self.src.len() + replacement.len());
        new_src.push_str(&self.src[..start]);
        new_src.push_str(replacement);
        new_src.push_str(&self.src[end..]);
        let new_len = new_src.len();
        let delta: isize = replacement.len() as isize - (end - start) as isize;

        // Find the left edge: the first cached token whose span ends
        // at or AFTER `start`. We re-lex from its start (so we don't
        // split a token).
        let mut left_idx = self
            .tokens
            .iter()
            .position(|t| t.end > start)
            .unwrap_or(self.tokens.len());
        // Conservatively widen left by one token to absorb adjacent
        // trivia that the fresh lex would coalesce (whitespace,
        // comments). Cheap and avoids merge-mismatch.
        left_idx = left_idx.saturating_sub(1);
        let lex_start = self
            .tokens
            .get(left_idx)
            .map(|t| t.start)
            .unwrap_or(self.src.len())
            .min(start);

        // Find the right edge: the first cached token whose start is
        // STRICTLY AFTER `end` (so its span doesn't touch the edit).
        let mut right_idx = self
            .tokens
            .iter()
            .position(|t| t.start > end)
            .unwrap_or(self.tokens.len());
        // Conservatively widen right by one token for the same
        // trivia-coalescing reason.
        if right_idx < self.tokens.len() {
            right_idx += 1;
        }

        // Conservative widen: re-lex from lex_start to the start of
        // the right-edge token (or end-of-buffer). This is the
        // smallest region that captures the edit + any tokens that
        // straddle it.
        let widen_end_old = self
            .tokens
            .get(right_idx)
            .map(|t| t.start)
            .unwrap_or(self.src.len());
        // Translate widen_end_old (in old-source coordinates) into
        // new-source coordinates.
        let widen_end_new = if widen_end_old >= end {
            let shifted = widen_end_old as isize + delta;
            shifted.max(0) as usize
        } else {
            widen_end_old
        };
        let widen_end_new = widen_end_new.min(new_len);

        // Re-lex the slice.
        let relex_src = &new_src[lex_start..widen_end_new];
        let relexed: Vec<CachedToken> = lex(relex_src)
            .into_iter()
            .filter(|t| t.kind != SyntaxKind::EOF)
            .map(|t| CachedToken {
                kind: t.kind,
                start: lex_start + t.start,
                end: lex_start + t.end,
            })
            .collect();

        // Build the new token vector:
        //   self.tokens[..left_idx]   (unchanged)
        // + relexed                   (re-lexed)
        // + self.tokens[right_idx..] with spans shifted by `delta`
        let mut new_tokens =
            Vec::with_capacity(left_idx + relexed.len() + (self.tokens.len() - right_idx));
        new_tokens.extend_from_slice(&self.tokens[..left_idx]);
        let relex_count = relexed.len();
        new_tokens.extend(relexed);
        for t in &self.tokens[right_idx..] {
            new_tokens.push(CachedToken {
                kind: t.kind,
                start: (t.start as isize + delta).max(0) as usize,
                end: (t.end as isize + delta).max(0) as usize,
            });
        }

        self.src = new_src;
        self.tokens = new_tokens;
        relex_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_lex_kinds(src: &str) -> Vec<(SyntaxKind, usize, usize)> {
        lex(src)
            .into_iter()
            .filter(|t| t.kind != SyntaxKind::EOF)
            .map(|t| (t.kind, t.start, t.end))
            .collect()
    }

    fn cache_kinds(c: &TokenCache) -> Vec<(SyntaxKind, usize, usize)> {
        c.tokens()
            .iter()
            .map(|t| (t.kind, t.start, t.end))
            .collect()
    }

    #[test]
    fn fresh_cache_matches_full_lex() {
        let src = "fn main() -> I64 { 1 + 2 }";
        let c = TokenCache::lex(src);
        assert_eq!(cache_kinds(&c), full_lex_kinds(src));
    }

    #[test]
    fn incremental_insert_matches_fresh() {
        let src = "fn main() -> I64 { 1 + 2 }";
        let mut c = TokenCache::lex(src);
        // Insert " * 3" before the trailing "}".
        let pos = src.find('}').unwrap();
        c.apply_edit(pos, pos, " * 3 ");
        let new_full = "fn main() -> I64 { 1 + 2  * 3 }";
        assert_eq!(c.source(), new_full);
        assert_eq!(cache_kinds(&c), full_lex_kinds(new_full));
    }

    #[test]
    fn incremental_delete_matches_fresh() {
        let src = "fn main() -> I64 { 1 + 2 + 3 }";
        let mut c = TokenCache::lex(src);
        // Delete " + 3" (range ends just before the trailing space + "}").
        let close = src.find('}').unwrap();
        c.apply_edit(close - 4, close - 1, "");
        let new_full = format!("{}{}", &src[..close - 4], &src[close - 1..]);
        assert_eq!(c.source(), new_full);
        assert_eq!(cache_kinds(&c), full_lex_kinds(&new_full));
    }

    #[test]
    fn incremental_replace_matches_fresh() {
        let src = "fn main() -> I64 { foo + bar }";
        let mut c = TokenCache::lex(src);
        let start = src.find("foo").unwrap();
        c.apply_edit(start, start + 3, "BAZ");
        let new_full = "fn main() -> I64 { BAZ + bar }";
        assert_eq!(c.source(), new_full);
        assert_eq!(cache_kinds(&c), full_lex_kinds(new_full));
    }

    #[test]
    fn incremental_reduces_relex_count() {
        use std::fmt::Write as _;
        // Insert in the *middle* of a 10 KLOC source. The number of
        // re-lexed tokens should be O(1), not O(N).
        let mut s = String::new();
        for i in 0..1000 {
            let _ = writeln!(s, "fn f{i}() -> I64 {{ 0 }}");
        }
        let mut c = TokenCache::lex(&s);
        let total = c.tokens().len();
        // Insert a comment in the middle.
        let mid = s.len() / 2;
        // Snap to a newline so we don't split a token.
        let mid = s[mid..].find('\n').map(|p| mid + p + 1).unwrap_or(mid);
        let relex = c.apply_edit(mid, mid, "// hi\n");
        // 'total' is large; relex should be only a handful of tokens.
        assert!(
            relex < total / 10,
            "relexed {relex} of {total} — incremental did not narrow"
        );
    }
}
