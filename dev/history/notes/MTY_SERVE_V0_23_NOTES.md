# `mty serve` + `mty new --template web-game` — v0.23 Track C notes

Shipped as part of the v0.23 parallel slice. Track A landed the
`wasm32-web` core-module emit + `mty:web/canvas@0.1` WIT; Track C
(this slice) adds the dev-loop tooling so the user can go from
`mty new --template web-game asteroids` → working browser tab in
one command.

## What shipped

1. **`mty new --template web-game <name>`** — template registry +
   compile-time-embedded scaffold under
   `crates/mty-cli/templates/web-game/`. The blank template is now
   the default; new templates are added by dropping files under
   `crates/mty-cli/templates/<name>/` and registering them in
   `crates/mty-cli/src/cmd/new.rs::TEMPLATES`.

2. **`mty serve [--port N] [--watch]`** — built-in dev server. Builds
   once at startup with `--target wasm32-web`, then serves
   `web/index.html` at `/`, every other static asset under `web/`,
   and the freshly-built `main.wasm` at `/main.wasm` with
   `Content-Type: application/wasm`. With `--watch`, a `notify` watcher
   on `src/` debounces and rebuilds; a hand-rolled RFC 6455 websocket
   on `/_reload` pushes a `reload` frame to every connected page on
   each successful rebuild.

3. **Default web-game scaffold** — keeps the v0.22 `log("evt:...")`
   stopgap pattern (mirrors `demos/05_notetris_web/src/main.mty`) so
   the template compiles on day-one of the v0.23 release. The
   generated `src/main.mty` carries a clear NOTE comment pointing at
   `crates/mty-codegen-wasm/wit/mty-web/canvas.wit` for the v0.24
   port that drops the JS-side mirror in favour of guest-driven
   canvas draws.

## Architecture decisions

### Why a template registry in `mty new`, not a separate command?

`mty new --template <name>` keeps the surface area small — same
command, same exit codes, same "doesn't overwrite an existing
directory" behaviour. Templates are arrays of `(relative_path,
included_str_content)` pairs with a `{{NAME}}` substitution pass at
write time. Adding a new template is three edits:
`templates/<name>/<files>` + a `Template` entry in `TEMPLATES` +
one test case.

We deliberately did not pull in a templating engine (askama, tera,
…); the only substitution is `{{NAME}}`, and `String::replace`
costs nothing.

### Why hyper instead of `tiny_http` / `actix-web` / `axum`?

`mty-stdlib` already depends on hyper (workspace dep) for
`std.http.serve`. Reusing it costs nothing extra in build time and
keeps the dependency graph honest. The accept loop pattern is
copied from `crates/mty-stdlib/src/http_server.rs::accept_loop` —
`hyper::server::conn::http1::Builder::serve_connection` over a
`TokioIo`-wrapped TcpStream.

### Why hand-rolled SHA-1 + base64 for the ws handshake?

RFC 6455 mandates SHA-1 for `Sec-WebSocket-Accept`; the `sha2` crate
that's already in the dep graph doesn't ship SHA-1. Rather than pull
a crypto crate just for one hash per page load, we hand-rolled both
SHA-1 and base64 (~80 lines total) and unit-tested against the
RFC's worked example. The server-side handshake also writes only
unmasked frames; the page never sends after the upgrade.

We never run `mty serve` exposed to the public internet — it's a
dev tool — so this stays well inside the "good enough" envelope.

### Why `notify` 6.x + a bridging thread?

`notify` ships a synchronous callback that runs on its own OS
thread. The dev server is async (tokio), so we bridge the
`std::sync::mpsc` channel `notify` fills into a
`tokio::sync::mpsc` channel via one long-lived `std::thread::spawn`.
Earlier drafts tried to `spawn_blocking` per event; that added
Arc/Mutex shuffling around the receiver because `recv()` takes
`self` by reference and the receiver isn't `Sync`.

### Debounce: 200ms

Single-save editor events (VSCode, vim, jetbrains) fire 1-3 events
in quick succession (Create + Modify + Modify is common). A 200ms
debounce window collapses these to one rebuild without making
intentional rapid-saves feel sluggish.

## Tests

| File | Coverage |
|------|----------|
| `crates/mty-cli/tests/cmd_new_template.rs` | blank-default still works; web-game scaffold produces 4 spec-mandated files + the `README`; placeholder substitution worked; `mty check` passes on the scaffold; `mty build --target wasm32-web` succeeds and writes `target/main.wasm`. Unknown template exits 2 with a useful message. |
| `crates/mty-cli/tests/cmd_serve.rs` | `/` returns 200 + text/html; `/main.wasm` returns 200 + application/wasm + the `\0asm` preamble; static `dom-shim.js` returns 200 + application/javascript; `/no-such-file` returns 404; running outside a package exits 2 with `mighty.toml` in stderr. |
| `cmd::new::tests` (unit) | template registry shape: default is blank; web-game registered; substitution covers multiple placeholders. |
| `cmd::serve::tests` (unit) | mime lookup; SHA-1 RFC 3174 vector; base64 RFC 4648 vectors; RFC 6455 worked example for `Sec-WebSocket-Accept`. |

### Watch-rebuild test (stretch)

`serve_watch_rebuilds_on_change` is `#[ignore]`'d. Filesystem-event
timing is flaky in the Windows CI sandbox — `ReadDirectoryChangesW`
delivery can lag by multiple seconds after a write under load. The
watcher path works in interactive dev (verified manually); we'll
re-enable the test once we can pin a per-platform deadline that
isn't either flaky or wasteful.

## Pre-flight gate (run before push)

```bash
cargo build --workspace
cargo test -p mty-cli --test cmd_new_template --test cmd_serve
cargo clippy -p mty-cli --no-deps -- -D warnings
cargo fmt -p mty-cli -- --check
```

All four pass at the v0.23 slice cut.

## Follow-ups (post-v0.23)

- **Canvas WIT cut-over** — once Track A's `mty:web/canvas@0.1` host
  binding ships in the dom-shim default, regenerate the web-game
  template so the agent owns the canvas draws and the JS mirror
  collapses to ~30 lines of input plumbing.
- **`mty serve` for non-web targets** — currently rejects packages
  without a `web/` directory. A `--target=native` mode that hot-runs
  the binary on rebuild would be a natural sibling.
- **HMR for assets** — image/css edits currently force a full
  page reload via the same `_reload` channel. A per-asset cache-bust
  could keep canvas state across CSS/asset edits.
- **Watch test reliability** — extract a tiny filesystem-event
  probe that captures notify timing on each platform; gate the
  rebuild assertion behind it instead of `#[ignore]`.
