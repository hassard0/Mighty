# 03 wall_timeout

Synchronous ask with a `@1s` deadline. Per amendment A41, the
slice-7 interp only fires the deadline between turns; the same
shape that *would* exceed in slice-8 today simply returns "ok".
Marked `#[ignore]` in the harness; tracked for slice-8 enforcement
work. Spec §16.2 + amendment A41.
