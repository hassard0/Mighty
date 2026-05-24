//! UTF-16-aware line / column index for a document.
//!
//! LSP positions are `(line, character)` with `character` counted in
//! **UTF-16 code units** (the historical default; LSP 3.17 lets the
//! client negotiate UTF-8/UTF-32, but VS Code still defaults to
//! UTF-16, so we implement the UTF-16 path and document the limitation).
//!
//! Our compiler pipeline uses **UTF-8 byte offsets** everywhere
//! (`SourceSpan { start: u32, end: u32 }`), so the LSP layer needs to
//! translate in both directions.

/// Mapping table: for each line, store the starting **byte** offset of
/// the line. Line N is the substring from `line_starts[N]` (inclusive)
/// to `line_starts[N+1]` (exclusive), with an implicit final entry at
/// `source.len()`.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line start. Always includes 0 at index 0.
    line_starts: Vec<u32>,
    /// Total source length in bytes — used as the implicit last line end.
    len: u32,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        let bytes = source.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                let next = (i + 1) as u32;
                line_starts.push(next);
            }
        }
        Self {
            line_starts,
            len: bytes.len() as u32,
        }
    }

    /// How many lines? (Counts the line *after* the last `\n` if there is one.)
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Total bytes.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Convert a UTF-8 byte offset → LSP `(line, character)` where
    /// character is a UTF-16 code-unit count from the start of the line.
    /// Clamped to the end of the source if `offset` is out of range.
    pub fn offset_to_position(&self, source: &str, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.len);
        // Binary search: find the largest line_start ≤ offset.
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line] as usize;
        let line_end = offset as usize;
        let line_bytes = &source.as_bytes()[line_start..line_end.min(source.len())];
        let line_str = std::str::from_utf8(line_bytes).unwrap_or("");
        let character = line_str.chars().map(|c| c.len_utf16() as u32).sum();
        (line as u32, character)
    }

    /// Convert LSP `(line, character)` → UTF-8 byte offset.
    /// `character` is a UTF-16 code-unit count.
    /// Clamped to the source bounds.
    pub fn position_to_offset(&self, source: &str, line: u32, character: u32) -> u32 {
        let line = (line as usize).min(self.line_starts.len().saturating_sub(1));
        let line_start = self.line_starts[line] as usize;
        let next_line_start = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] as usize
        } else {
            source.len()
        };
        // The line text excludes the trailing '\n' (so column lookups
        // don't drift onto the next line for end-of-line cursors).
        let line_text = {
            let s = &source[line_start..next_line_start];
            s.strip_suffix('\n').unwrap_or(s)
        };
        let mut utf16_count: u32 = 0;
        let mut byte_in_line: u32 = 0;
        for c in line_text.chars() {
            if utf16_count >= character {
                break;
            }
            utf16_count += c.len_utf16() as u32;
            byte_in_line += c.len_utf8() as u32;
        }
        let offset = line_start as u32 + byte_in_line;
        offset.min(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trip() {
        let src = "fn main() {\n    log(\"hi\")\n}\n";
        let li = LineIndex::new(src);
        assert_eq!(li.line_count(), 4);
        let (l, c) = li.offset_to_position(src, 0);
        assert_eq!((l, c), (0, 0));
        // Position at start of "log" on line 1 (col 4).
        let off = li.position_to_offset(src, 1, 4);
        assert_eq!(&src[off as usize..off as usize + 3], "log");
        let (l2, c2) = li.offset_to_position(src, off);
        assert_eq!((l2, c2), (1, 4));
    }

    #[test]
    fn utf8_multibyte() {
        // 'é' = 2 bytes in UTF-8, 1 code unit in UTF-16.
        let src = "let café = 1\n";
        let li = LineIndex::new(src);
        // Byte offset of '=' = "let café ".len() bytes.
        let eq_byte = src.find('=').unwrap() as u32;
        let (l, c) = li.offset_to_position(src, eq_byte);
        assert_eq!(l, 0);
        // "let café " = 9 chars = 9 UTF-16 code units.
        assert_eq!(c, 9);
        let back = li.position_to_offset(src, l, c);
        assert_eq!(back, eq_byte);
    }

    #[test]
    fn surrogate_pair() {
        // U+1F600 (grinning face) — 4 bytes UTF-8, 2 code units UTF-16.
        let src = "x = \"\u{1F600}\"\n";
        let li = LineIndex::new(src);
        let q_byte = src.rfind('"').unwrap() as u32;
        let (l, c) = li.offset_to_position(src, q_byte);
        assert_eq!(l, 0);
        // "x = \"" = 5 chars, then 2 UTF-16 units for the emoji = 7.
        assert_eq!(c, 7);
        // Round trip.
        assert_eq!(li.position_to_offset(src, l, c), q_byte);
    }

    #[test]
    fn out_of_range_clamped() {
        let src = "ok\n";
        let li = LineIndex::new(src);
        assert_eq!(li.position_to_offset(src, 99, 99), li.len);
        assert_eq!(li.offset_to_position(src, 99), (1, 0));
    }
}
