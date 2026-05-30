#!/usr/bin/env python3
"""v0.41 T5 — mark `# concept-doc` entries.

Reads a list of symbols (one per line, `#`-comments ignored) and inserts
`# concept-doc` on the blank line directly before each `##sym <name>` in
the matching `.docstub` file.

This is idempotent — running it twice does not duplicate the marker.

Usage:
    python3 scripts/v041_t5_audit_mark_concept.py path/to/concept_syms.txt
"""
import sys
from pathlib import Path


def main(syms_path: str, root: Path) -> int:
    syms = set()
    for line in Path(syms_path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        syms.add(line)
    print(f"loaded {len(syms)} concept-doc symbols")

    docs_dir = root / "crates" / "mty-stdlib" / "docs"
    total_marked = 0
    for f in sorted(docs_dir.glob("*.docstub")):
        body = f.read_text(encoding="utf-8")
        new_body, marked = mark_concepts(body, syms)
        if marked:
            f.write_text(new_body, encoding="utf-8")
            print(f"  {f.name}: marked {marked} entries")
            total_marked += marked
    print(f"total: {total_marked}")
    return 0


def mark_concepts(body: str, syms: set[str]) -> tuple[str, int]:
    out: list[str] = []
    lines = body.split("\n")
    marked = 0
    for i, line in enumerate(lines):
        if line.startswith("##sym "):
            name = line[len("##sym "):].strip()
            if name in syms:
                # Is the previous line already the marker?
                prev = out[-1] if out else ""
                if prev.strip() != "# concept-doc":
                    # Insert marker. We want it on its own line, BEFORE
                    # the blank line that separates entries — actually,
                    # the audit looks for the marker on a line followed
                    # (possibly with blanks) by `##sym`, so position
                    # doesn't strictly matter. We put it immediately
                    # before the `##sym` line for readability.
                    out.append("# concept-doc")
                    marked += 1
        out.append(line)
    return "\n".join(out), marked


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("usage: v041_t5_audit_mark_concept.py <syms-file>")
        sys.exit(2)
    root = Path(__file__).resolve().parent.parent
    sys.exit(main(sys.argv[1], root))
