# 01 undeclared_net

`pub fn ping` performs a `net.get` but does not declare `!{net}` in
its signature. Public fns must declare every effect they transitively
perform. Spec §9 — SD4001.
