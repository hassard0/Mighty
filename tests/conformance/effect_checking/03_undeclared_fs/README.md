# 03 undeclared_fs

`pub fn load` uses the `Fs` capability but omits `effect fs`. Spec §9 — MT4001.
