# Terrarium

A deterministic social-simulation core for the town of Briar Glen. Eight residents act through subjective observations; decision engines may propose actions, but only the world validates and mutates objective state.

## Run

```nu
cargo run -- run
cargo run -- run --seed 814921 --days 7
cargo run -- run --seed 814921 --ticks 10000
cargo run -- run --seed 814921 --days 1 --live
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

`--database` atomically stores a version-8 resumable checkpoint containing the world's metadata, agents, locations, intentions, memories, and ordered events. `--resume PATH` validates and continues that checkpoint, then atomically updates it; combine it with `--database OTHER_PATH` to write elsewhere. `--live` shows one dynamically sized horizontal row per resident with full location, activity, finances, mood, inventory, needs, current goal, intention, and strongest relationship; recent dialogue is shown in full. The terminal is restored when the run ends. `inspect` renders the same dashboard from a validated saved checkpoint and remains read-only.

Briar Glen uses a deterministic closed-loop marketplace. Residents start with 20 coins; workplaces start with 100 operating coins, pay 10-coin wages from that cash, and reject work when payroll is insolvent. Businesses sell meals for 5 coins, supplies for 6, repair kits for 8, or civic services for 4. Tangible purchases consume one stock unit and enter the resident's inventory; civic services apply immediately. Each resident can hold up to three of each item and must explicitly consume a meal or use supplies or a repair kit to restore food or safety. Residents build small reserves in ordinary conditions, prefer those reserves during shortages, and use safety items more readily during storms. Every ordinary shift produces four stock units. Prices, wages, and starting balances are fixed.

Each simulated day schedules one deterministic six-hour town event. Storms close non-home locations and accelerate safety loss; festivals strengthen companionship and status gains from conversation; shortages halve workplace production; and market days double it. The seed controls the daily event rotation and start hour. Active events and remaining time appear in observations and the dashboard, start/end events stream with other outcomes, and checkpoints preserve exact event state.

`--llm-model` selects an OpenAI-compatible server at `http://localhost:11434/v1` by default; override it with `--llm-url`. `--llm-api chat` is the default and uses `/chat/completions`; `--llm-api responses` uses `/responses`. Remote endpoints require HTTPS. `--llm-api-key-env` reads a Bearer token from the named environment variable so secrets never appear in process arguments. `--llm-temperature` accepts `0` through `2` (default `0`), `--llm-reasoning-effort` accepts `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`, and `--llm-max-tokens` sets `max_completion_tokens` for Chat Completions or `max_output_tokens` for Responses. OpenRouter's `--llm-provider` pins requests to one provider and disables provider fallbacks. Optional fields are omitted unless configured; model support varies. LLM requests stream output and use the 120-second timeout only for inactivity, so a slow model keeps running while it continues producing data; servers that ignore streaming and return ordinary JSON remain supported. LLM action outcomes print immediately as each response arrives. Model failures are traced and deterministically fall back to `Wait`. Set `RUST_LOG=debug` for detailed tracing.

## Validate

```nu
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. New worlds begin at Day 1 07:00. Seed-derived IDs, initial personalities and needs, and a seeded random engine make runs reproducible while different seeds diverge immediately.

Agents retain their 20 most recent witnessed movements, conversations, activities, goal completions, and confrontations. These subjective memories are persisted with checkpoints and included in future decisions; unseen events and idle waits are omitted. Conversations can pass along one event the speaker knows and the listener does not. These bounded rumors record the immediate source, retelling depth, and degrading confidence; honesty and the listener's trust affect credibility. A resident may confront a visible rumor subject once: honesty, mood, source credibility, and relationships produce a confirmation, denial, or challenge that updates confidence, beliefs, mood, and both relationships. Credible firsthand evidence and hearsay form bounded beliefs about residents' sociability, reliability, and hostility. Repeated evidence raises confidence, stale confidence decays, and only the observer's memories, rumors, and beliefs influence their choices.

Each resident keeps up to three deterministic contextual goals drawn from current needs, personality, occupation, relationships, and personally visited locations. Goals carry concrete work, conversation, destination, meal, or rest targets with required action counts and a one-day expiry. Only an exact authoritative action advances a goal; completed, expired, or impossible goals are replaced from the resident's latest context and emit completion events. Decision engines receive only that resident's active goals and prioritize feasible targets after urgent needs.

Residents may persist one short-term intention to visit a destination, make a purchase, rest, work, or speak to someone. The world immediately executes its first legal step, recalculates deterministic open routes for later steps, and performs terminal actions through the same authoritative validation path. Valid intentions continue before requesting another model decision, expire after three simulated hours, survive checkpoints, and clear when completed, rejected, unreachable, invalid, or interrupted by urgent needs. Only a resident's own intention appears in their subjective observation.

Needs are satisfaction values that decay with simulated time and recover through successful actions. Actions leave agents occupied for deterministic simulation time: travel for 10 minutes, conversation and shopping for 15 minutes, observation and waiting for 5 minutes, and work or rest for one hour. Busy residents skip decisions until the activity ends, while urgent hunger, exhaustion, or danger interrupts it; nearby activities appear in subjective observations. Effects and movement remain immediate to preserve simple deterministic event ordering. The random engine responds to urgent food, energy, companionship, and safety needs, heads to work while its workplace is open, and returns home at night. Personality then shapes deterministic choices: openness and impulsiveness favor exploration, agreeableness favors company, ambition favors work, and neuroticism favors safety and rest. `Eat` is available at home or an open food-serving location, `Rest` at home, and `Work` at an open workplace; `Wait` has no activity effects. Briar Glen locations use simple fixed daily hours: the bakery opens 06:00–14:00, the tavern 12:00–23:00, the chapel 06:00–20:00, other workplaces 08:00–18:00, and Riverside Houses remain always open. Observations expose each location's hours and current status, and closed destinations are omitted from movement affordances.

Residents have one short-term mood value from negative to positive that decays toward neutral. Activities, goal completion, rejected actions, and dialogue tone adjust it; mood shapes fallback choices and conversation tone without overriding urgent needs or feasible goals. Only a resident's own mood appears in their subjective observation.

Conversations carry a friendly, supportive, neutral, or tense tone. Friendly and supportive dialogue strengthen relationships in different ways, while tense dialogue reduces affection, trust, and respect and raises suspicion; mood, personality, and the existing relationship shape deterministic tone choices. Observations expose only the observer's relationship toward each visible resident, and the random engine prefers stronger relationships when choosing conversation partners. Random dialogue reflects personality, tone, the listener, and current location; model dialogue can also use subjective memories. Tone and messages persist in events and memories, and messages are trimmed and limited to one printable line of 200 characters before they enter events, memories, or streamed output.

Model decisions use strict JSON proposals through the same authoritative validation path as random decisions. Observations include immediate action affordances: adjacent move destinations, co-located conversation targets, and whether purchasing, resting, or working is currently legal. They also include each route's final destination and deterministic next hop along the shortest currently open path toward home, work, and the nearest affordable stocked offering for the resident's current need. Routes are recalculated while the intention persists; models remain restricted to subjective hints and immediate affordances, and authoritative validation remains the final boundary.
