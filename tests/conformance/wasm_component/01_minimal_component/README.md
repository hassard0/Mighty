# wasm_component/01_minimal_component

Pins the "valid component is the floor" invariant: even an empty
program MUST produce a component that validates. Implementations
that emit just a core module fail this case.

Spec §30.1 (component wrapper).
