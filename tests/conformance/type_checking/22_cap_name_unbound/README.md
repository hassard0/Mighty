# 22 cap_name_unbound

Positive-fire for **MT4060 CAP_NAME_UNBOUND**. Spec §8 (capabilities).

`Fs` is a capability family — its values are passed in as fn parameters.
Using the bare name `Fs` as a receiver is unbound. The v0.21 cap-name
resolver flags this without falling back to the slice-3 fresh-var path.
