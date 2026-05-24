# 04_cap_too_broad

v0.3 (A65) conformance: capability subsumption. The case shape models the
caller passing a too-broad cap to a callee declaring a narrower
constraint. Today both params resolve to `Cap{Fs, Any}` (no narrowing in
the function signatures), so the check returns clean. When slice-7 wires
function-signature-level cap narrowing, this case becomes a positive
MT4010 fire — update `expected_diagnostics.txt` then.

The MT4010 implementation itself is exercised by unit-test
`cap_subsumption_path_too_broad` in sdust-types.
