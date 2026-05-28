// Tree-sitter semantic tokens provider — STUB for v0.32.
//
// What this file is
// -----------------
//
// v0.31 Track 1 shipped a tree-sitter grammar for Mighty at
// `tools/tree-sitter/`. The TextMate grammar already in this
// extension covers the common case (~90%) — keywords, decorators,
// type paths — but tree-sitter's semantic analysis can do things
// TextMate can't:
//
//   * Distinguish the soft keyword `agent` (used at the start of an
//     item) from a value-position identifier also called `agent`.
//   * Pick out `Tainted[T]` as a distinct type token from `Vec[T]`,
//     so editor themes can colour it as a security-relevant marker.
//   * Resolve `swarm`/`budget`/`effect` only in their statement
//     positions.
//
// We register an *empty* semantic-tokens provider here as a v0.32
// placeholder so that:
//
//   1. Theme files (`editor.tokenColorCustomizations`) can already
//      reference our `taintedType` token type and `tainted` modifier
//      — they will simply be no-ops until the WASM binding lands.
//   2. The activation surface is wired up — when the WASM tree-sitter
//      binding ships in v0.33, the only change required is filling
//      in `analyseDocument()`.
//
// Why a stub
// ----------
//
// `web-tree-sitter` (the WASM binding) requires:
//   * Bundling a `.wasm` artifact (the compiled grammar) with the
//     extension and loading it via `Parser.Language.load(wasmPath)`.
//   * Loading `web-tree-sitter.wasm` (the runtime).
//   * A build step to compile the grammar.js → C → WASM via
//     `tree-sitter generate && tree-sitter build --wasm`.
//
// That toolchain is non-trivial to wire reliably across all the
// supported VS Code platforms (esp. Windows where tree-sitter-cli's
// emscripten dependency is fragile). The v0.31 Track 1 grammar is
// not yet shipping a pre-built `.wasm` either. Per the v0.32 mandate
// we therefore ship the integration scaffolding here and leave the
// actual parsing for v0.33.
//
// Public API
// ----------
//
// `registerSemanticTokens(context)` registers the (no-op) provider.
// `LEGEND` is the public token-type/modifier vocabulary, exported so
// the README and theme docs can reference it.

import * as vscode from "vscode";

/** Standard VS Code semantic-token types, plus our `taintedType` custom. */
export const TOKEN_TYPES: readonly string[] = [
  "namespace",
  "type",
  "class",
  "enum",
  "interface",
  "struct",
  "typeParameter",
  "parameter",
  "variable",
  "property",
  "enumMember",
  "event",
  "function",
  "method",
  "macro",
  "keyword",
  "modifier",
  "comment",
  "string",
  "number",
  "regexp",
  "operator",
  "decorator",
  // Custom — see README "Custom token types" section for the
  // editor.tokenColorCustomizations snippet that themes this.
  "taintedType",
];

/** Modifiers we intend to emit once parsing is wired. */
export const TOKEN_MODIFIERS: readonly string[] = [
  "declaration",
  "definition",
  "readonly",
  "static",
  "async",
  "deprecated",
  "modification",
  "documentation",
  "defaultLibrary",
  // Custom modifiers for Mighty-specific concepts.
  "soft", // soft keywords (`budget`, `swarm`, `agent`, …)
  "tainted", // anything carrying a Tainted[…] type
  "capability", // capability tokens (`!{net.http, fs.read}`)
];

export const LEGEND = new vscode.SemanticTokensLegend(
  [...TOKEN_TYPES],
  [...TOKEN_MODIFIERS],
);

/**
 * v0.32 stub provider. Returns an empty token set on every request —
 * the TextMate grammar continues to handle highlighting.
 *
 * Override `analyseDocument()` (or replace the body of
 * `provideDocumentSemanticTokens`) in v0.33 once the WASM binding is
 * in place.
 */
class MightyTreeSitterTokensProvider
  implements vscode.DocumentSemanticTokensProvider
{
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeSemanticTokens: vscode.Event<void> = this.emitter.event;

  provideDocumentSemanticTokens(
    _document: vscode.TextDocument,
    _token: vscode.CancellationToken,
  ): vscode.SemanticTokens {
    // Future: const tree = parser.parse(document.getText());
    //         walk(tree.rootNode, builder);
    const builder = new vscode.SemanticTokensBuilder(LEGEND);
    return builder.build();
  }

  /** Hook for the v0.33 implementation. */
  refresh(): void {
    this.emitter.fire();
  }
}

/**
 * Activation entrypoint. Registers the (stub) provider. The selector
 * matches the TextMate grammar — if the LSP also exposes semantic
 * tokens (it does, when `mighty.semanticTokens.enable` is true) VS
 * Code uses the highest-priority provider; since this one returns no
 * tokens, the LSP's output wins. That's intentional: we want this
 * registration to be a no-op until v0.33 fills it in.
 */
export function registerSemanticTokens(
  context: vscode.ExtensionContext,
): void {
  const provider = new MightyTreeSitterTokensProvider();
  context.subscriptions.push(
    vscode.languages.registerDocumentSemanticTokensProvider(
      [
        { scheme: "file", language: "mighty" },
        { scheme: "untitled", language: "mighty" },
      ],
      provider,
      LEGEND,
    ),
  );
}

/**
 * What the v0.33 implementation needs to do, captured here so the
 * follow-up commit has a checklist:
 *
 *   1. Add `web-tree-sitter` as a dependency in package.json.
 *   2. Build the grammar:
 *        cd tools/tree-sitter
 *        npx tree-sitter generate
 *        npx tree-sitter build --wasm
 *      …producing `tree-sitter-mighty.wasm`. Ship it under
 *      `tools/vscode/resources/tree-sitter-mighty.wasm` and add the
 *      copy step to `npm run compile`.
 *   3. Also vendor `tree-sitter.wasm` (the runtime) from the
 *      web-tree-sitter package under the same `resources/` dir.
 *   4. In `activate()` (extension.ts), `await TreeSitter.init({
 *        locateFile: (file) => path.join(context.extensionPath,
 *          "resources", file)
 *      })`, then load the grammar via
 *      `await TreeSitter.Language.load(...)`.
 *   5. Implement `analyseDocument` here:
 *        - Parse the document.
 *        - Walk the tree, mapping node kinds to (token type, modifier)
 *          pairs using the queries shipped in
 *          tools/tree-sitter/queries/highlights.scm.
 *        - Emit them via SemanticTokensBuilder.push().
 *      Use `queries.captures(tree.rootNode)` for ergonomic
 *      tree-walking — every capture name in highlights.scm becomes a
 *      (start, end, name) triple we can translate directly.
 *   6. Re-fire `onDidChangeSemanticTokens` on document change, using
 *      `vscode.workspace.onDidChangeTextDocument`. Tree-sitter
 *      supports incremental edits via `tree.edit()` + `parser.parse(
 *      newText, oldTree)` — that's the right call for files >1k LOC.
 *
 * The token legend exported above is forward-compatible — it's the
 * same legend the v0.33 provider should ship, so themes written
 * against the v0.32 stub will continue to work.
 */
