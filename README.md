# Terrarium

A deterministic social-simulation core for the town of Briar Glen. Eight residents act through subjective observations; decision engines may propose actions, but only the world validates and mutates objective state.

## Run

```nu
cargo run -- run
cargo run -- run --seed 814921 --days 7
cargo run -- run --seed 814921 --ticks 10000
cargo run -- run --seed 814921 --days 7 --database briar-glen.sqlite
cargo run -- run --resume briar-glen.sqlite --days 7
cargo run -- inspect briar-glen.sqlite
cargo run -- run --ticks 20 --llm-model qwen3:8b
```

OpenCode Go (using one of its Chat Completions models):

```nu
$env:OPENCODE_GO_API_KEY = (input --suppress-output "OpenCode Go API key: ")
cargo run -- run --ticks 1 --llm-model kimi-k3 --llm-url https://opencode.ai/zen/go/v1 --llm-api-key-env OPENCODE_GO_API_KEY
```

`--database` atomically stores a resumable checkpoint containing the world's metadata, agents, locations, memories, and ordered events. `--resume PATH` validates and continues that checkpoint, then atomically updates it; combine it with `--database OTHER_PATH` to write elsewhere. `inspect` remains read-only.

`--llm-model` selects an OpenAI-compatible Chat Completions server at `http://localhost:11434/v1` by default; override it with `--llm-url`. Remote endpoints require HTTPS. `--llm-api-key-env` reads a Bearer token from the named environment variable so secrets never appear in process arguments. Model failures are traced and deterministically fall back to `Wait`. Set `RUST_LOG=debug` for detailed tracing.

## Validate

```nu
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. Seed-derived IDs and a seeded random engine make runs reproducible.

Agents retain their 20 most recent witnessed movements, conversations, and observations. These subjective memories are persisted with checkpoints and included in future decisions; unseen events and idle waits are omitted. Beliefs remain intentionally unimplemented.

Needs are satisfaction values that decay with simulated time and recover through successful actions. The random engine responds to urgent food, energy, companionship, and safety needs, heads to work by day, and returns home at night. `Eat` is available at home and food-serving locations, `Rest` at home, and `Work` at an agent's workplace from 08:00 through 17:59; `Wait` has no activity effects.

Conversations strengthen both residents' directional affection, trust, and respect while reducing suspicion. Observations expose only the observer's relationship toward each visible resident, and the random engine prefers stronger relationships when choosing conversation partners.

Model decisions use strict JSON proposals through the same authoritative validation path as random decisions.
