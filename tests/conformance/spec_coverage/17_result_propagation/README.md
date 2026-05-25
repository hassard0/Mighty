# §17 errors — Result + `?` propagation

Canonical positive shape covering Spec v1.0-RC §17 (error handling):

- User error enum `ParseErr`.
- `Result`-returning function via `T!E` sugar.
- `?` propagation lifts the inner error to the enclosing fn's
  Result-Err type when they match exactly (v0.3 strict per A65).

Passes type-check clean.
