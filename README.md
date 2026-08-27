# Terrarium

A deterministic social-simulation core for the town of Briar Glen. Eight residents act through subjective observations; decision engines may propose actions, but only the world validates and mutates objective state.

## Run

```nu
cargo run -- run
cargo run -- run --seed 814921 --days 7
cargo run -- run --seed 814921 --ticks 10000
cargo run -- run --seed 814921 --days 7 --database briar-glen.sqlite
cargo run -- inspect briar-glen.sqlite
```

`--database` atomically stores the completed world's metadata, agents, locations, and ordered events in SQLite. `inspect` reads that database in a later process. Set `RUST_LOG=debug` to see tracing output separately from simulation output.

## Validate

```nu
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. Seed-derived IDs and a seeded random engine make runs reproducible.

Persistence is a completed-run snapshot rather than resumable simulation state. LLM integration remains intentionally deferred.
