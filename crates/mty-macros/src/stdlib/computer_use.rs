//! The `@computer_use` attribute macro (v0.30 Track C).
//!
//! `@computer_use` is Mighty's standard way to declare an agent that
//! drives a real desktop via Anthropic's Computer Use API. The macro:
//!
//! 1. Parses the attribute args: `width: <U32>`, `height: <U32>`,
//!    and the required `cap:` capability expression (which the
//!    capability checker reads to authorise screen / input access at
//!    type-check time).
//! 2. Generates a hidden `__computer_use_spec_<NAME>()` fn returning
//!    a JSON spec the runtime hands to
//!    [`mty_stdlib::computer::dispatcher::Dispatcher`](../../mty-stdlib/src/computer/dispatcher.rs)
//!    so the user's agent can be wired up without writing the boilerplate.
//!
//! ## Surface
//!
//! ```mty
//! @computer_use(width: 1280, height: 800, cap: computer.screen + computer.input)
//! agent BrowserOperator {
//!   on Run(task: Str) -> Str {
//!     std.computer.run(task)
//!   }
//! }
//! ```
//!
//! ## Diagnostic codes
//!
//! - **MT6017** ([`ComputerUseMacroError::MissingCap`]) — `cap:` not
//!   supplied. Computer Use without a capability is never safe; we
//!   make the omission a hard error.
//! - **MT6018** ([`ComputerUseMacroError::MalformedCap`]) — `cap:`
//!   expression is not a dotted capability path or sum.
//! - **MT6019** ([`ComputerUseMacroError::MalformedDimension`]) —
//!   `width:` / `height:` is not a positive integer literal.
//! - **MT6020** ([`ComputerUseMacroError::NotAnAgent`]) —
//!   `@computer_use` decorates something other than an `agent` item.
//!
//! The macro mirrors the [`tool`](super::tool) macro's shape so the
//! HIR preprocessor handles both with a single attribute-dispatch
//! path.

use crate::stdlib::format::decode_string_literal;

/// Mighty source-side description of the macro's output. Returned by
/// [`expand_computer_use_attribute`] when expansion succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerUseExpansion {
    /// The user's original agent decl. Returned unchanged so the
    /// agent is still spawnable from regular Mighty code.
    pub original_decl: String,
    /// Source for the synthesised spec fn
    /// (`__computer_use_spec_<NAME>`). Each entry is a top-level
    /// Mighty fn decl ready to be parsed.
    pub synthesised_decls: Vec<String>,
    /// JSON-encoded spec the runtime can deserialise directly.
    pub spec_json: String,
}

/// Parsed `@computer_use(...)` attribute body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerUseAttributeArgs {
    pub width: u32,
    pub height: u32,
    pub capability: String,
    /// Optional model override — `model: "claude-opus-4-7"`. If
    /// absent the dispatcher's default applies.
    pub model: Option<String>,
}

/// Lightweight view of the user's `agent` decl the macro needs.
/// Mirrors [`tool::ParsedFn`](super::tool::ParsedFn) — the macro
/// operates on a pre-parsed shape so the HIR preprocessor can hand
/// it just the bits it has without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAgent {
    pub name: String,
    /// The original source slice — returned verbatim in
    /// [`ComputerUseExpansion::original_decl`].
    pub source: String,
    /// True iff this is really an `agent` decl. The HIR preprocessor
    /// sets this; the macro rejects with [`ComputerUseMacroError::NotAnAgent`]
    /// when false.
    pub is_agent: bool,
}

/// Errors the `@computer_use` macro can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComputerUseMacroError {
    /// MT6017 — `cap:` arg missing.
    MissingCap,
    /// MT6018 — `cap:` arg is not a dotted path / sum.
    MalformedCap { got: String },
    /// MT6019 — `width:` / `height:` is not a positive integer
    /// literal (or is zero, or overflows U32).
    MalformedDimension { which: &'static str, got: String },
    /// MT6020 — `@computer_use` decorates a non-agent item.
    NotAnAgent { item_kind: String },
}

impl ComputerUseMacroError {
    /// Map the variant to its bare MT diagnostic code so callers
    /// emitting from the HIR preprocessor can attach the canonical
    /// `MT6017`..`MT6020` numbers without re-deriving them.
    pub fn code(&self) -> u16 {
        match self {
            ComputerUseMacroError::MissingCap => crate::diag::COMPUTER_USE_MISSING_CAP,
            ComputerUseMacroError::MalformedCap { .. } => crate::diag::COMPUTER_USE_MALFORMED_CAP,
            ComputerUseMacroError::MalformedDimension { .. } => {
                crate::diag::COMPUTER_USE_MALFORMED_DIMENSION
            }
            ComputerUseMacroError::NotAnAgent { .. } => crate::diag::COMPUTER_USE_NOT_AN_AGENT,
        }
    }
}

impl std::fmt::Display for ComputerUseMacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComputerUseMacroError::MissingCap => write!(
                f,
                "MT6017: `@computer_use(...)` requires a `cap:` argument (e.g. `cap: computer.screen + computer.input`)"
            ),
            ComputerUseMacroError::MalformedCap { got } => write!(
                f,
                "MT6018: `@computer_use` cap must be a dotted path or sum of dotted paths (got `{got}`)"
            ),
            ComputerUseMacroError::MalformedDimension { which, got } => write!(
                f,
                "MT6019: `@computer_use` {which} must be a positive integer literal (got `{got}`)"
            ),
            ComputerUseMacroError::NotAnAgent { item_kind } => write!(
                f,
                "MT6020: `@computer_use` only decorates `agent` items, not {item_kind}"
            ),
        }
    }
}

/// Parse the `@computer_use(...)` attribute arguments. The args slice
/// is the comma-separated raw source pieces, exactly as the HIR
/// preprocessor produces them for a `MACRO_CALL`.
///
/// Accepted shapes:
///
/// - `@computer_use(width: 1280, height: 800, cap: computer.screen + computer.input)`
/// - `@computer_use(cap: computer.screen)` — defaults `width: 1024,
///   height: 768`
/// - `@computer_use(width: 1280, height: 800, cap: computer.input,
///   model: "claude-opus-4-7")`
pub fn parse_computer_use_attribute_args(
    args: &[&str],
) -> Result<ComputerUseAttributeArgs, ComputerUseMacroError> {
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut capability: Option<String> = None;
    let mut model: Option<String> = None;
    for raw in args {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("width:") {
            width = Some(parse_dim_arg("width", rest.trim())?);
        } else if let Some(rest) = trimmed.strip_prefix("height:") {
            height = Some(parse_dim_arg("height", rest.trim())?);
        } else if let Some(rest) = trimmed.strip_prefix("cap:") {
            capability = Some(parse_cap_expr(rest.trim())?);
        } else if let Some(rest) = trimmed.strip_prefix("model:") {
            // Best-effort: accept string literals; anything else is
            // stored verbatim. Validation lives upstream (parse will
            // already have caught a non-literal token).
            model =
                Some(decode_string_literal(rest.trim()).unwrap_or_else(|| rest.trim().to_string()));
        } else if !trimmed.is_empty() {
            // Unknown positional arg — refuse rather than silently
            // accept. The set of named args is small enough that any
            // typo is interesting.
            return Err(ComputerUseMacroError::MalformedCap {
                got: trimmed.to_string(),
            });
        }
    }
    let capability = capability.ok_or(ComputerUseMacroError::MissingCap)?;
    Ok(ComputerUseAttributeArgs {
        width: width.unwrap_or(1024),
        height: height.unwrap_or(768),
        capability,
        model,
    })
}

fn parse_dim_arg(which: &'static str, raw: &str) -> Result<u32, ComputerUseMacroError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ComputerUseMacroError::MalformedDimension {
            which,
            got: raw.to_string(),
        });
    }
    // Accept plain integer literals only (no `_` separators, no `0x`
    // hex). The dimension surface is tiny — we want surprising
    // syntax to fail closed.
    let n: u32 = s
        .parse()
        .map_err(|_| ComputerUseMacroError::MalformedDimension {
            which,
            got: raw.to_string(),
        })?;
    if n == 0 {
        return Err(ComputerUseMacroError::MalformedDimension {
            which,
            got: raw.to_string(),
        });
    }
    Ok(n)
}

fn parse_cap_expr(raw: &str) -> Result<String, ComputerUseMacroError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ComputerUseMacroError::MissingCap);
    }
    // Capability expressions are either dotted paths
    // (`computer.screen`) or sums (`computer.screen + computer.input`).
    // Validate by ensuring every `+`-separated atom is a dotted path
    // of `[A-Za-z_][A-Za-z0-9_]*` segments.
    for atom in s.split('+').map(str::trim) {
        if atom.is_empty() {
            return Err(ComputerUseMacroError::MalformedCap {
                got: raw.to_string(),
            });
        }
        for segment in atom.split('.') {
            let mut chars = segment.chars();
            let first = chars
                .next()
                .ok_or_else(|| ComputerUseMacroError::MalformedCap {
                    got: raw.to_string(),
                })?;
            if !(first.is_ascii_alphabetic() || first == '_') {
                return Err(ComputerUseMacroError::MalformedCap {
                    got: raw.to_string(),
                });
            }
            if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(ComputerUseMacroError::MalformedCap {
                    got: raw.to_string(),
                });
            }
        }
    }
    // Re-canonicalise: trim around the `+`s so `a + b` and `a+b`
    // hash to the same string.
    let canon = s.split('+').map(str::trim).collect::<Vec<_>>().join("+");
    Ok(canon)
}

/// JSON for the dispatcher spec — minimal hand-rolled emitter
/// (mirrors [`tool::render_descriptor_json`](super::tool::render_descriptor_json)
/// so the macro crate stays serde-free).
pub fn render_spec_json(args: &ComputerUseAttributeArgs, agent_name: &str) -> String {
    let model_field = match &args.model {
        Some(m) => format!(",\"model\":\"{}\"", json_escape(m)),
        None => String::new(),
    };
    format!(
        "{{\"agent\":\"{agent}\",\"width\":{w},\"height\":{h},\"capability\":\"{cap}\"{model}}}",
        agent = json_escape(agent_name),
        w = args.width,
        h = args.height,
        cap = json_escape(&args.capability),
        model = model_field,
    )
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Run the `@computer_use` macro on a parsed agent + attribute args.
pub fn expand_computer_use_attribute(
    attr_args: &[&str],
    agent: &ParsedAgent,
) -> Result<ComputerUseExpansion, ComputerUseMacroError> {
    if !agent.is_agent {
        return Err(ComputerUseMacroError::NotAnAgent {
            item_kind: "non-agent item".to_string(),
        });
    }
    let parsed = parse_computer_use_attribute_args(attr_args)?;
    let spec_json = render_spec_json(&parsed, &agent.name);
    let spec_fn = synthesise_spec_fn(&agent.name, &spec_json);
    Ok(ComputerUseExpansion {
        original_decl: agent.source.clone(),
        synthesised_decls: vec![spec_fn],
        spec_json,
    })
}

fn synthesise_spec_fn(agent_name: &str, spec_json: &str) -> String {
    // The synth fn mirrors the @tool macro's `__tool_descriptor_*`
    // shape — body-less Mighty stub that holds the spec text as a
    // literal. Runtime side reads the JSON via the host shim in
    // mty_stdlib::computer::dispatcher::from_macro_args.
    let escaped = escape_for_mty_string_literal(spec_json);
    format!("fn __computer_use_spec_{agent_name}() -> Str {{\n    \"{escaped}\"\n}}\n")
}

fn escape_for_mty_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_agent() -> ParsedAgent {
        ParsedAgent {
            name: "BrowserOperator".into(),
            source: "agent BrowserOperator { on Run(task: Str) -> Str { task } }".into(),
            is_agent: true,
        }
    }

    #[test]
    fn missing_cap_errors() {
        let err = parse_computer_use_attribute_args(&["width: 1280", "height: 800"]).unwrap_err();
        assert!(matches!(err, ComputerUseMacroError::MissingCap));
    }

    #[test]
    fn happy_path_parses_full_attr() {
        let args = parse_computer_use_attribute_args(&[
            "width: 1280",
            "height: 800",
            "cap: computer.screen + computer.input",
        ])
        .unwrap();
        assert_eq!(args.width, 1280);
        assert_eq!(args.height, 800);
        assert_eq!(args.capability, "computer.screen+computer.input");
        assert!(args.model.is_none());
    }

    #[test]
    fn defaults_apply_when_dimensions_missing() {
        let args = parse_computer_use_attribute_args(&["cap: computer.screen"]).unwrap();
        assert_eq!(args.width, 1024);
        assert_eq!(args.height, 768);
        assert_eq!(args.capability, "computer.screen");
    }

    #[test]
    fn model_arg_parsed_as_string_literal() {
        let args = parse_computer_use_attribute_args(&[
            "cap: computer.screen",
            "model: \"claude-opus-4-7\"",
        ])
        .unwrap();
        assert_eq!(args.model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn zero_dimension_errors() {
        let err =
            parse_computer_use_attribute_args(&["width: 0", "cap: computer.screen"]).unwrap_err();
        assert!(matches!(
            err,
            ComputerUseMacroError::MalformedDimension { which: "width", .. }
        ));
    }

    #[test]
    fn non_integer_dimension_errors() {
        let err = parse_computer_use_attribute_args(&[
            "width: twelve",
            "height: 800",
            "cap: computer.screen",
        ])
        .unwrap_err();
        assert!(matches!(
            err,
            ComputerUseMacroError::MalformedDimension { which: "width", .. }
        ));
    }

    #[test]
    fn malformed_cap_errors() {
        let err = parse_computer_use_attribute_args(&["cap: 123bad"]).unwrap_err();
        assert!(matches!(err, ComputerUseMacroError::MalformedCap { .. }));
    }

    #[test]
    fn cap_with_spaces_around_plus_normalises() {
        let args =
            parse_computer_use_attribute_args(&["cap: computer.screen   +   computer.input"])
                .unwrap();
        assert_eq!(args.capability, "computer.screen+computer.input");
    }

    #[test]
    fn render_spec_json_carries_all_fields() {
        let args = ComputerUseAttributeArgs {
            width: 1280,
            height: 800,
            capability: "computer.screen+computer.input".to_string(),
            model: Some("claude-opus-4-7".into()),
        };
        let s = render_spec_json(&args, "BrowserOperator");
        assert!(s.contains("\"agent\":\"BrowserOperator\""), "{s}");
        assert!(s.contains("\"width\":1280"), "{s}");
        assert!(s.contains("\"height\":800"), "{s}");
        assert!(
            s.contains("\"capability\":\"computer.screen+computer.input\""),
            "{s}"
        );
        assert!(s.contains("\"model\":\"claude-opus-4-7\""), "{s}");
    }

    #[test]
    fn render_spec_json_omits_model_when_none() {
        let args = ComputerUseAttributeArgs {
            width: 1024,
            height: 768,
            capability: "computer.screen".into(),
            model: None,
        };
        let s = render_spec_json(&args, "X");
        assert!(!s.contains("\"model\""), "{s}");
    }

    #[test]
    fn expand_succeeds_for_simple_agent() {
        let exp = expand_computer_use_attribute(
            &[
                "width: 1280",
                "height: 800",
                "cap: computer.screen + computer.input",
            ],
            &simple_agent(),
        )
        .unwrap();
        assert!(exp.spec_json.contains("\"agent\":\"BrowserOperator\""));
        assert!(exp.spec_json.contains("\"width\":1280"));
        assert_eq!(exp.synthesised_decls.len(), 1);
        assert!(exp.synthesised_decls[0].contains("__computer_use_spec_BrowserOperator"));
        assert_eq!(exp.original_decl, simple_agent().source);
    }

    #[test]
    fn expand_rejects_non_agent() {
        let mut not_agent = simple_agent();
        not_agent.is_agent = false;
        let err = expand_computer_use_attribute(&["cap: computer.screen"], &not_agent).unwrap_err();
        assert!(matches!(err, ComputerUseMacroError::NotAnAgent { .. }));
    }

    #[test]
    fn expand_rejects_missing_cap() {
        let err = expand_computer_use_attribute(&["width: 1280"], &simple_agent()).unwrap_err();
        assert!(matches!(err, ComputerUseMacroError::MissingCap));
    }
}
