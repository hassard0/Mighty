# wasm_component/04_user_wit

Pins the user-WIT extension shape: when the implementation is given
`--wit world.wit`, the emitted component's world MUST be the
user-supplied one (here, `example:greeter`), and the Mighty
`_greet` fn MUST be wired as the `greet` export.

Implementations that ignore the `--wit` arg and emit only the
default `mty:main/run` world fail this case.

Spec §30.4 (user WIT worlds).
