# agent_protocol

Spec §13 + slice-5 design. Strict agent/protocol conformance:

- Every protocol-declared message must have an `on` handler in
  implementing agents.
- No `on Msg(...)` handler may reference a message no implemented
  protocol declares.
- Handler arity must match the protocol declaration.

## Cases

- `01_arity_mismatch` — MT4030 protocol_arity_mismatch.
- `02_missing_handler` — MT4032 protocol_missing_handler.
- `03_extra_handler` — MT4033 protocol_extra_handler.
- `04_protocol_ok` — positive: agent fully implements protocol; clean.
