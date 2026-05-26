# `mty serve --watch` deterministic test — v0.24 Track C notes

Shipped as part of the v0.24 parallel slice. Track C owns the dev-loop
plumbing started in v0.23; this slice de-flakes the watcher
integration test so we can drop the `#[ignore]` annotation that gated
the v0.23 acceptance.

## Problem

v0.23 left
`crates/mty-cli/tests/cmd_serve.rs::serve_watch_rebuilds_on_change`
`#[ignore]`'d:

> Filesystem-event timing is flaky in the Windows CI sandbox —
> `ReadDirectoryChangesW` delivery can lag by multiple seconds after
> a write under load. The watcher path works in interactive dev
> (verified manually); we'll re-enable the test once we can pin a
> per-platform deadline that isn't either flaky or wasteful.

Two compounding sources of nondeterminism:

1. **OS event-delivery jitter.** `ReadDirectoryChangesW` on Windows
   batches/coalesces events asynchronously; macOS `FSEvents` has a
   coalesce-window of its own; Linux `inotify` is usually prompt
   but degrades under high I/O load. There's no portable "wait
   until the watcher has definitely seen this write" primitive.

2. **Write coalescing.** A single editor save is often Create +
   Modify + Modify; `notify` debounces these into one logical event.
   That's correct behaviour — but it means the watcher only fires
   after a quiescence window expires.

A reliable integration test would need to budget for both the OS
delivery delay AND the debounce — call it ~600ms baseline plus a
generous variance — and even then would flake under contended CI.

## Decision: Option B — env-gated test hook

We chose **Option B** from the swarm brief: bypass `notify`,
exercise the rebuild-and-broadcast path end-to-end via an in-process
trigger.

### Mechanism

1. The rebuild logic that the `notify` callback used to inline is
   now `AppState::rebuild_and_broadcast(pkg_root)` —
   single source of truth.

2. When `mty serve` starts with `MTY_SERVE_TEST_HOOKS=1` in env,
   it records `state.test_hooks_enabled = true`. This is read once
   at startup; flipping the env mid-run has no effect.

3. With both `test_hooks_enabled` AND `--watch` active, the handler
   routes `POST /_test_trigger_reload` to a tiny shim that calls
   `state.rebuild_and_broadcast(&watch_root).await`. The endpoint
   returns `200 ok` after the rebuild + broadcast complete.

4. The watcher task itself is unchanged — `notify` still drives the
   debounced rebuild for real users. The test hook is an extra entry
   point, not a replacement.

### What the new test covers

`crates/mty-cli/tests/cmd_serve_watch.rs`:

| Test | Asserts |
|------|---------|
| `watch_reload_broadcast_via_test_hook` | Full end-to-end: scaffold web-game, prebuild, spawn `mty serve --watch`, open `/_reload` ws, POST `/_test_trigger_reload`, assert a `b"reload"` text frame arrives on the ws within 60s (typical actual: <3s on a warm host). |
| `test_hook_404s_without_watch` | The hook returns `409` (not 200) when wired with `MTY_SERVE_TEST_HOOKS=1` but **without** `--watch`. Guards against a future refactor that silently exposes the hook on every server. |
| `test_hook_404s_without_env_var` | The hook returns `404` (route not registered) when `MTY_SERVE_TEST_HOOKS` is unset, even with `--watch`. End-user guarantee: nobody who didn't opt into the hook ever sees it. |

The websocket client is hand-rolled to mirror the hand-rolled server
(see `serve.rs::handle_reload_ws`): RFC 6455 client upgrade, then
read one minimal server-to-client text frame (`0x81, len, payload`).
Kept under 80 LOC; no `tungstenite` dep.

### Why this is deterministic

* `POST /_test_trigger_reload` returns synchronously after the
  rebuild + broadcast. There's no in-OS event-queue latency in the
  loop.
* The websocket subscriber is registered (via the upgrade GET) and
  blocked on `rx.recv().await` *before* the POST is issued, so the
  broadcast can't race the subscribe.
* The 60s read-deadline on the ws is ~20x the worst observed
  rebuild time we've seen locally; even on a contended CI bot
  doing a cold cargo build inside the rebuild, that's headroom.

### What this does NOT test

* `notify` actually delivers events on a real file write. That's
  exactly the source of CI flake we're routing around. The real
  integration is verified by the manual smoke (below).
* The 200ms debounce window. We trip the hook with a single HTTP
  POST; the debounce window only matters when bursts of events
  arrive on the channel.

## Real-notify smoke (manual)

Run after touching anything in `serve.rs::spawn_watcher` or any
`notify`-version bump:

```bash
# Terminal 1
cd $(mktemp -d) && mty new --template web-game smoke && cd smoke
mty serve --watch --port 8765

# Terminal 2
sleep 1
# Trip the watcher 3x; expect 3 "change detected, rebuilding" +
# "rebuild ok" lines in terminal 1 within ~2s.
for i in 1 2 3; do
  echo "// touch $i" >> src/main.mty
  sleep 0.5
done

# Terminal 3
# Confirm the page sees the reload — open
# http://127.0.0.1:8765 in a browser with devtools open, then
# repeat the touch loop. Expect the page to refresh each time.
```

This is the only path that proves `notify` itself is wired; we keep
it as a manual gate because the cost of running it in CI (flake +
slow + per-platform OS event semantics) outweighs the value.

## Pre-flight gate (run before push)

```bash
cargo build --workspace 2>&1 | tail -5
cargo test -p mty-cli --test cmd_serve --test cmd_serve_watch 2>&1 | tail -10
# Deterministic? Five-of-five must pass.
for i in 1 2 3 4 5; do
  cargo test -p mty-cli --test cmd_serve_watch -- --test-threads=1 2>&1 | tail -3
done
cargo clippy -p mty-cli --no-deps --all-targets -- -D warnings 2>&1 | tail -5
cargo fmt -p mty-cli -- --check 2>&1 | tail -5
```

Observed at slice cut (local Windows host):

* All 5 cmd_serve.rs tests green; all 3 cmd_serve_watch.rs tests green.
* Five-of-five determinism: each run ~1.5-3s wall-clock.
* clippy + fmt: clean (sibling-track WIP warnings in
  `mty-hir::lower::macros` are out of scope for this track).

## Follow-ups (post-v0.24)

* **Per-platform notify probe.** If we ever want to gate the real
  `notify` integration in CI, the right pattern is a separate probe
  test that writes a file in a tmpdir watched by `notify` and
  reports the observed delivery latency. Run it once at the top of
  the suite; skip the integration test on platforms where the probe
  is > N ms. This is more work than the test it replaces, hence
  parked.
* **Public test-hook documentation.** `MTY_SERVE_TEST_HOOKS=1` is
  intentionally undocumented in `docs/reference/cli/mty-serve.md` —
  it's a test-only knob, not a user feature. If the hook surface
  grows (e.g. a `--paused` mode), revisit.
