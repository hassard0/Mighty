# 13 unresolved_value

Positive-fire for **MT2021 UNRESOLVED_VALUE**. Spec v1.0-RC §5 (names/scopes).

Agent and supervisor scopes are STRICT (A65): unknown value names are
hard errors rather than getting a fresh inference var. The `Ping`
handler references `undefined_name`, so the type checker reports
MT2021.
