//! `Regex` — compiled pattern + the methods Mighty source calls.
//!
//! Thin wrapper over [`regex::Regex`] from the `regex` crate. The
//! wrapper shapes the surface to what reads well in Mighty source:
//!
//! - `Regex::new(pattern)` → compiled `Regex`
//! - `r.find(hay)` → `Option<Match>`
//! - `r.find_all(hay)` → `Vec<Match>`
//! - `r.captures(hay)` → `Option<Captures>`
//! - `r.replace_all(hay, rep)` → `String`
//! - `r.is_match(hay)` → `bool`
//! - `r.split(hay)` → `Vec<String>`

use super::r#match::{Captures, Match};

/// Compiled regular expression.
#[derive(Debug, Clone)]
pub struct Regex {
    inner: ::regex::Regex,
}

impl Regex {
    /// Compile a regex pattern. Returns [`RegexErr::Compile`] if the
    /// pattern is malformed.
    ///
    /// Anchors, groups, alternation, repetition, Unicode categories
    /// and ASCII shorthands (`\d \w \s`) are all supported. Look-around
    /// is intentionally NOT supported — the `regex` crate uses RE2-style
    /// finite automata for guaranteed linear time.
    pub fn new(pattern: &str) -> Result<Self, RegexErr> {
        ::regex::Regex::new(pattern)
            .map(|inner| Self { inner })
            .map_err(|e| RegexErr::Compile(e.to_string()))
    }

    /// First match in `haystack`, or `None` if the pattern doesn't fire.
    #[must_use]
    pub fn find(&self, haystack: &str) -> Option<Match> {
        self.inner
            .find(haystack)
            .map(|m| Match::from_haystack(haystack, m.start(), m.end()))
    }

    /// All non-overlapping matches in `haystack`, left to right.
    #[must_use]
    pub fn find_all(&self, haystack: &str) -> Vec<Match> {
        self.inner
            .find_iter(haystack)
            .map(|m| Match::from_haystack(haystack, m.start(), m.end()))
            .collect()
    }

    /// Capture groups for the first match in `haystack`, or `None` if
    /// the pattern doesn't fire. Group 0 is the whole match; groups
    /// 1.. are the parenthesised subgroups in left-to-right order. A
    /// group that did not participate (e.g. an alternative that didn't
    /// fire) is `None`.
    #[must_use]
    pub fn captures(&self, haystack: &str) -> Option<Captures> {
        self.inner.captures(haystack).map(|cs| {
            let groups = cs
                .iter()
                .map(|opt_m| opt_m.map(|m| Match::from_haystack(haystack, m.start(), m.end())))
                .collect();
            Captures { groups }
        })
    }

    /// All capture groups for every match. Useful for "scan + extract"
    /// loops without zipping `find_all` and `captures` by index.
    #[must_use]
    pub fn captures_all(&self, haystack: &str) -> Vec<Captures> {
        self.inner
            .captures_iter(haystack)
            .map(|cs| {
                let groups = cs
                    .iter()
                    .map(|opt_m| opt_m.map(|m| Match::from_haystack(haystack, m.start(), m.end())))
                    .collect();
                Captures { groups }
            })
            .collect()
    }

    /// Replace every match of the pattern with the replacement string.
    /// The replacement supports `$0`, `$1`, ... backrefs to capture
    /// groups (see the `regex` crate's `Regex::replace_all` docs).
    #[must_use]
    pub fn replace_all(&self, haystack: &str, replacement: &str) -> String {
        self.inner.replace_all(haystack, replacement).into_owned()
    }

    /// Replace only the first match. See [`Regex::replace_all`] for
    /// replacement syntax.
    #[must_use]
    pub fn replace(&self, haystack: &str, replacement: &str) -> String {
        self.inner.replace(haystack, replacement).into_owned()
    }

    /// Cheap "does this haystack contain any match?" predicate.
    #[must_use]
    pub fn is_match(&self, haystack: &str) -> bool {
        self.inner.is_match(haystack)
    }

    /// Split `haystack` on every match. The matches themselves are
    /// removed from the output (CSV-style splitter on `,\s*` is a
    /// typical use).
    #[must_use]
    pub fn split(&self, haystack: &str) -> Vec<String> {
        self.inner.split(haystack).map(str::to_string).collect()
    }

    /// The original pattern string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }
}

/// Errors returned by `std.regex`.
#[derive(Debug, thiserror::Error)]
pub enum RegexErr {
    #[error("regex compile: {0}")]
    Compile(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- compile ----------------------------------------------------------

    #[test]
    fn compile_simple_literal() {
        let r = Regex::new("hello").unwrap();
        assert!(r.is_match("say hello there"));
        assert!(!r.is_match("say goodbye there"));
    }

    #[test]
    fn compile_with_classes_and_repetition() {
        let r = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
        assert!(r.is_match("date: 2026-05-30"));
    }

    #[test]
    fn compile_rejects_bad_pattern() {
        // Unclosed group is a syntax error.
        let err = Regex::new("(unclosed").unwrap_err();
        let s = format!("{}", err);
        assert!(s.starts_with("regex compile:"), "{}", s);
    }

    #[test]
    fn as_str_returns_original_pattern() {
        let r = Regex::new(r"\w+").unwrap();
        assert_eq!(r.as_str(), r"\w+");
    }

    // ---- find -------------------------------------------------------------

    #[test]
    fn find_returns_first_match_only() {
        let r = Regex::new(r"\d+").unwrap();
        let m = r.find("a12 b34 c56").unwrap();
        assert_eq!(m.text, "12");
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn find_returns_none_on_no_match() {
        let r = Regex::new(r"\d+").unwrap();
        assert!(r.find("no digits here").is_none());
    }

    #[test]
    fn find_iso_date() {
        let r = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
        let m = r.find("date: 2026-05-30").unwrap();
        assert_eq!(m.text, "2026-05-30");
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 16);
    }

    #[test]
    fn find_handles_anchors() {
        let r = Regex::new(r"^hello").unwrap();
        assert!(r.find("hello world").is_some());
        assert!(r.find("say hello").is_none());
    }

    #[test]
    fn find_unicode_word() {
        // \w under Unicode mode matches café's é.
        let r = Regex::new(r"\w+").unwrap();
        let m = r.find("café 123").unwrap();
        assert_eq!(m.text, "café");
    }

    // ---- find_all ---------------------------------------------------------

    #[test]
    fn find_all_returns_every_match() {
        let r = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
        let all = r.find_all("2026-05-30 to 2026-06-01");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "2026-05-30");
        assert_eq!(all[1].text, "2026-06-01");
        // Offsets are byte-accurate.
        assert_eq!(all[0].start, 0);
        assert_eq!(all[1].start, 14);
    }

    #[test]
    fn find_all_empty_on_no_match() {
        let r = Regex::new(r"\d+").unwrap();
        assert!(r.find_all("no digits").is_empty());
    }

    #[test]
    fn find_all_non_overlapping() {
        // "aaaa" matched by "aa" yields TWO matches at 0 and 2, not
        // three — the engine doesn't overlap by default.
        let r = Regex::new("aa").unwrap();
        let all = r.find_all("aaaa");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].start, 0);
        assert_eq!(all[1].start, 2);
    }

    // ---- captures ---------------------------------------------------------

    #[test]
    fn captures_extracts_groups() {
        let r = Regex::new(r"(\w+)=(\w+)").unwrap();
        let caps = r.captures("key=value").unwrap();
        assert_eq!(caps.len(), 3);
        assert_eq!(caps.get(0).unwrap().text, "key=value");
        assert_eq!(caps.get(1).unwrap().text, "key");
        assert_eq!(caps.get(2).unwrap().text, "value");
    }

    #[test]
    fn captures_returns_none_on_no_match() {
        let r = Regex::new(r"(\w+)=(\w+)").unwrap();
        assert!(r.captures("nothing here").is_none());
    }

    #[test]
    fn captures_marks_non_participating_groups_none() {
        // Either-or — one of the two alternatives matches.
        let r = Regex::new(r"(\d+)|([a-z]+)").unwrap();
        let caps = r.captures("hello").unwrap();
        assert_eq!(caps.len(), 3);
        // Group 0 = whole match
        assert_eq!(caps.get(0).unwrap().text, "hello");
        // Group 1 = digits, did NOT participate
        assert!(caps.get(1).is_none());
        // Group 2 = letters, DID participate
        assert_eq!(caps.get(2).unwrap().text, "hello");
    }

    #[test]
    fn captures_all_iterates_every_match() {
        let r = Regex::new(r"(\w+)=(\w+)").unwrap();
        let all = r.captures_all("a=1 b=2 c=3");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].get(1).unwrap().text, "a");
        assert_eq!(all[0].get(2).unwrap().text, "1");
        assert_eq!(all[2].get(1).unwrap().text, "c");
        assert_eq!(all[2].get(2).unwrap().text, "3");
    }

    // ---- replace ----------------------------------------------------------

    #[test]
    fn replace_all_swaps_every_match() {
        let r = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
        let out = r.replace_all("date: 2026-05-30 then 2026-06-01", "[date]");
        assert_eq!(out, "date: [date] then [date]");
    }

    #[test]
    fn replace_first_only() {
        let r = Regex::new(r"\d+").unwrap();
        let out = r.replace("a1 b2 c3", "X");
        assert_eq!(out, "aX b2 c3");
    }

    #[test]
    fn replace_with_backrefs() {
        // $1 $2 backrefs reference capture groups.
        let r = Regex::new(r"(\w+)=(\w+)").unwrap();
        let out = r.replace_all("a=1 b=2", "$2=$1");
        assert_eq!(out, "1=a 2=b");
    }

    #[test]
    fn replace_all_no_match_returns_input_unchanged() {
        let r = Regex::new(r"\d+").unwrap();
        let out = r.replace_all("no digits", "X");
        assert_eq!(out, "no digits");
    }

    // ---- is_match + split -------------------------------------------------

    #[test]
    fn is_match_works() {
        let r = Regex::new(r"^[A-Z][a-z]+$").unwrap();
        assert!(r.is_match("Hello"));
        assert!(!r.is_match("hello"));
        assert!(!r.is_match("HELLO"));
    }

    #[test]
    fn split_on_comma_whitespace() {
        let r = Regex::new(r",\s*").unwrap();
        let parts = r.split("a, b,c,  d");
        assert_eq!(parts, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn split_on_no_match_returns_single_element() {
        let r = Regex::new(r"\d+").unwrap();
        let parts = r.split("no digits here");
        assert_eq!(parts, vec!["no digits here"]);
    }

    // ---- real-world fixtures ---------------------------------------------

    #[test]
    fn extract_log_level_and_message() {
        let r = Regex::new(r"^\[(INFO|WARN|ERROR)\]\s+(.+)$").unwrap();
        let caps = r.captures("[ERROR] connection refused").unwrap();
        assert_eq!(caps.get(1).unwrap().text, "ERROR");
        assert_eq!(caps.get(2).unwrap().text, "connection refused");
    }

    #[test]
    fn email_local_at_domain() {
        let r = Regex::new(r"([A-Za-z0-9._%+-]+)@([A-Za-z0-9.-]+\.[A-Za-z]{2,})").unwrap();
        let caps = r
            .captures("contact me at ihassard@example.com please")
            .unwrap();
        assert_eq!(caps.get(1).unwrap().text, "ihassard");
        assert_eq!(caps.get(2).unwrap().text, "example.com");
    }

    #[test]
    fn ipv4_dotted_quad() {
        let r = Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap();
        let caps = r.captures("connecting to 192.168.4.193:8080").unwrap();
        assert_eq!(caps.get(0).unwrap().text, "192.168.4.193");
        assert_eq!(caps.get(1).unwrap().text, "192");
        assert_eq!(caps.get(4).unwrap().text, "193");
    }

    #[test]
    fn url_path_query_extract() {
        let r = Regex::new(r"^([^?]+)\?(.+)$").unwrap();
        let caps = r.captures("/api/v1/items?q=hello&p=2").unwrap();
        assert_eq!(caps.get(1).unwrap().text, "/api/v1/items");
        assert_eq!(caps.get(2).unwrap().text, "q=hello&p=2");
    }
}
