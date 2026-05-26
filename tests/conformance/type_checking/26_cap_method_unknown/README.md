# 26 cap_method_unknown

Positive-fire for **MT4064 CAP_METHOD_UNKNOWN**. Spec §8.

A method that's not in the family's built-in surface. The cap-
resolver pass enumerates the available methods for each family
(`Fs`, `Net`, `Clock`, `Dom`, `Model`) and emits MT4064 with the
list as a note when the call doesn't match.
