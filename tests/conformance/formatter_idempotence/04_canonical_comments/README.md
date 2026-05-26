# formatter_idempotence/04_canonical_comments

Pins the comment-preservation contract: line comments stay attached
to their containing block, blank lines between comment groups
survive, inner-block comments retain their indent. The formatter
MUST NOT drop, reorder, or re-flow comments across paragraph
boundaries.

Spec §27.4.3 (comment preservation).
