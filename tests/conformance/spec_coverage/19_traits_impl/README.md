# §19 traits — trait + impl + method call

Canonical positive shape covering Spec v1.0-RC §19 (traits / dispatch):

- Trait declaration with a method signature.
- `impl Trait for Type` providing the method body.
- Method dispatched via dot-syntax (`t.show()`).

Passes type-check clean.
