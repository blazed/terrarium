# Terrarium

A deterministic social-simulation core for the town of Briar Glen. Eight residents act through subjective observations; decision engines may propose actions, but only the world validates and mutates objective state.

## Run

```nu
cargo run -- run
cargo run -- run --seed 814921 --days 7
cargo run -- run --seed 814921 --ticks 10000
cargo run -- run --seed 814921 --days 7 --database briar-glen.sqlite
cargo run -- inspect briar-glen.sqlite
cargo run -- run --ticks 20 --llm-model qwen3:8b
```

OpenCode Go (using one of its Chat Completions models):

```nu
$env:OPENCODE_GO_API_KEY = (input --suppress-output "OpenCode Go API key: ")
cargo run -- run --ticks 1 --llm-model kimi-k3 --llm-url https://opencode.ai/zen/go/v1 --llm-api-key-env OPENCODE_GO_API_KEY
```

`--database` atomically stores the completed world's metadata, agents, locations, and ordered events in SQLite. `inspect` reads that database in a later process.

`--llm-model` selects an OpenAI-compatible Chat Completions server at `http://localhost:11434/v1` by default; override it with `--llm-url`. Remote endpoints require HTTPS. `--llm-api-key-env` reads a Bearer token from the named environment variable so secrets never appear in process arguments. Model failures are traced and deterministically fall back to `Wait`. Set `RUST_LOG=debug` for detailed tracing.

## Validate

```nu
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. Seed-derived IDs and a seeded random engine make runs reproducible.

Persistence is a completed-run snapshot rather than resumable simulation state. Model decisions use strict JSON proposals through the same authoritative validation path as random decisions.
