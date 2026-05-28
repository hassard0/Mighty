# Agent Gallery

A curated set of Mighty examples that can be opened directly in the
browser playground. Each one is forkable with a single click — the
gallery entry encodes the source as a base64url permalink that the
playground decodes at load.

This directory is the source of truth. `tools/playground/src/examples.ts`
mirrors the same files for the in-page picker; updating one without the
other gets caught in CI (v0.34 follow-up: a `gallery_sync.rs` test).

## Layout

```
tools/gallery/
├── README.md
├── index.json                  # ordered manifest + permalink payloads
└── examples/
    ├── 01_hello_agent/main.mty
    ├── 02_tool_calling/main.mty
    ├── 03_swarm_review/main.mty
    ├── 04_eval_suite/main.mty
    ├── 05_taint_safety/main.mty
    ├── 06_observability/main.mty
    └── 07_computer_use/main.mty
```

## `index.json` shape

```jsonc
{
  "version": 1,
  "playgroundBase": "https://hassard0.github.io/Mighty/playground/",
  "examples": [
    {
      "id": "01_hello_agent",
      "title": "01 — Hello, Mighty",
      "summary": "One-line program — parses, type-checks, runs.",
      "capabilities": ["parse", "run"],
      "source": "examples/01_hello_agent/main.mty",
      "permalinkPayload": "<base64url(utf8(source))>"
    },
    // ...
  ]
}
```

- `id` — kebab-case, must match the directory name + the
  `tools/playground/src/examples.ts` constant id.
- `capabilities` — free-form tags. Used by the gallery search UI
  (v0.34) and the docs site indexer. Keep them lowercase.
- `permalinkPayload` — `base64url(utf8(source))`. The playground
  builds `?` URLs of the form
  `<playgroundBase>#code=<permalinkPayload>`.

## Add a new example

1. Pick the next two-digit prefix (`08_…`).
2. `mkdir tools/gallery/examples/08_my_example`.
3. Write `main.mty` — keep the leading comment block explaining
   *what the example demonstrates* in one paragraph + a build/run
   stanza.
4. Run the example through `mty fmt` + `mty check` to make sure
   the source is canonical and clean.
5. Compute the permalink payload:

   ```bash
   node -e "process.stdout.write(
     Buffer.from(require('fs').readFileSync(
       'tools/gallery/examples/08_my_example/main.mty'
     )).toString('base64')
       .replace(/\+/g,'-').replace(/\\//g,'_').replace(/=+\$/, '')
   )"
   ```

6. Append the entry to `tools/gallery/index.json` (keep ordering by
   `id`).
7. Mirror the source into `tools/playground/src/examples.ts` so the
   in-page picker shows the new entry. Add the `Example` record at
   the same index as in `index.json`.
8. Run the playground locally and verify the example loads via both
   the picker AND the permalink (`#code=…`).

## Why entries are duplicated in the playground source

The playground is a static site — it can't read `index.json` at
build time without a network round-trip (which defeats the
30-second-to-first-run goal). We accept the duplication and keep
the two files in sync via the (forthcoming) gallery_sync test.

## v0.34 follow-ups

- `gallery.html` static page that renders `index.json` + thumbnails
  + a per-example fork-into-playground button.
- A CI test (`tests/gallery_sync.rs`) that verifies every
  `examples.ts` entry has a matching `index.json` record and that
  the base64 payload round-trips back to the file source.
- Embed mode for blog/docs — `<iframe src="…/playground/embed.html#code=…">`.
