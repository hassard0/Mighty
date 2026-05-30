#!/usr/bin/env python3
"""v0.41 T5 — prune unresolved docstub entries.

Reads the list of symbols to delete (one per line, lines beginning with `#`
ignored) and removes the matching `##sym ... ##end` blocks from every
`.docstub` file in `crates/mty-stdlib/docs/`.

Then mutates `crates/mty-doc/src/examples.rs` to drop the matching
`StdlibExample { ... },` entries from `STDLIB_EXAMPLES`.

Usage:
    python3 scripts/v041_t5_audit_prune.py path/to/delete_list.txt
"""
import re
import sys
from pathlib import Path


def main(delete_list_path: str, root: Path) -> int:
    syms = set()
    for line in Path(delete_list_path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        syms.add(line)
    print(f"loaded {len(syms)} symbols to delete")

    # 1) Prune docstubs.
    docs_dir = root / "crates" / "mty-stdlib" / "docs"
    total_removed = 0
    for f in sorted(docs_dir.glob("*.docstub")):
        body = f.read_text(encoding="utf-8")
        new_body, removed = prune_docstub(body, syms)
        if removed:
            f.write_text(new_body, encoding="utf-8")
            print(f"  {f.name}: removed {removed} entries")
            total_removed += removed
    print(f"docstub total: {total_removed}")

    # 2) Prune examples.rs.
    ex_path = root / "crates" / "mty-doc" / "src" / "examples.rs"
    ex_body = ex_path.read_text(encoding="utf-8")
    new_ex_body, ex_removed = prune_examples(ex_body, syms)
    if ex_removed:
        ex_path.write_text(new_ex_body, encoding="utf-8")
        print(f"examples.rs: removed {ex_removed} entries")
    else:
        print("examples.rs: nothing to remove")

    if total_removed != ex_removed:
        print(
            f"WARNING: docstub deletions ({total_removed}) "
            f"!= examples.rs deletions ({ex_removed})"
        )
    return 0


def prune_docstub(body: str, syms: set[str]) -> tuple[str, int]:
    """Walk the file. When we hit `##sym X` whose X is in `syms`, drop
    every line until (and including) the matching `##end`. Trailing
    blank line directly after the block is also consumed so we don't
    leave double-blanks."""
    out_lines: list[str] = []
    lines = body.split("\n")
    i = 0
    removed = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("##sym "):
            name = line[len("##sym "):].strip()
            if name in syms:
                # Skip until ##end inclusive.
                while i < len(lines) and not lines[i].startswith("##end"):
                    i += 1
                if i < len(lines):  # consume ##end
                    i += 1
                # Drop the trailing blank line if any (otherwise we get
                # double blanks).
                if i < len(lines) and lines[i].strip() == "":
                    i += 1
                removed += 1
                continue
        out_lines.append(line)
        i += 1
    return "\n".join(out_lines), removed


# Match a single `StdlibExample { ... },` block. We use a state machine
# rather than regex because the inner `example: "..."` field can contain
# escaped quotes that are awkward to anchor with `.*?`.
def prune_examples(body: str, syms: set[str]) -> tuple[str, int]:
    out: list[str] = []
    lines = body.split("\n")
    i = 0
    removed = 0
    while i < len(lines):
        line = lines[i]
        # Detect the start of an entry. Two shapes accepted:
        #     StdlibExample {
        # or
        #     "_" => StdlibExample {
        stripped = line.strip()
        if stripped == "StdlibExample {":
            # Find the matching closing `},` line. We track brace depth.
            block_start = i
            depth = 1
            j = i + 1
            symbol = None
            while j < len(lines) and depth > 0:
                # Capture the symbol field on the first `symbol: "X",` line.
                m = re.match(r'\s*symbol:\s*"([^"]+)"\s*,', lines[j])
                if m and symbol is None:
                    symbol = m.group(1)
                for ch in lines[j]:
                    if ch == "{":
                        depth += 1
                    elif ch == "}":
                        depth -= 1
                        if depth == 0:
                            break
                j += 1
            # When depth hits 0 we broke from the inner loop but still
            # ran `j += 1`, so `j` now points to the line AFTER the
            # closing brace. The actual last line of the block is j-1.
            block_end = j - 1
            if symbol is not None and symbol in syms:
                # Drop the block.
                # Also drop a single trailing blank line if present.
                next_i = block_end + 1
                if next_i < len(lines) and lines[next_i].strip() == "":
                    next_i += 1
                # Also drop a leading section-comment line if it
                # applied only to this entry. (Conservative: leave
                # comments alone.)
                removed += 1
                i = next_i
                continue
            else:
                # Keep all lines.
                out.extend(lines[block_start : block_end + 1])
                i = block_end + 1
                continue
        out.append(line)
        i += 1
    return "\n".join(out), removed


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("usage: v041_t5_audit_prune.py <delete-list>")
        sys.exit(2)
    root = Path(__file__).resolve().parent.parent
    sys.exit(main(sys.argv[1], root))
