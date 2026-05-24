# mailbox_ordering

Spec §25.3 + slice-7 design. Agents process messages in FIFO order;
back-to-back `?Msg(...)` from the same caller observe a monotonic
view of the agent's state.

The conformance harness runs these through the slice-6 SIR
interpreter (deterministic by construction). The tokio runtime (used
by `sdust run` in production) preserves FIFO per agent via the
`tokio::sync::mpsc` mailbox.

## Cases

- `01_counter_fifo` — three `?Inc()` in sequence return 1, 2, 3.
- `02_accumulator` — sequential `?Add(n)` accumulates monotonically.
- `03_three_message_seq` — issuing `?Msg("a")`, `?Msg("b")`, `?Msg("c")` returns them concatenated in order.
