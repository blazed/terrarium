# AGENTS.md

## Scope

These instructions apply to the entire repository. Add a nested `AGENTS.md` only when a subtree genuinely needs different rules.

## Direction

A Westworld-style town that runs itself: deterministic local AI drives every
resident, an LLM makes a bounded number of social and cognitive decisions,
and the town generates its own drama — economy, housing, relationships,
crime. Spectator mode first; a game may come later.

## Backlog

Work is planned as GitHub milestones and issues:
https://github.com/blazed/terrarium/milestones. Each issue is sized for one
focused session; `needs-spec` means the intent is fixed but details are not.
When a milestone starts, its issues become a `[task:*]` DAG of empty jj
revisions (`jj-todo-workflow`); the landing commit carries `Closes #N`.
Read the current milestone before proposing new work.

## Standing decisions

- Every idle resident acts each tick, in stable ID order.
- The local engine is deterministic per seed; LLM runs are not and need not be.
- The spectator UI is a static page over an exported checkpoint; no server.
- Crime is theft and violence with real-world consequences: witnesses,
  rumors, a sheriff, jail, reputation.
- Population changes through newcomers only: no births, no aging.
- No configuration for values that never differ between runs; the town
  definition is data because there will be several towns.

## Project priorities

- Prefer simple, direct Rust over abstractions for hypothetical future needs.
- **Breaking changes are preferred. No backwards compatibility is required.** Keep the current model clean instead of adding compatibility layers.
- Do not add legacy deserializers, `serde` aliases/defaults, migrations, deprecated APIs, adapters, or dual code paths solely to preserve old behavior or stored data.
- When changing an API, schema, event, or checkpoint format, update all callers, fixtures, tests, and documentation in the same change.
- Preserve deterministic behavior for a given world seed unless the requested behavior intentionally changes it.

## Architecture

- `World::execute` is the authoritative action-validation and mutation boundary. Decision engines propose actions; they must not duplicate world validation.
- Decision engines may use only `AgentObservation`, never unrestricted world state.
- Keep observations subjective: do not leak unseen agents, events, memories, beliefs, or relationships.
- Keep successful and rejected actions represented by immutable events.
- Persistence must load into a validated `World`; reject invalid state rather than repairing it silently.
- Use existing domain types and helpers before introducing new ones.

## Rust conventions

- Use stable Rust and the repository's existing Rust 2024 style.
- Prefer standard-library features and already-installed dependencies. Add a dependency only when it materially reduces code or risk.
- Keep errors typed at trust and persistence boundaries.
- Avoid speculative traits, factories, wrappers, and one-use forwarding helpers.
- Add the smallest focused regression test for non-trivial behavior changes and bug fixes.

## Validation

Run these before declaring work complete:

```text
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

For persistence or determinism changes, also compare split checkpoint/resume behavior with an uninterrupted run.

Report which checks ran and any checks that were skipped or failed.

## Version control

This repository uses Jujutsu (`jj`) with a Git backend. Use `jj`, not mutating `git` commands, and preserve unrelated working-copy changes.
