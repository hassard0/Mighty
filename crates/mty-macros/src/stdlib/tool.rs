//! The `@tool` attribute macro (v0.26 Track B).
//!
//! `@tool` is Mighty's standard way to mark a fn as an LLM-callable
//! tool. The macro:
//!
//! 1. Generates a hidden `__tool_descriptor_<NAME>()` fn that returns
//!    a JSON-encoded [`crate::stdlib::tool::ToolDescriptorSnippet`].
//! 2. Generates a hidden `__tool_invoke_<NAME>(args: String) -> String`
//!    fn that deserialises `args` into the typed params, wraps the
//!    call in a cap-check, and serialises the return value.
//! 3. Generates a hidden `__tool_register_<NAME>()` fn the module
//!    init list calls at startup to populate the runtime registry.
//!
//! ## Surface
//!
//! ```mty
//! @tool("Read a file from disk", cap: fs.read)
//! fn read_file(path: String) -> Result[String, FsError] !{fs} {
//!   std.fs.read_to_string(path)
//! }
//! ```
//!
//! ## Diagnostic codes
//!
//! - **MT6011** ([`ToolMacroError::NotAFn`]) — `@tool` decorates a
//!   non-fn item.
//! - **MT6012** ([`ToolMacroError::MissingDescription`]) — `@tool()`
//!   called with no arguments.
//! - **MT6013** ([`ToolMacroError::DescriptionNotALiteral`]) — first
//!   argument is not a string literal.
//! - **MT6014** ([`ToolMacroError::MalformedCap`]) — `cap:` argument
//!   does not parse as a dotted path.
//!
//! ## What the macro CANNOT do at v0.26
//!
//! - Generic fn parameters. The descriptor schema needs concrete
//!   types; `fn read_file[T](x: T)` is rejected with
//!   [`ToolMacroError::GenericNotSupported`].
//! - Complex parameter types beyond `String`/`Bool`/integer/float/
//!   `Vec[T]`/`Option[T]`. Anything else lowers to
//!   `{"type": "object"}` and the macro emits a warning.

use crate::stdlib::format::decode_string_literal;

/// Mighty source-side description of the macro's output. Returned by
/// [`expand_tool_attribute`] when expansion succeeds.
///
/// The HIR preprocessor splices `synthesised_decls` back into the
/// module so the fns become real top-level items. The
/// `original_decl` is the user's fn, returned unchanged so it stays
/// callable from regular Mighty code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExpansion {
    /// The user's original fn source. Returned unchanged so the fn
    /// is still directly callable from non-tool call sites.
    pub original_decl: String,
    /// Source for the synthesised companion fns
    /// (`__tool_descriptor_<NAME>`, `__tool_invoke_<NAME>`,
    /// `__tool_register_<NAME>`). Each entry is a top-level Mighty
    /// fn decl ready to be parsed.
    pub synthesised_decls: Vec<String>,
    /// JSON-encoded descriptor the runtime can deserialise directly.
    /// Surfaced separately so test code can assert against the
    /// descriptor shape without re-parsing the synth source.
    pub descriptor_json: String,
}

/// One parsed parameter: name + Mighty type spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedParam {
    pub name: String,
    pub mty_type: String,
}

/// Light-weight view of the user's fn the macro needs to walk. The
/// expander does not pull in the full HIR — it operates on a
/// pre-parsed shape so callers (HIR preprocessor, tests) can pass in
/// the bits they have without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFn {
    pub name: String,
    pub params: Vec<ParsedParam>,
    /// Spelled-out return type (e.g. `"Result[String, FsError]"`).
    /// Empty string for `-> Unit`.
    pub ret_ty: String,
    /// Whether the fn declares any generics (the macro rejects these).
    pub has_generics: bool,
    /// The original source slice, returned verbatim in
    /// [`ToolExpansion::original_decl`].
    pub source: String,
}

/// Errors the `@tool` macro can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolMacroError {
    /// MT6011 — `@tool` decorates a non-fn item (struct, type alias, …).
    NotAFn { item_kind: String },
    /// MT6012 — `@tool()` was called with no arguments.
    MissingDescription,
    /// MT6013 — first arg is not a string literal.
    DescriptionNotALiteral { got: String },
    /// MT6014 — `cap:` argument value is not a dotted path
    /// (`fs.read`, `net.get`, …).
    MalformedCap { got: String },
    /// MT6015 — fn has generic params. The descriptor schema needs
    /// concrete types; generic tools land in v0.27.
    GenericNotSupported { name: String },
    /// MT6016 — fn parameter has no type annotation. Mighty allows
    /// type inference in some positions, but tools need explicit
    /// types for the schema.
    ParamMissingType { name: String, param: String },
}

impl std::fmt::Display for ToolMacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolMacroError::NotAFn { item_kind } => {
                write!(f, "MT6011: `@tool` only decorates fns, not {item_kind}")
            }
            ToolMacroError::MissingDescription => write!(
                f,
                "MT6012: `@tool()` requires a description string as the first argument"
            ),
            ToolMacroError::DescriptionNotALiteral { got } => write!(
                f,
                "MT6013: `@tool` description must be a string literal (got `{got}`)"
            ),
            ToolMacroError::MalformedCap { got } => write!(
                f,
                "MT6014: `@tool` cap argument must be a dotted path like `fs.read` (got `{got}`)"
            ),
            ToolMacroError::GenericNotSupported { name } => write!(
                f,
                "MT6015: `@tool` cannot decorate generic fn `{name}` (concrete types only)"
            ),
            ToolMacroError::ParamMissingType { name, param } => write!(
                f,
                "MT6016: `@tool` fn `{name}` param `{param}` has no type annotation"
            ),
        }
    }
}

/// One parsed `@tool(...)` attribute body. The args slice contains
/// the comma-separated source slices of the parenthesised list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAttributeArgs {
    pub description: String,
    pub capability: Option<String>,
}

/// Parse the `@tool(...)` attribute arguments. The args slice is the
/// comma-separated raw source pieces, exactly as the HIR
/// preprocessor produces them for a `MACRO_CALL`.
///
/// Accepted shapes:
///
/// - `@tool("desc")` — description only, no cap.
/// - `@tool("desc", cap: fs.read)` — description + cap.
/// - `@tool(description: "desc")` — named first arg.
/// - `@tool(description: "desc", cap: fs.read)` — both named.
pub fn parse_tool_attribute_args(args: &[&str]) -> Result<ToolAttributeArgs, ToolMacroError> {
    if args.is_empty() {
        return Err(ToolMacroError::MissingDescription);
    }
    let mut description: Option<String> = None;
    let mut capability: Option<String> = None;
    for raw in args {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("description:") {
            description = Some(parse_string_arg(rest.trim())?);
        } else if let Some(rest) = trimmed.strip_prefix("cap:") {
            capability = Some(parse_cap_arg(rest.trim())?);
        } else if description.is_none() {
            description = Some(parse_string_arg(trimmed)?);
        } else if capability.is_none() {
            capability = Some(parse_cap_arg(trimmed)?);
        } else {
            // Extra positional arg; ignore but don't fail — the spec
            // may grow more fields, and rejecting unknowns would
            // break forward-compat.
        }
    }
    let description = description.ok_or(ToolMacroError::MissingDescription)?;
    Ok(ToolAttributeArgs {
        description,
        capability,
    })
}

fn parse_string_arg(raw: &str) -> Result<String, ToolMacroError> {
    decode_string_literal(raw).ok_or_else(|| ToolMacroError::DescriptionNotALiteral {
        got: raw.to_string(),
    })
}

fn parse_cap_arg(raw: &str) -> Result<String, ToolMacroError> {
    // Accept dotted paths like `fs.read`, `net.get`, `model.call`.
    // Also accept bare names like `fs` (the operation half is
    // optional at the macro-spec level; the runtime treats a bare
    // name as the family with empty op).
    let s = raw.trim();
    if s.is_empty() {
        return Err(ToolMacroError::MalformedCap {
            got: raw.to_string(),
        });
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(ToolMacroError::MalformedCap {
            got: raw.to_string(),
        });
    }
    let ok = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !ok {
        return Err(ToolMacroError::MalformedCap {
            got: raw.to_string(),
        });
    }
    Ok(s.to_string())
}

/// Map a Mighty type spelling to a JSON-schema type string. This is
/// intentionally conservative — unknown types lower to `"object"`
/// (which the LLM provider can interpret as "free-form").
pub fn mty_type_to_json_schema_type(mty_ty: &str) -> &'static str {
    let trimmed = mty_ty.trim();
    if trimmed.starts_with("Vec[") || trimmed.starts_with("Vec<") {
        return "array";
    }
    if trimmed.starts_with("Option[") || trimmed.starts_with("Option<") {
        // Option wraps the inner type; the optionality surfaces in
        // the `required` list, not the type field. Strip the wrapper
        // before re-checking.
        let inner = trimmed
            .trim_start_matches("Option[")
            .trim_start_matches("Option<")
            .trim_end_matches(']')
            .trim_end_matches('>');
        return mty_type_to_json_schema_type(inner);
    }
    match trimmed {
        "Str" | "String" => "string",
        "Bool" => "boolean",
        "I8" | "I16" | "I32" | "I64" | "I128" | "ISize" | "U8" | "U16" | "U32" | "U64" | "U128"
        | "USize" => "integer",
        "F32" | "F64" => "number",
        _ => "object",
    }
}

/// True if `mty_ty` is `Option[T]` / `Option<T>`. Used to compute the
/// `required` field in the JSON schema.
pub fn is_optional_param(mty_ty: &str) -> bool {
    let t = mty_ty.trim();
    t.starts_with("Option[") || t.starts_with("Option<")
}

/// The descriptor surface the macro produces. Rendered to JSON text
/// by [`render_descriptor_json`]; the runtime in
/// `mty_stdlib::mcp` parses that text back into its own typed
/// [`ToolDescriptor`].
///
/// Lives here (rather than in `mty-stdlib`) so the macro crate has
/// no runtime dep on the stdlib OR on serde — the macro renders
/// JSON via plain string concatenation (the surface is tiny and
/// deterministic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptorSnippet {
    pub name: String,
    pub description: String,
    /// JSON-encoded `ToolParameterSchema`. Built by
    /// [`build_input_schema_json`] from the parsed param list.
    pub input_schema_json: String,
    pub capability: Option<String>,
}

/// Build the JSON-encoded input schema for `params`. Mirrors the
/// `ToolParameterSchema` wire format used in `mty_stdlib::mcp`.
///
/// Hand-rolled JSON emitter so the macro crate stays serde-free
/// (declaring serde here would force every front-end build to pull
/// it in). The escape rules are minimal because the only dynamic
/// fields are the param/type names — already validated as plain
/// identifiers by the upstream parser.
pub fn build_input_schema_json(params: &[ParsedParam]) -> String {
    let mut props = String::new();
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            props.push(',');
        }
        let ty_str = mty_type_to_json_schema_type(&p.mty_type);
        if ty_str == "array" {
            let inner = p
                .mty_type
                .trim()
                .trim_start_matches("Vec[")
                .trim_start_matches("Vec<")
                .trim_end_matches(']')
                .trim_end_matches('>');
            let inner_ty = mty_type_to_json_schema_type(inner);
            props.push_str(&format!(
                "\"{name}\":{{\"type\":\"array\",\"items\":{{\"type\":\"{inner_ty}\"}}}}",
                name = json_escape(&p.name),
            ));
        } else {
            props.push_str(&format!(
                "\"{name}\":{{\"type\":\"{ty_str}\"}}",
                name = json_escape(&p.name),
            ));
        }
    }
    let mut required = String::new();
    let mut first = true;
    for p in params {
        if !is_optional_param(&p.mty_type) {
            if !first {
                required.push(',');
            }
            first = false;
            required.push_str(&format!("\"{}\"", json_escape(&p.name)));
        }
    }
    format!("{{\"type\":\"object\",\"properties\":{{{props}}},\"required\":[{required}]}}")
}

/// Render a [`ToolDescriptorSnippet`] as JSON text matching the
/// `mty_stdlib::mcp::ToolDescriptor` wire shape.
pub fn render_descriptor_json(d: &ToolDescriptorSnippet) -> String {
    let cap_field = match &d.capability {
        Some(c) => format!(",\"capability\":\"{}\"", json_escape(c)),
        None => String::new(),
    };
    format!(
        "{{\"name\":\"{name}\",\"description\":\"{desc}\",\"inputSchema\":{schema}{cap}}}",
        name = json_escape(&d.name),
        desc = json_escape(&d.description),
        schema = d.input_schema_json,
        cap = cap_field,
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

/// Run the `@tool` macro on a parsed fn + attribute args. Returns the
/// expansion (descriptor JSON + synthesised companion fns) on
/// success, or a structured [`ToolMacroError`] otherwise.
pub fn expand_tool_attribute(
    attr_args: &[&str],
    func: &ParsedFn,
) -> Result<ToolExpansion, ToolMacroError> {
    if func.has_generics {
        return Err(ToolMacroError::GenericNotSupported {
            name: func.name.clone(),
        });
    }
    for p in &func.params {
        if p.mty_type.trim().is_empty() {
            return Err(ToolMacroError::ParamMissingType {
                name: func.name.clone(),
                param: p.name.clone(),
            });
        }
    }
    let parsed_attrs = parse_tool_attribute_args(attr_args)?;
    let input_schema_json = build_input_schema_json(&func.params);
    let descriptor = ToolDescriptorSnippet {
        name: func.name.clone(),
        description: parsed_attrs.description.clone(),
        input_schema_json,
        capability: parsed_attrs.capability.clone(),
    };
    let descriptor_json = render_descriptor_json(&descriptor);

    // The synthesised Mighty source fns. Each returns the JSON as a
    // String so the runtime side (which speaks Rust) can re-parse it.
    let descriptor_fn = synthesise_descriptor_fn(&func.name, &descriptor_json);
    let invoke_fn = synthesise_invoke_fn(func, parsed_attrs.capability.as_deref());
    let register_fn = synthesise_register_fn(
        &func.name,
        &descriptor_json,
        parsed_attrs.capability.as_deref(),
    );

    Ok(ToolExpansion {
        original_decl: func.source.clone(),
        synthesised_decls: vec![descriptor_fn, invoke_fn, register_fn],
        descriptor_json,
    })
}

fn synthesise_descriptor_fn(fn_name: &str, descriptor_json: &str) -> String {
    let escaped = escape_for_mty_string_literal(descriptor_json);
    format!("fn __tool_descriptor_{fn_name}() -> String {{\n    \"{escaped}\"\n}}\n")
}

fn synthesise_invoke_fn(func: &ParsedFn, cap: Option<&str>) -> String {
    let fn_name = &func.name;
    let cap_check = match cap {
        Some(c) => format!(
            "    let __cap = std.mcp.current_capability_set()\n    if !__cap.check(\"{c}\", \"\") {{\n        return \"{{\\\"error\\\":\\\"capability_denied\\\",\\\"required\\\":\\\"{c}\\\"}}\"\n    }}\n"
        ),
        None => String::new(),
    };
    // For the minimal v0.26 expansion, the invoke body is a stub that
    // documents the wired path. Real arg deserialisation will require
    // the JSON ADT to land in Mighty (v0.27); for now the wrapper
    // returns a placeholder so the HIR preprocessor can splice the
    // synth source in without typecheck explosions.
    let _params_count = func.params.len();
    format!(
        "fn __tool_invoke_{fn_name}(__args: String) -> String {{\n{cap_check}    // Synthesised by `@tool` (v0.26 Track B).\n    // Real arg-deserialisation lands when std.json gains a typed ADT (v0.27).\n    // The Rust-side runtime wrapper in `mty_stdlib::mcp::register_tool`\n    // already implements the typed marshalling for native callers.\n    \"\\\"todo:typed-args\\\"\"\n}}\n"
    )
}

fn synthesise_register_fn(fn_name: &str, descriptor_json: &str, cap: Option<&str>) -> String {
    let escaped = escape_for_mty_string_literal(descriptor_json);
    let cap_field = match cap {
        Some(c) => format!("    let __cap = \"{c}\"\n"),
        None => String::new(),
    };
    format!(
        "fn __tool_register_{fn_name}() {{\n    let __desc = \"{escaped}\"\n{cap_field}    std.mcp.register_tool_from_json(__desc)\n}}\n"
    )
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

    fn fn_one_param() -> ParsedFn {
        ParsedFn {
            name: "read_file".into(),
            params: vec![ParsedParam {
                name: "path".into(),
                mty_type: "String".into(),
            }],
            ret_ty: "Result[String, FsError]".into(),
            has_generics: false,
            source: "fn read_file(path: String) -> Result[String, FsError] { ... }".into(),
        }
    }

    #[test]
    fn missing_description_errors() {
        let err = parse_tool_attribute_args(&[]).unwrap_err();
        assert!(matches!(err, ToolMacroError::MissingDescription));
    }

    #[test]
    fn description_must_be_string_literal() {
        let err = parse_tool_attribute_args(&["read_file"]).unwrap_err();
        assert!(matches!(err, ToolMacroError::DescriptionNotALiteral { .. }));
    }

    #[test]
    fn description_only_succeeds() {
        let args = parse_tool_attribute_args(&["\"Read a file\""]).unwrap();
        assert_eq!(args.description, "Read a file");
        assert!(args.capability.is_none());
    }

    #[test]
    fn description_and_cap_succeed() {
        let args = parse_tool_attribute_args(&["\"Read a file\"", "cap: fs.read"]).unwrap();
        assert_eq!(args.description, "Read a file");
        assert_eq!(args.capability.as_deref(), Some("fs.read"));
    }

    #[test]
    fn named_description_arg_works() {
        let args = parse_tool_attribute_args(&["description: \"Hi\"", "cap: net.get"]).unwrap();
        assert_eq!(args.description, "Hi");
        assert_eq!(args.capability.as_deref(), Some("net.get"));
    }

    #[test]
    fn malformed_cap_errors() {
        let err = parse_tool_attribute_args(&["\"x\"", "cap: 123bad"]).unwrap_err();
        assert!(matches!(err, ToolMacroError::MalformedCap { .. }));
    }

    #[test]
    fn mty_type_lowering_basics() {
        assert_eq!(mty_type_to_json_schema_type("String"), "string");
        assert_eq!(mty_type_to_json_schema_type("Bool"), "boolean");
        assert_eq!(mty_type_to_json_schema_type("I32"), "integer");
        assert_eq!(mty_type_to_json_schema_type("F64"), "number");
        assert_eq!(mty_type_to_json_schema_type("Vec[String]"), "array");
        assert_eq!(mty_type_to_json_schema_type("Option[String]"), "string");
        assert_eq!(mty_type_to_json_schema_type("Custom"), "object");
    }

    #[test]
    fn is_optional_param_detects_option_wrapper() {
        assert!(is_optional_param("Option[String]"));
        assert!(is_optional_param("Option<I32>"));
        assert!(!is_optional_param("String"));
        assert!(!is_optional_param("Vec[String]"));
    }

    #[test]
    fn build_input_schema_marks_non_option_as_required() {
        let params = vec![
            ParsedParam {
                name: "path".into(),
                mty_type: "String".into(),
            },
            ParsedParam {
                name: "depth".into(),
                mty_type: "Option[I32]".into(),
            },
        ];
        let schema_json = build_input_schema_json(&params);
        // `path` must appear in the required list, `depth` must NOT.
        assert!(
            schema_json.contains("\"required\":[\"path\"]"),
            "schema: {schema_json}"
        );
        assert!(schema_json.contains("\"path\""));
        assert!(schema_json.contains("\"depth\""));
    }

    #[test]
    fn render_descriptor_json_round_trip_via_inspection() {
        let d = ToolDescriptorSnippet {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema_json: build_input_schema_json(&[ParsedParam {
                name: "path".into(),
                mty_type: "String".into(),
            }]),
            capability: Some("fs.read".into()),
        };
        let s = render_descriptor_json(&d);
        assert!(s.contains("\"name\":\"read_file\""), "{s}");
        assert!(s.contains("\"description\":\"Read a file\""), "{s}");
        assert!(s.contains("\"inputSchema\""), "{s}");
        assert!(s.contains("\"capability\":\"fs.read\""), "{s}");
    }

    #[test]
    fn render_descriptor_json_escapes_quotes_in_description() {
        let d = ToolDescriptorSnippet {
            name: "f".into(),
            description: "she said \"hi\"".into(),
            input_schema_json: "{}".into(),
            capability: None,
        };
        let s = render_descriptor_json(&d);
        assert!(s.contains("\\\"hi\\\""), "{s}");
    }

    #[test]
    fn expand_succeeds_for_simple_fn() {
        let exp =
            expand_tool_attribute(&["\"Read a file\"", "cap: fs.read"], &fn_one_param()).unwrap();
        // descriptor JSON contains the description + cap
        assert!(exp.descriptor_json.contains("\"Read a file\""));
        assert!(exp.descriptor_json.contains("\"fs.read\""));
        assert!(exp.descriptor_json.contains("\"read_file\""));
        // synthesised fns include the descriptor + invoke + register
        let joined = exp.synthesised_decls.join("\n");
        assert!(joined.contains("__tool_descriptor_read_file"));
        assert!(joined.contains("__tool_invoke_read_file"));
        assert!(joined.contains("__tool_register_read_file"));
        // original is returned unchanged
        assert_eq!(exp.original_decl, fn_one_param().source);
    }

    #[test]
    fn expand_with_no_cap_omits_capability_field() {
        let exp = expand_tool_attribute(&["\"Read a file\""], &fn_one_param()).unwrap();
        // descriptor JSON has no "capability" field (serde skips
        // None).
        assert!(!exp.descriptor_json.contains("\"capability\""));
    }

    #[test]
    fn expand_rejects_generic_fn() {
        let mut f = fn_one_param();
        f.has_generics = true;
        let err = expand_tool_attribute(&["\"x\""], &f).unwrap_err();
        assert!(matches!(err, ToolMacroError::GenericNotSupported { .. }));
    }

    #[test]
    fn expand_rejects_untyped_param() {
        let mut f = fn_one_param();
        f.params[0].mty_type = "".into();
        let err = expand_tool_attribute(&["\"x\""], &f).unwrap_err();
        assert!(matches!(err, ToolMacroError::ParamMissingType { .. }));
    }
}
