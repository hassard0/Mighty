# 27 cap_constraint_invalid

Positive-fire for **MT4065 CAP_CONSTRAINT_INVALID**. Spec §8.

A narrowing constructor (`fs.path(...)`) is called without the
required string-literal argument. The cap-resolver pass validates
the constructor's argument shape against the family's accepted
constraints and emits MT4065 when the shape doesn't match.
