# formatter_idempotence/02_canonical_match

Pins match-expression canonical form: one arm per line, `=>` followed
by single space + body. The formatter MUST emit this shape and MUST
NOT re-flow arms across lines on subsequent `fmt` runs.

Spec §27.4 (formatter normative form).
