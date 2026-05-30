# `std.regex` — regular expressions (internals, v0.40 T4)

**Module:** `mty_stdlib::regex` (submodules `regex`, `match`)
**Mighty surface:** `use std.regex.Regex`

`std.regex` is the regex surface that every web app, log parser,
input validator, and templating layer eventually wants. v0.40 T4
adds it on top of the [`regex`](https://docs.rs/regex/) crate — the
RE2-style finite-automata engine that guarantees linear time in the
input.

## Module shape

```
crates/mty-stdlib/src/regex/
  ├── mod.rs   — top-level Regex + Match + Captures re-exports
  ├── regex.rs — `Regex` struct, all matching methods, `RegexErr`
  └── match.rs — `Match` { text, start, end } + `Captures { groups }`
```

## Surface

```rust
let r = Regex::new(r"\d{4}-\d{2}-\d{2}")?;

// Boolean predicate.
r.is_match("date: 2026-05-30")          // -> bool

// First match.
r.find("date: 2026-05-30")              // -> Option<Match>

// All non-overlapping matches, left to right.
r.find_all("2026-05-30 to 2026-06-01")  // -> Vec<Match>

// Capture groups (group 0 = whole match, 1.. = parens).
r.captures("key=value")                  // -> Option<Captures>
r.captures_all("a=1 b=2 c=3")            // -> Vec<Captures>

// Substitution. Replacement supports $0, $1, ... backrefs.
r.replace("a1 b2 c3", "X")              // -> String (first only)
r.replace_all("date: 2026-05-30", "[d]") // -> String (every match)

// Splitter.
r.split("a, b,c,  d")                    // -> Vec<String>
```

## Match value type

```rust
struct Match {
    text: String,    // matched substring (owned)
    start: usize,    // byte offset in haystack
    end: usize,
}
```

`start` / `end` are **byte offsets** in UTF-8. For ASCII haystacks
this also matches character indices; for multi-byte UTF-8 the offsets
remain byte-accurate, which is what every downstream API
(slicing, error spans) wants.

`Match` is **owned** — it holds a `String`, not a borrow. This means
the match outlives the haystack, which is the right ergonomic for
Mighty source where match results typically get stored in fields or
returned from functions.

## Captures value type

```rust
struct Captures {
    groups: Vec<Option<Match>>,
}
```

`groups[0]` is the overall match; `groups[1..]` are the parenthesised
subgroups in left-to-right order.

A group that did not participate (e.g. an alternative that didn't
fire) is `None`. The `Captures::get(idx)` helper folds the
`Option<&Match>` for you.

## Syntax reference

The full regex syntax accepted by `Regex::new` is documented at
<https://docs.rs/regex/latest/regex/#syntax>. Highlights:

- **Character classes**: `\d \w \s \D \W \S` with Unicode-aware
  semantics by default (ASCII shorthands via `(?-u:\d)` etc.).
- **Anchors**: `^` `$` `\b` `\B` `\A` `\z`.
- **Repetition**: `*` `+` `?` `{n}` `{n,}` `{n,m}` (greedy) and
  lazy `*? +? ??` variants.
- **Groups**: `(pat)` capturing, `(?:pat)` non-capturing,
  `(?P<name>pat)` named.
- **Alternation**: `a|b`.
- **Look-around**: **NOT supported** — RE2 design trade-off for
  guaranteed linear time.

## Performance properties

- **Linear time** in the input, guaranteed. No catastrophic
  backtracking — the engine uses Thompson NFA simulation, not PCRE-
  style backtracking.
- **Compile cost is amortised**: cache the compiled `Regex` and reuse
  it. Compiling a pattern is O(pattern length) plus a small constant
  for the DFA cache.
- **Allocation**: `find` allocates one `String` for the match text
  (this is the price of owned `Match` values — see "Match value type"
  above). `find_all` allocates `n + 1` strings for `n` matches.
  `replace_all` allocates one `String` for the output.

## No capability required

Regex compilation and matching are pure functions over the pattern
and haystack. No I/O, no entropy, no clock — no capability gate
needed.

## Test coverage

31 tests across `regex.rs` + `match.rs`:

- **Compile** — simple literals, classes/repetition, malformed
  patterns (error path), `as_str` round-trip.
- **Find** — first match only, no-match path, anchors, Unicode word
  boundaries (`café`).
- **Find all** — every non-overlapping match, empty result path,
  the "aaaa matched by aa yields 2 not 3" rule.
- **Captures** — group extraction, no-match path, non-participating
  alternation groups, multi-match `captures_all`.
- **Replace** — first-only, all, backref expansion (`$1`, `$2`),
  no-op when nothing matches.
- **Real-world fixtures** — log-line parsing, email local@domain
  extraction, IPv4 dotted quad, URL path/query split.

## Deps added in v0.40 T4

| Crate | Version | Purpose |
|---|---|---|
| `regex` | 1 | RE2-style regex engine (already in lockfile transitively via tracing-subscriber's filter expressions) |

Promoting `regex` to a direct dep keeps the lockfile flat and makes
the version intent explicit.

## v0.41 follow-ups

- **RegexSet** — match against a set of patterns in a single pass
  (`regex::RegexSet` upstream). Useful for routers and log dispatchers.
- **Bytes regex** — `regex::bytes::Regex` for non-UTF-8 haystacks.
  Currently the surface is `&str`-only.
- **Replacement closures** — `replace_all_with(hay, |caps| ...)` so
  the replacement can be computed per-match from the captures.
