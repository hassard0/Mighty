"""One-shot helper used during v1.0-RC spec consolidation.

Adds a `**Status:** ...` line under each `## Axx` amendment header in
`docs/spec/v0.1-amendments.md`. Idempotent: re-running replaces an
existing status line in place.
"""
import re
from pathlib import Path

classifications = {
    "A1":  ("FROZEN", "Decimal size-literal suffixes k/M shipped slice 2; lexer stable through v0.6."),
    "A2":  ("FROZEN", "Turbofish Path::[T,U] shipped slice 2; parser stable through v0.6."),
    "A3":  ("FROZEN", "Keyword-tolerant .method/.field shipped slice 2; widely used."),
    "A4":  ("FROZEN", "Keyword-tolerant effect names shipped slice 2."),
    "A5":  ("FROZEN", "run <expr> form shipped slice 2 (slice 3+ restricts to sandbox/budget)."),
    "A6":  ("FROZEN", "if let shipped slice 2; widely used."),
    "A7":  ("FROZEN", "Strict ? rule shipped slice 3 (MT2010/MT2011 stable)."),
    "A8":  ("SUPERSEDED", "Superseded by A19 (explicit defaulting pass in slice 4)."),
    "A9":  ("FROZEN", "Primitive names in both namespaces shipped slice 3; stable."),
    "A10": ("SUPERSEDED", "Permissive method table superseded by A17 (slice 4) + A65/A65.c (v0.3) strict scopes; opaque prelude still uses table residually."),
    "A11": ("OPEN", "Anonymous error unions still resolve to Result[T, Error] sentinel; first-class union ADTs deferred to v1.1+."),
    "A12": ("FROZEN", "Same-line postfix ?/! rule shipped slice 3; stable."),
    "A13": ("SUPERSEDED", "Hardcoded Copy set superseded by A26 #[derive(Copy)] in slice 5."),
    "A14": ("SUPERSEDED", "Conservative Sendable set superseded by A65.b full Sendable trait in v0.3."),
    "A15": ("OPEN", "Arena escape direct-naming MVP; indirect-flow detection still post-v1.0."),
    "A16": ("FROZEN", "MT2015 promoted to Error in slice 4; stable."),
    "A17": ("FROZEN", "Method dispatch policy shipped slice 4; trait coherence (A24) layers on top."),
    "A18": ("SUPERSEDED", "Warning-only check superseded by A28 (strict MT4030..MT4033) + A65.c (strict types)."),
    "A19": ("FROZEN", "Defaulting pass shipped slice 4; closes A8 deferral."),
    "A20": ("SUPERSEDED", "Lexical regions superseded by A55 NLL last-use deactivation in v0.3."),
    "A21": ("SUPERSEDED", "Unconditional tolerance superseded by A65 scope-aware policy in v0.3."),
    "A22": ("FROZEN", "Effect inference algorithm shipped slice 5; stable through v0.6."),
    "A23": ("FROZEN", "Capability narrowing constraints shipped slice 5; stable."),
    "A24": ("FROZEN", "Name-only trait coherence shipped slice 5; generic-arg overlap detection deferred to v1.1+."),
    "A25": ("FROZEN", "dyn Trait object safety shipped slice 5; conservative rules stable."),
    "A26": ("FROZEN", "Derive set shipped slice 5; supersedes A13."),
    "A27": ("SUPERSEDED", "Metadata-only sandbox parsing superseded by A43 (slice 7 runtime enforcement)."),
    "A28": ("FROZEN", "Strict protocol-handler checks shipped slice 5; supersedes A18."),
    "A29": ("SUPERSEDED", "Reserved-only MT3009 superseded by A56 precise emission in v0.3."),
    "A30": ("FROZEN", "Strict-profile alloc ban shipped slice 5; A65.d ratifies in v0.3 conformance."),
    "A31": ("OPEN", "Arena runtime enforcement still partial — MT5007 reserved; static MT3010 + bumpalo (A50) cover most cases."),
    "A32": ("SUPERSEDED", "Synchronous slice-6 dispatch superseded by async tokio-backed dispatch (slice 7) and A70 cancellation."),
    "A33": ("FROZEN", "Effect/Host trait dispatch shipped slice 6; stable."),
    "A34": ("SUPERSEDED", "Metadata-only budgets/sandboxes (slice 6) superseded by A43+A70+A99 runtime enforcement."),
    "A35": ("FROZEN", "Slice-6 deterministic interpreter shipped; A39 deterministic mode lifts to runtime."),
    "A36": ("SUPERSEDED", "Slice-7 in-memory http.serve shape superseded by A96 (real socket bind, agent dispatch)."),
    "A37": ("SUPERSEDED", "Approximate slice-7 memory budget superseded by A50 (bumpalo) + A99 (auto-charging) in v0.5."),
    "A38": ("SUPERSEDED", "OTLP-flavoured JSON superseded by A71 (real OTLP wire-format) in v0.3."),
    "A39": ("FROZEN", "Deterministic mode shipped slice 7; A106 keeps single-worker pin under deterministic(seed)."),
    "A40": ("FROZEN", "Mailbox depth/policy defaults shipped slice 7; A72 adds slab-pool optimisation underneath unchanged API."),
    "A41": ("SUPERSEDED", "Between-turn cancellation superseded by A70 cooperative mid-turn cancellation in v0.3."),
    "A42": ("FROZEN", "Restart window semantics shipped slice 7; stable."),
    "A43": ("FROZEN", "Top-level sandbox runtime execution shipped slice 7; supersedes A27."),
    "A44": ("FROZEN", "Deref-of-ref write path shipped slice 7; stable."),
    "A45": ("OPEN", "--legacy-interp flag retained; deprecation/removal deferred to v1.1+ once codegen covers full MtyIR surface."),
    "A46": ("FROZEN", "Cranelift default backend + LLVM scaffold shipped slice 8; LLVM activation still gated on build host."),
    "A47": ("OPEN", "Wasm Component Model wrapper deferred; core modules ship today. A97 (mighty:web/dom interface) layered on top; full wit-component still v1.1+."),
    "A48": ("FROZEN", "mty run JIT-then-fallback shipped slice 8; stable."),
    "A49": ("OPEN", "Per-(fn, type-args) monomorphisation strips generics in v0.1 MVP; full specialisation in v1.1+."),
    "A50": ("FROZEN", "bumpalo-backed arenas with byte-charging shipped slice 8; supersedes A37."),
    "A51": ("FROZEN", "MT8001..MT8010 codegen trap codes reserved + emitted slice 8; stable."),
    "A52": ("FROZEN", "Native linker discovery order shipped slice 8; stable."),
    "A53": ("FROZEN", "libloading-resolved externs shipped slice 8; stable."),
    "A54": ("FROZEN", "Place-algebra borrow tracking shipped v0.3; depth-1 truncation noted as future work but contract stable."),
    "A55": ("FROZEN", "NLL last-use deactivation shipped v0.3; supersedes A20."),
    "A56": ("FROZEN", "Precise MT3009 emission shipped v0.3; supersedes A29."),
    "A70": ("FROZEN", "Cooperative mid-turn cancellation shipped v0.3; supersedes A41."),
    "A71": ("FROZEN", "Real OTLP wire-format shipped v0.3; supersedes A38."),
    "A72": ("FROZEN", "Slab-pool mailbox frames shipped v0.3; performance optimisation under A40 API."),
    "A73": ("FROZEN", "Batched deadline scheduler shipped v0.3; stable internal infrastructure."),
    "A65": ("FROZEN", "Scope-aware permissive/strict policy shipped v0.3; supersedes A21."),
    "A65.b": ("FROZEN", "Sendable trait shipped v0.3; supersedes A14."),
    "A65.c": ("FROZEN", "MT4031 strict handler param-type check (local protocols) shipped v0.3; supersedes A18 fully."),
    "A65.d": ("FROZEN", "core profile rejects alloc ratified v0.3; reinforces A30."),
    "A74": ("FROZEN", "LSP v0.5 capability expansion shipped v0.5; single-file scope frozen for v1.0, cross-file rename deferred to v1.1+."),
    "A80": ("FROZEN", "break/continue as real HIR nodes shipped v0.5; labelled break deferred to v1.1+."),
    "A81": ("FROZEN", "__sdust_iter_next wire protocol shipped v0.5; full trait-based iterators deferred to v1.1+."),
    "A82": ("FROZEN", "Loop back-edge fixed-point shipped v0.5; stable."),
    "A90": ("FROZEN", "name!(args) marker shipped v0.5; v0.4 plain-call form retained for back-compat (v1.1+ may deprecate)."),
    "A91": ("FROZEN", "MT6001 unknown_macro activated v0.5; stable."),
    "A92": ("FROZEN", "Extended hygiene mangling shipped v0.5; set-of-scopes hygiene deferred to v1.1+."),
    "A93": ("FROZEN", "Cross-file pub macro surface shipped v0.5; end-to-end use wiring through mty-pkg deferred to v1.1+."),
    "A94": ("OPEN", "Procedural macros parse + store + purity check (MT6005/MT6006); sandboxed execution deferred to v1.1+, MT6006 gates every call site today."),
    "A95": ("FROZEN", "Standard macro library shipped v0.5; auto-import wiring through mty-pkg deferred to v1.1+."),
    "A96": ("FROZEN", "std.http.serve real socket bind shipped v0.5; per-agent dispatch wiring still default echo (v1.1+ closes)."),
    "A97": ("OPEN", "mighty:web/dom interface added; canonical-ABI return-area bridge for option<string> / string returns still v1.1+."),
    "A98": ("FROZEN", "Str method table real impls shipped v0.5; stable."),
    "A99": ("FROZEN", "MemBudgetExceeded + auto-charging shipped v0.5; supersedes A37."),
    "A100": ("FROZEN", "FsCap process-wide default allowlist enforcement shipped v0.5; per-call materialisation from manifest still v1.1+ (A109 pins isolation contract)."),
    "A101": ("FROZEN", "v0.6 multi-worker scheduler shipped; stable."),
    "A102": ("OPEN", "Affinity hints: runtime API frozen, front-end syntax `agent X with affinity = ...` reserved-but-not-parsed pending v1.1+."),
    "A103": ("OPEN", "Lightweight migration shipped; lossless live migration deferred to v1.1+."),
    "A104": ("FROZEN", "Per-worker telemetry shipped v0.6; stable."),
    "A105": ("FROZEN", "Driver runtime separation shipped v0.6; stable internal."),
    "A106": ("FROZEN", "Default worker count = available_parallelism shipped v0.6; supersedes A39 default but preserves deterministic(seed) override."),
    "A107": ("FROZEN", "Central diag catalog for MT6001-MT6006 shipped v0.6; stable."),
    "A108": ("FROZEN", "BuiltinId::DomOp shipped v0.6; closes v0.5 deferral #6."),
    "A109": ("FROZEN", "Per-call FsCap isolation contract pinned v0.6; manifest-driven materialisation still v1.1+."),
}

LEGEND = """
## Status legend (added during v1.0-RC consolidation)

Each amendment below carries a `**Status:**` line classifying it for the
v1.0 release candidate:

- **FROZEN** - normative decision shipped + tested + stable for at least two slices; folded into v1.0-RC verbatim.
- **SUPERSEDED** - made obsolete by a later amendment (cross-referenced).
- **OPEN** - partial / experimental / has documented gaps; v1.0 ships it as-is with v1.1+ evolution path.
- **REVERTED** - tried and rolled back (none in the v0.1..v0.6 corpus).

Counts at v1.0-RC: 63 FROZEN, 15 SUPERSEDED, 10 OPEN, 0 REVERTED (88 total).
See `docs/spec/CHANGELOG.md` for the chronological log and
`docs/spec/v1.0-rc.md` for the consolidated normative spec.

"""

def main():
    path = Path(r"C:\Users\ihass\stardust\docs\spec\v0.1-amendments.md")
    content = path.read_text(encoding="utf-8")

    # Strip any existing Status block first (idempotent re-run).
    content = re.sub(
        r"\n\*\*Status:\*\* [^\n]+\n",
        "\n",
        content,
    )
    # Strip any prior legend.
    content = re.sub(
        r"\n## Status legend.*?(?=\n## A\d)",
        "\n",
        content,
        count=1,
        flags=re.DOTALL,
    )

    header_re = re.compile(r"^(## (A\d+(?:\.[a-z])?)\s+—\s+.*?)$", re.MULTILINE)

    def repl(m):
        full_header = m.group(1)
        name = m.group(2)
        if name not in classifications:
            return full_header + "\n\n**Status:** UNCLASSIFIED - needs review."
        status, note = classifications[name]
        return f"{full_header}\n\n**Status:** {status} - {note}"

    new_content = header_re.sub(repl, content)

    first_idx = new_content.find("## A1")
    new_content = new_content[:first_idx] + LEGEND + new_content[first_idx:]

    path.write_text(new_content, encoding="utf-8")

    froz = sum(1 for v in classifications.values() if v[0] == "FROZEN")
    sup = sum(1 for v in classifications.values() if v[0] == "SUPERSEDED")
    op = sum(1 for v in classifications.values() if v[0] == "OPEN")
    rev = sum(1 for v in classifications.values() if v[0] == "REVERTED")
    print(f"FROZEN: {froz}")
    print(f"SUPERSEDED: {sup}")
    print(f"OPEN: {op}")
    print(f"REVERTED: {rev}")
    print(f"TOTAL: {len(classifications)}")


if __name__ == "__main__":
    main()
