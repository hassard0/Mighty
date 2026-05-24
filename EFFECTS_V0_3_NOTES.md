# EFFECTS_V0_3_NOTES — v0.3 soundness-hardening interpretation log

Owner: effect / capability hardening sub-agent of the v0.3 swarm.
Scope: `crates/mty-types/`, `tests/conformance/{effect_checking,
capability_checking,agent_protocol}/`, the v0.3 sections of
`docs/internals/{typeck,sendable,effects,capabilities,traits}.md`,
and the A65 amendments in `docs/spec/v0.1-amendments.md`.

## Headline

Three loose ends from slice 3 / 4 / 5 are now formally closed:

1. **A21 → A65 scope-aware tolerance.** Slice 3's permissive
   "unknown values resolve to fresh inference vars" policy now
   only applies in permissive scopes (top-level fn, extern, macro,
   unsafe, arena, budget, sandbox). Strict scopes (agent body,
   handler body, plus the framework-marked supervisor/cap-narrow
   scopes) promote an unresolved name to MT2021.
2. **A65.b Sendable.** The cross-agent message-arg type-shape
   contract is now a formal trait: Copy ∨ (owned + Sized + no
   internal refs), with `#[derive(Sendable)]` opt-in. MT3011
   fires at every `!Msg(...)` / `?Msg(...)` site.
3. **A65.c MT4031 strict handler param-type check.** For protocols
   defined in the current package, handler params are bound to
   fresh inference vars; the body's usage type is unified post-hoc
   with the protocol's declared type, with mismatch reported as
   MT4031. External protocols continue to warn MT2026.

## Interpretation calls

### IC1 — Strict policy in supervisor/cap-narrow bodies

`SupervisorBody` and `CapNarrowBody` are marked strict for
framework consistency, but the v0.3 implementation keeps
`tolerance_open=true` for both. Reason: today supervisor child
expressions reference capability names that the runtime injects
(`net`, `clock`, ...) — those names aren't bound at the type-check
phase. Sandbox-with bodies similarly invoke runtime-provided
identifiers (`run job(input)?` in example 18). Flipping the
toggle to false would regress canonical example 18.

**Resolution:** keep `tolerance_open=true` for these scopes; the
ScopeKind framework is in place so that when slice-7 wires real
cap-name resolution, the toggle flips and MT2021 activates
automatically with no further code change. Documented in
`docs/internals/typeck.md` and the A65 amendment.

### IC2 — Generic / unbound types in Sendable check

The Sendable check treats `Var(_)` and `Param(_)` types as
permissive (no MT3011 fires). Reason: slice-3 inference frequently
leaves fresh vars in send-arg positions that pin to concrete
Sendable types only after defaulting. Rejecting them at the
type-checker gate would over-reject in practice. The cost is that
truly-non-Sendable generic instantiations (e.g. a sender passing
a `&T` via a generic helper) escape the static check — but those
are caught downstream by the borrow checker's MT3009/MT3008.

### IC3 — MT4031 protocol param post-check vs intra-body unification

A naive implementation would bind handler params at the
protocol-declared type and let any incompatible usage downstream
surface as MT2001 type_mismatch. The spec ask was clearer: emit
MT4031 with cross-references to both the handler and the protocol
declaration.

**Implementation:** bind to fresh inference vars (for local
protocols only), type-check the body normally (downstream SD2001s
suppressed only by the body's structural shape), then run a final
unification between each param's inferred type and the protocol's
declared type. Mismatch → MT4031 with both spans and the inferred
vs declared type names. Local-vs-external is detected via
`DefMap::protocol_msg_names` membership.

For external protocols we fall back to the slice-5 behavior:
bind to the declared types and skip the MT4031 check. This keeps
canonical examples 13 (uses opaque `Fetch` protocol) and 19 (uses
`http.Handler`) compiling unchanged.

### IC4 — Strict `core` profile conformance case

The strict `profile = "core"` MT4002 check is exercised by the new
unit test `crate::effects::tests::core_profile_rejects_alloc`.
The conformance case shape lives at
`tests/conformance/effect_checking/05_strict_core_profile/` but
asserts the no-error (host) outcome because the harness reads the
workspace's `mighty.toml` directly. Per-case `mighty.toml` overrides
are flagged as v0.3.1 work in the case README.

### IC5 — Sendable handling of `AgentRef[T]`

An `AgentRef[T]` is Sendable iff `T` is. This matches the
distributed-actor-topology pattern: agents handing references to
other agents around is exactly how the supervisor tree is wired,
and the AgentRef itself carries no host-side authority that would
break when crossing a boundary.

### IC6 — Sendable handling of opaque ADTs

Opaque ADTs (prelude types like `Url`, `Page`, `IoErr`) are
treated as Sendable. Reason: opaque means "we don't know the
internal shape", and slice-3 intent is to be conservative-
permissive in unknown territory. A real cross-package metadata
system (v0.4+) will be able to consult external Sendable bounds
and refine this.

## Post-v0.3 work flagged

1. **Slice-7 supervisor/cap-narrow name binding.** Once
   supervisor scopes expose their child cap names to type-check,
   flip `tolerance_open=false` in the SupervisorBody /
   CapNarrowBody branches; the existing strict ScopeKind will
   pick up MT2021 automatically.
2. **Per-case `mighty.toml` in the conformance harness.** Today the
   harness reads the workspace `mighty.toml`; adding per-case
   overrides enables direct conformance assertion of MT4002 (and
   future profile-driven diagnostics).
3. **Function-signature cap-narrowing.** MT4010 fires only at
   method-call subsumption sites today. v0.3.1 should propagate
   `CapConstraint::And(...)` into fn signatures so callers can
   declare narrowed-Fs parameters explicitly and the call-site
   subsumption check elevates broader caps to MT4010.
4. **Cross-package Sendable propagation.** When external crates
   declare a type Sendable via metadata, our MT3011 check should
   see the bound. Today external opaque ADTs are permissively
   Sendable; v0.4 should tighten this when the metadata API
   exists.
5. **Sendable lambda capture analysis.** `TyData::Fn { .. }` is
   currently treated as Sendable because slice-5 doesn't express
   closure captures. A real Sendable-for-Fn rule needs the
   capture set on the type.

## New diagnostic codes

(No new code numbers; existing MT2021, MT3011, MT4031, MT4002 are
re-aimed.)

- MT2021 (unresolved_value) — text + note updated to call out the
  strict scope context.
- MT3011 (non_sendable_message_arg) — rule formalized; text +
  reason note tied to the Sendable definition.
- MT4031 (protocol_param_type_mismatch) — newly activated for
  local protocols; text + note tied to the protocol declaration.
- MT4002 (alloc_in_core) — explain text + conformance case shape
  added; no behavior change to the check itself.
- MT4041 (derive_unknown) — text updated to include `Sendable` in
  the list of supported derives.

## Test count delta

- 22 → 25 unit tests inside `mty-types` (added Sendable + two
  effects-profile tests).
- 0 → 14 new integration tests under
  `crates/mty-types/tests/` (scope-strict / scope-permissive /
  sendable / protocol-param-strict / cap-subsumption files).
- 4 new conformance cases under `tests/conformance/`.

## Files modified

- `crates/mty-types/src/check.rs` — added `ScopeKind`,
  `check_sendable_arg`, scope_kind plumbing in sub-scope openers,
  rewrote `synth_path` strict/permissive fork.
- `crates/mty-types/src/items.rs` — set scope_kind for each
  Cx construction (TopLevelFn / AgentBody / HandlerBody /
  SupervisorBody), added local-protocol MT4031 post-check, added
  helpers `is_handler_protocol_local` / `first_protocol_name`.
- `crates/mty-types/src/defs.rs` — added `DefMap::user_sendable`.
- `crates/mty-types/src/resolve.rs` — `derive(Sendable)` handler.
- `crates/mty-types/src/sendable.rs` (NEW) — Sendable rule.
- `crates/mty-types/src/lib.rs` — wire `sendable` module.
- `crates/mty-types/src/diag.rs` — added
  `unresolved_value_strict`, `protocol_param_type_mismatch`,
  `non_sendable_message_arg`; updated derive_unknown text path.
- `crates/mty-types/src/effects.rs` — added strict-profile +
  parse-profile unit tests.
- `crates/mty-types/Cargo.toml` — add `mty-driver` dev-dep
  for the integration tests' end-to-end pipeline.
- `crates/mty-diagnostics/src/codes.rs` — updated MT2021, MT3011,
  MT4031, MT4041 explain text.
- `docs/internals/{typeck,sendable,effects,capabilities,traits}.md`
  — A65 documentation.
- `docs/spec/v0.1-amendments.md` — A65, A65.b, A65.c, A65.d.
- `tests/conformance/effect_checking/{04_undeclared_alloc,
  05_strict_core_profile}/`,
  `tests/conformance/capability_checking/04_cap_too_broad/`,
  `tests/conformance/agent_protocol/05_handler_param_type/` —
  new conformance cases.
- `crates/mty-types/tests/{scope_strict_agent.rs,
  scope_strict_supervisor.rs, scope_permissive_extern.rs,
  sendable_copy_passes.rs, sendable_non_copy_fails.rs,
  protocol_param_strict.rs, cap_subsumption.rs}` — new
  integration tests.
