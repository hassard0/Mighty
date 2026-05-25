# 01 unexpected_token

Positive-fire for **MT0001 UNEXPECTED_TOKEN**. Spec v1.0-RC §3-4
(lexical, items). The bare `)` at top level is not a valid item
start, so the parser reports MT0001.
