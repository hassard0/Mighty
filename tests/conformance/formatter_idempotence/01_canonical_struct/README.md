# formatter_idempotence/01_canonical_struct

A struct with two-space indent + one field per line + no trailing
blank lines is already in canonical form. `fmt(input.mty)` MUST
produce exactly `canonical.mty` and re-running `fmt` MUST be a no-op.

Note: `input.mty` and `canonical.mty` are identical in this seed
case. v1.0 follow-ups will add variants with redundant blank lines
and tab/space mixing to pin the normaliser more strictly; the
current v0.20 fixture pins the canonical-form invariant first.

Spec §27.4 (formatter normative form).
