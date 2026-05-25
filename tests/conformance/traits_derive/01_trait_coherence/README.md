# 01 trait_coherence

Positive-fire for **MT4022 TRAIT_COHERENCE_VIOLATION**. Spec v1.0-RC §19 (traits).

Two `impl Show for Foo` blocks each define `show`. The type checker
detects the duplicate implementation and reports MT4022.
