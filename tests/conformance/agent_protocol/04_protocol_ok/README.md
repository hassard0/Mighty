# 04 protocol_ok

Positive case: protocol Echo declares `Ping`; agent Echoer
implements `on Ping` with matching arity. `mty check` must
succeed (exit 0). Spec §13.
