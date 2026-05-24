//! Documentation intermediate representation.
//!
//! `DocPackage` is the in-memory shape produced by [`crate::extract`]
//! and consumed by the renderers in [`crate::render`]. It is
//! deliberately decoupled from the HIR so renderers don't need to
//! drag in the type checker.
//!
//! Stability: v0.2-internal. Field names may change in v0.3.

/// A whole package's documentation.
#[derive(Debug, Clone, Default)]
pub struct DocPackage {
    /// Package name. For a single-file run, defaults to the file stem
    /// when no `package <name>` declaration is present.
    pub name: String,
    /// Free-form version string (today: always "0.0.0" — populated
    /// from `Stardust.toml` in v0.3).
    pub version: String,
    /// Package-level synopsis sourced from leading `//!` comments in
    /// the file head, or empty if none.
    pub synopsis: String,
    /// Package-level body (CommonMark) — same source as `synopsis`,
    /// but the synopsis is just the first sentence; the body is
    /// everything.
    pub body: String,
    /// Modules declared via `mod foo` items. v0.2 only records the
    /// declared path — there is no cross-file module resolution yet.
    pub modules: Vec<DocModule>,
    /// All documented items at the top level (functions, structs,
    /// enums, traits, agents, protocols, supervisors, type aliases,
    /// constants).
    pub items: Vec<DocItem>,
}

/// A module entry. v0.2 only records the declared path.
#[derive(Debug, Clone)]
pub struct DocModule {
    pub path: Vec<String>,
    pub synopsis: String,
}

/// Item visibility as documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocVisibility {
    Public,
    Private,
}

/// Item flavour. Mirrors the HIR variants we care about for docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocItemKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Agent,
    Protocol,
    Supervisor,
    TypeAlias,
    Const,
}

impl DocItemKind {
    /// Human-readable section header used in Go-style stdout output.
    pub fn section_header(self) -> &'static str {
        match self {
            DocItemKind::Fn => "FUNCTIONS",
            DocItemKind::Struct => "TYPES",
            DocItemKind::Enum => "TYPES",
            DocItemKind::TypeAlias => "TYPES",
            DocItemKind::Trait => "TRAITS",
            DocItemKind::Agent => "AGENTS",
            DocItemKind::Protocol => "PROTOCOLS",
            DocItemKind::Supervisor => "SUPERVISORS",
            DocItemKind::Const => "CONSTANTS",
        }
    }

    /// Short tag used in search indices and the HTML sidebar.
    pub fn tag(self) -> &'static str {
        match self {
            DocItemKind::Fn => "fn",
            DocItemKind::Struct => "struct",
            DocItemKind::Enum => "enum",
            DocItemKind::Trait => "trait",
            DocItemKind::Agent => "agent",
            DocItemKind::Protocol => "protocol",
            DocItemKind::Supervisor => "supervisor",
            DocItemKind::TypeAlias => "type",
            DocItemKind::Const => "const",
        }
    }
}

/// Pre-rendered signature ready to print verbatim. We keep both the
/// raw form (for Go-style stdout) and a pre-linkified HTML form (for
/// the HTML renderer).
#[derive(Debug, Clone, Default)]
pub struct ItemSignature {
    /// Plain-text signature, e.g. `pub fn add(a: I32, b: I32) -> I32`.
    pub plain: String,
    /// HTML signature with `<a href="...">Type</a>` links injected
    /// around any name that appears in this package's symbol table.
    pub html: String,
}

/// A single documented item.
#[derive(Debug, Clone)]
pub struct DocItem {
    pub name: String,
    pub kind: DocItemKind,
    pub visibility: DocVisibility,
    pub signature: ItemSignature,
    /// First sentence of the doc comment, used in indexes and the
    /// section listing. Empty if no doc comment.
    pub synopsis: String,
    /// Remaining doc-comment body (CommonMark source).
    pub body: String,
    /// Extracted code blocks tagged `sd` or `stardust`.
    pub examples: Vec<DocExample>,
    /// Optional since-version (parsed from `# Since` heading).
    pub since: Option<String>,
    /// Backlinks: names of items in this package that call/use this
    /// item. Populated by [`crate::extract::compute_backlinks`].
    pub used_by: Vec<String>,
    /// Stable URL slug used by both markdown and HTML renderers
    /// (e.g. `fn.add`, `struct.User`). Stable across renderings.
    pub anchor: String,
}

/// One extracted example block.
#[derive(Debug, Clone)]
pub struct DocExample {
    pub code: String,
    pub language: String,
}
