# 23 cap_family_mismatch

Positive-fire for **MT4061 CAP_FAMILY_MISMATCH**. Spec §8.

When the same identifier is bound to two distinct capability families
across the package (`Fs` and `Net`), the cap-resolver surfaces the
cross-fn collision so the user can pick a single family or rename one
of the bindings.
