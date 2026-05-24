# 05 match_arm_infer

`match` arm types must unify against the function's declared return.
The integer literals `1` and `2` infer to `I64` from the declared
return. Spec §6.4 + §6.6 (match exhaustiveness).
