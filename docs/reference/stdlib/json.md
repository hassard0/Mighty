# `std.json`

Real JSON parser + emitter. Wraps `serde_json` but exposes a
Mighty-shaped `Json` enum.

## Surface

```sd
enum Json {
  Null,
  Bool(Bool),
  Num(F64),
  Str(String),
  Arr(Vec[Json]),
  Obj(Map[Str, Json]),
}

fn parse(s: Str) -> Json!JsonErr
fn encode(v: Json) -> String
fn encode_pretty(v: Json) -> String
```

## Semantics

- `parse` rejects malformed input with `JsonErr::Parse(message)`.
- `encode` emits compact (no-whitespace) form.
- `encode_pretty` emits 2-space-indented form.
- `Obj` is backed by a `BTreeMap` so encoded output is **sorted by
  key**. This is intentional: it makes JSON snapshots deterministic
  across runs and platforms.

## Example

```sd
use std.json

fn main() {
  let v = json.parse("{\"hello\":[1,2,3]}")?
  let out = json.encode_pretty(v)
  log(out)
}
```

## Known limits (v0.2)

- `Json::Num` is `f64`. Integers larger than 2^53 lose precision on
  round-trip. A `Json::Int(i64)` + `Json::Uint(u64)` split is planned
  for v0.3.

## See also

- [`std.test`](test.md) for deterministic JSON-snapshot testing.
- [STDLIB_V0_2_NOTES.md](https://github.com/hassard0/Mighty/blob/main/STDLIB_V0_2_NOTES.md) for the v0.3
  roadmap.
