# 24 cap_scope_violation

Positive-fire for **MT4062 CAP_SCOPE_VIOLATION**. Spec §8.

A capability bound as a parameter of `outer` is referenced from
`inner` where the binding is not in scope. The v0.21 cap-resolver
pass detects the cross-scope reference and emits MT4062 with the
popped-at-depth hint.
