# 01 arity_mismatch

`protocol Greet { Hello(name: Str, age: I32) }` declares arity 2;
`agent Greeter` implements `on Hello(name)` with arity 1. SD4030.
Spec §13.
