# native_abi/04_callback

Pins the function-pointer ABI: a Mighty fn that accepts a `fn(I32)
-> I32` parameter MUST be callable from C with a `int32_t (*)
(int32_t)` callback. The callback is invoked on the C stack frame
with C calling convention.

Spec §29.4 (callback ABI).
