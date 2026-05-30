//! `Match` and `Captures` — the value types returned by
//! [`Regex::find`](super::Regex::find),
//! [`Regex::find_all`](super::Regex::find_all), and
//! [`Regex::captures`](super::Regex::captures).
//!
//! Both types are plain owned data — they outlive the haystack and
//! don't borrow from the input. This is the right ergonomics for
//! Mighty source where the regex result is typically stored in a
//! field or returned from a function.

/// One regex match: the matched substring and the byte offsets it
/// spans in the haystack.
///
/// `start` / `end` are byte offsets (UTF-8) — the same convention the
/// underlying `regex` crate uses. For typical ASCII matches this is
/// also the character index; for matches spanning multi-byte UTF-8 the
/// offsets remain byte-accurate, which is what every downstream API
/// (slicing, error spans, etc) needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

impl Match {
    /// Build a `Match` from a haystack slice and byte range. Internal
    /// helper — Mighty callers go through [`Regex::find`](super::Regex::find).
    #[must_use]
    pub(crate) fn from_haystack(hay: &str, start: usize, end: usize) -> Self {
        Self {
            text: hay[start..end].to_string(),
            start,
            end,
        }
    }

    /// Length of the matched substring in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Is the matched substring empty? Empty matches happen with
    /// patterns like `^`, `$`, or `\b`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// All capture groups from a single regex match. `groups[0]` is the
/// overall match; `groups[1..]` are the parenthesised subgroups in
/// left-to-right order. A group that did not participate (e.g. an
/// alternative that didn't fire) is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captures {
    pub groups: Vec<Option<Match>>,
}

impl Captures {
    /// Look up a group by index. `0` is the whole match; `1..` are the
    /// parenthesised subgroups. Returns `None` for out-of-range or
    /// non-participating groups.
    #[must_use]
    pub fn get(&self, idx: usize) -> Option<&Match> {
        self.groups.get(idx).and_then(|g| g.as_ref())
    }

    /// Number of groups (including group 0). Always >= 1 for a
    /// successful match.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Is there nothing here? Returns `false` for any successful
    /// match (group 0 is always present).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_from_haystack_carries_text_and_offsets() {
        let m = Match::from_haystack("hello world", 6, 11);
        assert_eq!(m.text, "world");
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 11);
        assert_eq!(m.len(), 5);
        assert!(!m.is_empty());
    }

    #[test]
    fn empty_match_is_empty() {
        let m = Match::from_haystack("abc", 1, 1);
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert_eq!(m.text, "");
    }

    #[test]
    fn captures_get_returns_groups() {
        let caps = Captures {
            groups: vec![
                Some(Match::from_haystack("k=v", 0, 3)),
                Some(Match::from_haystack("k=v", 0, 1)),
                Some(Match::from_haystack("k=v", 2, 3)),
            ],
        };
        assert_eq!(caps.len(), 3);
        assert_eq!(caps.get(0).unwrap().text, "k=v");
        assert_eq!(caps.get(1).unwrap().text, "k");
        assert_eq!(caps.get(2).unwrap().text, "v");
        assert!(caps.get(3).is_none());
    }

    #[test]
    fn captures_get_skips_non_participating() {
        let caps = Captures {
            groups: vec![Some(Match::from_haystack("a", 0, 1)), None],
        };
        assert!(caps.get(0).is_some());
        assert!(caps.get(1).is_none());
    }
}
