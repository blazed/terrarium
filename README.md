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

OpenRouter:

```nu
$env:OPENROUTER_API_KEY = (input --suppress-output "OpenRouter API key: ")
cargo run -- run --ticks 20 --llm-model openai/gpt-5-mini --llm-url https://openrouter.ai/api/v1 --llm-api-key-env OPENROUTER_API_KEY --llm-temperature 0.3 --llm-reasoning-effort medium --llm-max-tokens 1024 --llm-provider Anthropic
```

OpenCode Go (Chat Completions or Responses):

```nu
$env:OPENCODE_GO_API_KEY = (input --suppress-output "OpenCode Go API key: ")
cargo run -- run --ticks 1 --llm-model kimi-k3 --llm-url https://opencode.ai/zen/go/v1 --llm-api-key-env OPENCODE_GO_API_KEY
cargo run -- run --ticks 1 --llm-model gpt-5.6-luna --llm-url https://opencode.ai/zen/go/v1 --llm-api responses --llm-api-key-env OPENCODE_GO_API_KEY
```

`--database` atomically stores a resumable checkpoint containing the world's metadata, agents, locations, memories, and ordered events. `--resume PATH` validates and continues that checkpoint, then atomically updates it; combine it with `--database OTHER_PATH` to write elsewhere. `inspect` remains read-only.

`--llm-model` selects an OpenAI-compatible server at `http://localhost:11434/v1` by default; override it with `--llm-url`. `--llm-api chat` is the default and uses `/chat/completions`; `--llm-api responses` uses `/responses`. Remote endpoints require HTTPS. `--llm-api-key-env` reads a Bearer token from the named environment variable so secrets never appear in process arguments. `--llm-temperature` accepts `0` through `2` (default `0`), `--llm-reasoning-effort` accepts `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`, and `--llm-max-tokens` sets `max_completion_tokens` for Chat Completions or `max_output_tokens` for Responses. OpenRouter's `--llm-provider` pins requests to one provider and disables provider fallbacks. Optional fields are omitted unless configured; model support varies. LLM action outcomes print immediately as each response arrives. Model failures are traced and deterministically fall back to `Wait`. Set `RUST_LOG=debug` for detailed tracing.

## Validate

```nu
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. New worlds begin at Day 1 07:00. Seed-derived IDs, initial personalities and needs, and a seeded random engine make runs reproducible while different seeds diverge immediately.

Agents retain their 20 most recent witnessed movements, conversations, activities, and goal completions. These subjective memories are persisted with checkpoints and included in future decisions; unseen events and idle waits are omitted. Witnessed dialogue and work also form bounded beliefs about residents' sociability, reliability, and hostility. Repeated evidence raises confidence, stale confidence decays, and only the observer's beliefs influence their conversation choices.

Each resident has livelihood, community, exploration, and wellbeing goals. Successful matching actions advance bounded progress and emit one completion event at 100%; decision engines receive that subjective progress and prioritize feasible unfinished goals after urgent needs.

Needs are satisfaction values that decay with simulated time and recover through successful actions. Actions leave agents occupied for deterministic simulation time: travel for 10 minutes, conversation and eating for 15 minutes, observation and waiting for 5 minutes, and work or rest for one hour. Busy residents skip decisions until the activity ends, while urgent hunger, exhaustion, or danger interrupts it; nearby activities appear in subjective observations. Effects and movement remain immediate to preserve simple deterministic event ordering. The random engine responds to urgent food, energy, companionship, and safety needs, heads to work while its workplace is open, and returns home at night. Personality then shapes deterministic choices: openness and impulsiveness favor exploration, agreeableness favors company, ambition favors work, and neuroticism favors safety and rest. `Eat` is available at home or an open food-serving location, `Rest` at home, and `Work` at an open workplace; `Wait` has no activity effects. Briar Glen locations use simple fixed daily hours: the bakery opens 06:00–14:00, the tavern 12:00–23:00, the chapel 06:00–20:00, other workplaces 08:00–18:00, and Riverside Houses remain always open. Observations expose each location's hours and current status, and closed destinations are omitted from movement affordances.

Residents have one short-term mood value from negative to positive that decays toward neutral. Activities, goal completion, rejected actions, and dialogue tone adjust it; mood shapes fallback choices and conversation tone without overriding urgent needs or feasible goals. Only a resident's own mood appears in their subjective observation.

Conversations carry a friendly, supportive, neutral, or tense tone. Friendly and supportive dialogue strengthen relationships in different ways, while tense dialogue reduces affection, trust, and respect and raises suspicion; mood, personality, and the existing relationship shape deterministic tone choices. Observations expose only the observer's relationship toward each visible resident, and the random engine prefers stronger relationships when choosing conversation partners. Random dialogue reflects personality, tone, the listener, and current location; model dialogue can also use subjective memories. Tone and messages persist in events and memories, and messages are trimmed and limited to one printable line of 200 characters before they enter events, memories, or streamed output.

Model decisions use strict JSON proposals through the same authoritative validation path as random decisions. Observations include immediate action affordances: adjacent move destinations, co-located conversation targets, and whether eating, resting, or working is currently legal. They also include deterministic next-hop hints along the shortest currently open route toward home, work, and food. Routes are recalculated each turn rather than persisted; models remain restricted to immediate affordances, and authoritative validation remains the final boundary.
