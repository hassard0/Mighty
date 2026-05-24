# 04 two_children

Supervisor with two children + per-child `on_fail` clauses
(`restart up_to N in DUR` vs `backoff lo..hi; restart`).  Locks in
that the per-child policy grammar parses + lowers. Spec §15.
