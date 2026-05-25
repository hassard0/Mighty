# 12 pub_param_needs_type

Positive-fire for **MT2020 PUB_PARAM_NEEDS_TYPE**. Spec v1.0-RC §10 (functions).

`pub fn` parameters must declare an explicit type so cross-module
callers can rely on the signature. `helper(x)` omits the type
annotation, so the type checker reports MT2020.
