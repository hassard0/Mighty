# macros/06_proc_macro_resource_exceeded

MT6008 positive-fire (Gap F — v0.10 audit). The proc-macro `hog`
requests `repeat(input, 4_000_000)` which blows past the sandbox's
16 MiB memory cap (or, equivalently, its step cap — both are accepted
by the underlying executor, per `mty-macros/tests/proc_macro_exec_mem.rs`).
The HIR macros lowerer maps the `ResourceExceeded` variant onto
`MT6008 PROC_MACRO_RESOURCE_EXCEEDED`.

Promotes MT6008 from auxiliary (mty-macros unit tests) to direct
conformance_full coverage.

Spec ref: §20 compile-time metaprogramming, sandbox resource bounds.
