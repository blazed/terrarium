# Terrarium

A deterministic social-simulation core for the town of Briar Glen. Eight residents act through subjective observations; decision engines may propose actions, but only the world validates and mutates objective state.

## Run

```nu
cargo run -- run
cargo run -- run --seed 814921 --days 7
cargo run -- run --seed 814921 --ticks 10000
cargo run -- run --seed 814921 --days 1 --live
cargo run -- run --seed 814921 --days 7 --database briar-glen.sqlite
cargo run -- run --town assets/briar_glen.json --seed 42 --days 7
cargo run -- run --resume briar-glen.sqlite --days 7
cargo run -- inspect briar-glen.sqlite
cargo run -- report briar-glen.sqlite
cargo run -- report briar-glen.sqlite --json
cargo run -- chronicle briar-glen.sqlite
cargo run -- chronicle briar-glen.sqlite --all
cargo run -- run --ticks 20 --llm-model qwen3:8b
cargo run -- run --days 7 --llm-model qwen3:8b --llm-log decisions.jsonl
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

`--database` atomically stores a version-12 resumable checkpoint containing the world's metadata, agents, locations, health/lifecycle state, intentions, memories, and ordered events. `--resume PATH` validates and continues that checkpoint, then atomically updates it; combine it with `--database OTHER_PATH` to write elsewhere. `--live` shows one dynamically sized horizontal row per resident with full location, activity, health/life state, finances, mood, inventory, needs, current goal, intention, and strongest relationship; recent dialogue is shown in full. The terminal is restored when the run ends. `inspect` renders the same dashboard from a validated saved checkpoint and remains read-only. `report` prints run, resident, social, economy, behaviour, and LLM metrics from the validated checkpoint; `--json` prints the same data as JSON. `chronicle` renders the event log by day with a daily summary and hides waits, observations, and rejected actions unless `--all` is given.

Briar Glen uses a deterministic closed-loop marketplace. Residents start with 20 coins; workplaces start with 100 operating coins, pay 10-coin wages from that cash, and reject work when payroll is insolvent. Businesses sell meals for 5 coins, supplies for 6, repair kits for 8, medicine or treatment for 12, or civic services for 4. Tangible purchases consume one stock unit and enter the resident's inventory; civic services apply immediately. Each resident can hold up to three of each item and must explicitly consume a meal or use supplies, a repair kit, or medicine to restore needs or health. Briar Glen Clinic is connected to Town Hall and Riverside Houses; it sells medicine and charges the same configured price for treatment. Residents build small reserves in ordinary conditions, prefer those reserves during shortages, and use safety items more readily during storms. Every ordinary shift produces four stock units. Prices, wages, and starting balances are fixed. Co-located residents can give a needed meal, supply pack, repair kit, or medicine when the receiver has capacity. Agreeable residents share surplus inventory with people they trust after securing their own urgent needs; aid improves mood and mutual relationships and is preserved in witnessed memories.

Each simulated day schedules one deterministic six-hour town event. Storms close non-home locations and accelerate safety loss; festivals strengthen companionship and status gains from conversation; shortages halve workplace production; and market days double it. The seed controls the daily event rotation and start hour. Active events and remaining time appear in observations and the dashboard, start/end events stream with other outcomes, and checkpoints preserve exact event state.

Residents have bounded health and can become hungry, exhausted, injured, or symptomatic. Critical needs and symptomatic Briar fever reduce health; sustained illness can cause death, which removes a resident from future scheduling while preserving their historical record. Briar fever's deterministic outbreak begins on Day 2 at 08:00: infection incubates for one day, symptoms last two days, recovery lasts one day, and immunity lasts three days. Transmission is evaluated hourly between co-located residents from the seed, tick, and stable resident IDs. Incubation remains private; only symptoms are included in the resident's subjective observations. Disease infection, symptoms, recovery, immunity expiry, and death are persisted as ordered events.

`--llm-model` enables a local-first hybrid engine: deterministic internal AI handles routine needs, travel, work, commerce, health, and existing intentions, while the LLM is reserved for spaced social or cognitive choices. An accepted model response establishes a bounded intention; local AI then performs its travel and follow-up steps without another request. LLM rest continues until energy reaches 80%, work continues for up to three simulated hours, and urgent needs, invalid state, expiry, or town disruption interrupt safely. Summaries report starts, local follow-up steps, completions, and interruptions. The default budget is two LLM attempts per resident per simulated day; override it with positive `--llm-calls-per-day N`. `--llm-log PATH` writes JSONL entries for accepted LLM proposals, their normalized intentions, local continuation actions, and terminal completion or interruption reasons; it excludes prompts, credentials, hidden state, and unrelated local decisions. Budgets, timing, intention telemetry, and cumulative routing counts are checkpointed, and model failures execute the internal proposal instead of wasting the tick on `Wait`. The OpenAI-compatible server defaults to `http://localhost:11434/v1`; override it with `--llm-url`. `--llm-api chat` uses `/chat/completions`; `--llm-api responses` uses `/responses`. Remote endpoints require HTTPS. `--llm-api-key-env` reads a Bearer token from the named environment variable. `--llm-temperature` accepts `0` through `2`, `--llm-reasoning-effort` accepts `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`, and `--llm-max-tokens` limits output. OpenRouter's `--llm-provider` pins one provider and disables provider fallbacks. Requests stream with a 120-second inactivity timeout. Set `RUST_LOG=debug` for detailed tracing.

## Town as data

Towns are authored as JSON. The built-in Briar Glen lives at `assets/briar_glen.json` and
remains the default when no `--town` is given; it is also the complete schema example.
Each run can load any other town instead:

```nu
cargo run -- run --town mytown.json --seed 42
```

- Location and resident array order determines the seeded, deterministic IDs; every
  reference (`home`, `workplace`, connections) uses the unique location name, never an
  index, so reordering cannot silently redirect a reference.
- `connections` lists each undirected edge once; the loader inserts both runtime
  directions.
- The seed controls all IDs and the sampled personality and need values. Each trait is
  sampled as `(base + uniform(-spread..=spread)).clamp(0, 1)`, so `base ± spread` must
  stay inside `[0, 1]`.
- `--town` cannot be combined with `--resume`, because a resumed checkpoint already
  contains its world.
- Business cash, stock, revenue, wages, and the other runtime accounting fields are
  not configurable; they always start from the same fixed constants.

## Validate

```nu
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo deny check
```

## Architecture

The pipeline is `World → perceive → AgentObservation → DecisionEngine → ProposedAction → World::execute → Event`. IDs, simulation time, actions, rejections, and events are typed. New worlds begin at Day 1 07:00. Seed-derived IDs, initial personalities and needs, and a seeded local engine make runs reproducible while different seeds diverge immediately.

Agents retain their 20 most recent witnessed movements, conversations, activities, goal completions, and confrontations. These subjective memories are persisted with checkpoints and included in future decisions; unseen events and idle waits are omitted. Conversations can pass along one event the speaker knows and the listener does not. These bounded rumors record the immediate source, retelling depth, and degrading confidence; honesty and the listener's trust affect credibility. A resident may confront a visible rumor subject once: honesty, mood, source credibility, and relationships produce a confirmation, denial, or challenge that updates confidence, beliefs, mood, and both relationships. Credible firsthand evidence and hearsay form bounded beliefs about residents' sociability, reliability, and hostility. Repeated evidence raises confidence, stale confidence decays, and only the observer's memories, rumors, and beliefs influence their choices.

Each resident keeps up to three deterministic contextual goals drawn from current needs, personality, occupation, relationships, and personally visited locations. Goals carry concrete work, conversation, destination, meal, or rest targets with required action counts and a one-day expiry. Only an exact authoritative action advances a goal; completed, expired, or impossible goals are replaced from the resident's latest context and emit completion events. Decision engines receive only that resident's active goals and prioritize feasible targets after urgent needs.

Residents may persist one short-term intention to visit a destination, make a purchase, rest, work, or speak to someone. The world immediately executes its first legal step, recalculates deterministic open routes for later steps, and performs terminal actions through the same authoritative validation path. Valid intentions continue before requesting another model decision, expire after three simulated hours, survive checkpoints, and clear when completed, rejected, unreachable, invalid, or interrupted by urgent needs. Only a resident's own intention appears in their subjective observation.

Needs are satisfaction values that decay with simulated time and recover through successful actions. Residents also have normalized health, a persistent injury flag, and an explicit alive/dead lifecycle. Critical hunger, exhaustion, and danger slowly damage health; supplies and repair kits restore safety and health, rest restores health, and severe injuries make work unavailable. Health reaching zero emits one death event, removes the resident from scheduling and location occupancy while retaining their historical record, and the runner stops early when no residents remain alive. Subjective observations expose only the observer's own health and injury state. Actions leave agents occupied for deterministic simulation time: travel for 10 minutes, conversation and shopping for 15 minutes, observation and waiting for 5 minutes, and work or rest for one hour. Every living idle resident acts once per tick in stable ID order. Busy residents skip decisions until the activity ends, while urgent hunger, exhaustion, or danger interrupts it; nearby activities appear in subjective observations. Effects and movement remain immediate to preserve simple deterministic event ordering. The local engine responds to urgent food, energy, companionship, and safety needs, heads to work while its workplace is open, and returns home at night. Personality then shapes deterministic choices: openness and impulsiveness favor exploration, agreeableness favors company, ambition favors work, and neuroticism favors safety and rest. `Eat` is available at home or an open food-serving location, `Rest` at home, and `Work` at an open workplace; `Wait` has no activity effects. Briar Glen locations use simple fixed daily hours: the bakery opens 06:00–14:00, the tavern 12:00–23:00, the chapel 06:00–20:00, other workplaces 08:00–18:00, and Riverside Houses remain always open. Riverside Houses is the shared residential hub; each of the eight residents owns a separate Home-kind home connected only to it, so travel between town and home routes through the hub. Observations expose each location's hours and current status, and closed destinations are omitted from movement affordances.

Residents have one short-term mood value from negative to positive that decays toward neutral. Activities, goal completion, rejected actions, and dialogue tone adjust it; mood shapes fallback choices and conversation tone without overriding urgent needs or feasible goals. Only a resident's own mood appears in their subjective observation.

Conversations carry a friendly, supportive, neutral, or tense tone. Friendly and supportive dialogue strengthen relationships in different ways, while tense dialogue reduces affection, trust, and respect and raises suspicion; mood, personality, and the existing relationship shape deterministic tone choices. Observations expose only the observer's relationship toward each visible resident and the latest tick when they spoke directly, derived from existing memories rather than new state. Optional conversations avoid partners spoken to within the previous six simulated hours when another action or partner is available; urgent companionship and explicit goals may still talk. Local dialogue reflects personality, tone, the listener, and current location; model dialogue can also use subjective memories and is prompted to vary statements, offers, requests, thanks, and questions. Tone and messages persist in events and memories, and messages are trimmed and limited to one printable line of 200 characters before they enter events, memories, or streamed output.

Model decisions use strict JSON proposals through the same authoritative validation path as local decisions. Observations include immediate action affordances: adjacent move destinations, co-located conversation targets, and whether purchasing, resting, or working is currently legal. They also include each route's final destination and deterministic next hop along the shortest currently open path toward home, work, and the nearest affordable stocked offering for the resident's current need. Routes are recalculated while the intention persists; models remain restricted to subjective hints and immediate affordances, and authoritative validation remains the final boundary.
