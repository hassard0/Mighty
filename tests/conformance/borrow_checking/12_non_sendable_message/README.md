# 12 non_sendable_message

Positive-fire for **MT3011 NON_SENDABLE_MESSAGE_ARG**. Spec v1.0-RC §12 (agents).

Agent send/ask requires every payload argument to be Sendable. A
reference `&I32` is not Sendable (it carries a non-shared lifetime),
so passing `&x` across an agent boundary is rejected with MT3011.
