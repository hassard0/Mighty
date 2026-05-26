# deterministic_replay/04_send_message

Pins the agent-protocol replay events: every external `send` of a
protocol message MUST produce a `MessageSent` trace event, and the
agent's handler invocation MUST produce a matching `MessageHandled`
trace event. The `Spawn` event MUST precede both.

This is the core of the cross-agent determinism story: a recorded
trace can be re-played and the agent observes the same message
payload sequence even if the host's scheduling changes between runs.

Spec §28.4 (cross-agent determinism).
