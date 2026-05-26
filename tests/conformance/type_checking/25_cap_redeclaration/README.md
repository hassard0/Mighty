# 25 cap_redeclaration

Positive-fire for **MT4063 CAP_REDECLARATION**. Spec §8.

A function declares the same parameter name twice with capability
types. The cap-resolver pass detects the duplicate binding in the
fn's scope frame and emits MT4063 with the frame-depth hint.
