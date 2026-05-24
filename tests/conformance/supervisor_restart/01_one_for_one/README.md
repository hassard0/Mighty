# 01 one_for_one

Supervisor with `strategy: one_for_one` and a single child. The
`on_fail` clause uses `restart up_to 3 in 30s`. Conformance proof:
the declaration parses, type-checks, lowers, and `main` runs
cleanly. Spec §15.
