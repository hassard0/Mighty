# spec_coverage/

Canonical positive-shape examples for spec v1.0-RC sections that
weren't already exercised by a category-specific negative test. Each
case is named `NN_label/` where `NN` matches the spec section number.

Per `docs/spec/conformance-coverage.md`, the broader negative-test
categories (`type_checking/`, `traits_derive/`, `agent_protocol/`,
`borrow_checking/`, etc.) already pin the rejection behaviour for
their respective sections. The cases here pin the *acceptance*
behaviour for the canonical positive constructs so subsequent slices
can't quietly regress them.
