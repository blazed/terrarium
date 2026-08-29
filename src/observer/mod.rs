use crate::sim::{
    Agent, AgentId, Event, EventKind, IntentionGoal, LifeState, LocationId, ObservationTarget,
    RoutingStats, World,
};

pub fn render_run(world: &World) -> String {
    render_run_since(world, 0)
}

pub fn render_run_since(world: &World, first_event: usize) -> String {
    let mut lines = vec![
        world.name.clone(),
        format!("Seed: {}", world.seed),
        format!("Agents: {}", world.agents.len()),
        String::new(),
    ];
    let mut last_day = 0;
    for event in world.events().iter().skip(first_event) {
        if event.tick.day() != last_day {
            last_day = event.tick.day();
            lines.push(format!("Day {last_day}"));
            lines.push(String::new());
        }
        lines.push(render_event(world, event));
    }

    lines.extend([String::new(), render_summary(world)]);
    lines.join("\n")
}

pub fn render_dashboard(world: &World) -> String {
    let counts = event_counts(world);
    let (local, attempts, llm, fallbacks) = routing_counts(world);
    let (intentions, steps, completed, interrupted) = intention_counts(world);
    let town_event = world.active_town_event.map_or_else(
        || "Town event: none".into(),
        |event| {
            format!(
                "Town event: {} ({} ticks remaining)",
                event.kind,
                event.ends_at.0 - world.tick.0
            )
        },
    );
    let mut lines = vec![
        format!("=== {} — {} ===", world.name, world.tick),
        town_event,
        format!(
            "Events: {} | Moves: {} | Talks: {} | Confrontations: {} | Rejected: {}",
            world.events().len(),
            counts.moves,
            counts.conversations,
            counts.confrontations,
            counts.rejected
        ),
        format!(
            "Purchases: {} | Items used: {} | Treatments: {} | Rests: {} | Work: {} | Goals completed: {} | Deaths: {}",
            counts.purchases,
            counts.items_used,
            counts.treatments,
            counts.rests,
            counts.work,
            counts.goals,
            counts.deaths
        ),
        format!(
            "Local decisions: {local} | LLM attempts: {attempts} | LLM decisions: {llm} | LLM fallbacks: {fallbacks}"
        ),
        format!(
            "LLM intentions: {intentions} | Local steps: {steps} | Completed: {completed} | Interrupted: {interrupted}"
        ),
        String::new(),
        "RESIDENTS".into(),
    ];
    let headers = [
        "Name",
        "Location",
        "Activity / health",
        "$",
        "Mood",
        "M/S/R/Med",
        "Needs % Mn/F/E/S/C/St",
        "Urgent",
        "Goal",
        "Intention",
        "Strongest tie",
        "B/R",
    ]
    .map(str::to_owned);
    let rows = world
        .agents
        .values()
        .filter(|agent| agent.is_alive())
        .map(|agent| {
            let goal = agent
                .goals
                .iter()
                .find(|goal| goal.progress < goal.required)
                .or_else(|| agent.goals.first())
                .map(|goal| format!("{} ({}/{})", goal.description, goal.progress, goal.required))
                .unwrap_or_else(|| "—".into());
            let intention = agent
                .intention
                .as_ref()
                .map(|intention| intention_name(world, &intention.goal))
                .unwrap_or_else(|| "—".into());
            let (urgent_need, urgent_value) = most_urgent_need(agent);
            let conditions = agent
                .health_conditions()
                .into_iter()
                .map(|condition| format!("{condition:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join("+");
            let conditions = if conditions.is_empty() {
                "healthy".into()
            } else {
                conditions
            };
            [
                agent.name.clone(),
                location_name(world, agent.location),
                format!(
                    "{} · hp {}% · {conditions}",
                    agent
                        .activity
                        .map(|activity| format!("{:?}", activity.kind))
                        .unwrap_or_else(|| "Idle".into()),
                    (agent.health * 100.0).round() as u8,
                ),
                agent.balance.to_string(),
                format!("{:+.2}", agent.mood),
                format!(
                    "{}/{}/{}/{}",
                    agent.inventory.meals,
                    agent.inventory.supplies,
                    agent.inventory.repair_kits,
                    agent.inventory.medicine
                ),
                format!(
                    "{}/{}/{}/{}/{}/{}",
                    (agent.needs.money * 100.0).round() as u8,
                    (agent.needs.food * 100.0).round() as u8,
                    (agent.needs.energy * 100.0).round() as u8,
                    (agent.needs.safety * 100.0).round() as u8,
                    (agent.needs.companionship * 100.0).round() as u8,
                    (agent.needs.status * 100.0).round() as u8,
                ),
                format!("{urgent_need} {}%", (urgent_value * 100.0).round() as u8),
                goal,
                intention,
                strongest_relationship(world, agent),
                format!(
                    "{}/{}",
                    agent.beliefs.len(),
                    agent.rumors.iter().filter(|rumor| !rumor.resolved).count()
                ),
            ]
        })
        .collect::<Vec<_>>();
    let widths: [usize; 12] = std::array::from_fn(|column| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
            .max(headers[column].chars().count())
    });
    let render_row = |row: &[String; 12]| {
        row.iter()
            .enumerate()
            .map(|(column, cell)| {
                let width = widths[column];
                if matches!(column, 3 | 4 | 5 | 11) {
                    format!("{cell:>width$}")
                } else {
                    format!("{cell:<width$}")
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    lines.push(render_row(&headers));
    lines.extend(rows.iter().map(render_row));

    let deceased = world
        .agents
        .values()
        .filter_map(|agent| match agent.life {
            LifeState::Dead { tick, cause } => Some(format!(
                "{} — died from {} at {} in {}",
                agent.name,
                cause,
                tick,
                location_name(world, agent.location)
            )),
            LifeState::Alive => None,
        })
        .collect::<Vec<_>>();
    if !deceased.is_empty() {
        lines.extend([String::new(), "DECEASED".into()]);
        lines.extend(deceased);
    }

    lines.extend([
        String::new(),
        "BUSINESSES".into(),
        format!(
            "{:<20} {:<14} {:>6} {:>7} {:>8} {:>7} {:>6}",
            "Name", "Offering", "Cash", "Stock", "Revenue", "Wages", "Price"
        ),
    ]);
    for location in world.locations.values() {
        if let Some(business) = location.business {
            lines.push(format!(
                "{:<20} {:<14} {:>6} {:>7} {:>8} {:>7} {:>6}",
                clipped(&location.name, 20),
                business.offering,
                business.cash,
                business.stock,
                business.revenue,
                business.wages_paid,
                business.price,
            ));
        }
    }

    lines.extend([String::new(), "RECENT EVENTS".into()]);
    for event in world.events().iter().rev().take(8).rev() {
        lines.push(render_event(world, event));
    }
    lines.join("\n")
}

#[derive(Default)]
struct EventCounts {
    moves: usize,
    conversations: usize,
    confrontations: usize,
    purchases: usize,
    items_used: usize,
    rests: usize,
    work: usize,
    goals: usize,
    deaths: usize,
    treatments: usize,
    rejected: usize,
}

fn event_counts(world: &World) -> EventCounts {
    // ponytail: recount the small event log; cache counters if long live runs make redraws slow.
    let mut counts = EventCounts::default();
    for event in world.events() {
        match &event.kind {
            EventKind::Moved { .. } => counts.moves += 1,
            EventKind::Spoke { .. } => counts.conversations += 1,
            EventKind::Confronted { .. } => counts.confrontations += 1,
            EventKind::Purchased { .. } => counts.purchases += 1,
            EventKind::ItemUsed { .. } => counts.items_used += 1,
            EventKind::Rested { .. } => counts.rests += 1,
            EventKind::Worked { .. } => counts.work += 1,
            EventKind::GoalCompleted { .. } => counts.goals += 1,
            EventKind::Died { .. } => counts.deaths += 1,
            EventKind::Treated { .. } => counts.treatments += 1,
            EventKind::ActionRejected { .. } => counts.rejected += 1,
            EventKind::DiseaseInfected { .. }
            | EventKind::DiseaseSymptoms { .. }
            | EventKind::DiseaseRecovered { .. }
            | EventKind::DiseaseImmunityExpired { .. }
            | EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::Observed { .. }
            | EventKind::Waited { .. } => {}
        }
    }
    counts
}

fn routing_total(world: &World, field: impl Fn(&RoutingStats) -> u64) -> u64 {
    world
        .agents
        .values()
        .map(|agent| field(&agent.routing))
        .sum()
}

fn intention_counts(world: &World) -> (u64, u64, u64, u64) {
    (
        routing_total(world, |r| r.llm_intentions_started),
        routing_total(world, |r| r.llm_intention_steps),
        routing_total(world, |r| r.llm_intentions_completed),
        routing_total(world, |r| r.llm_intentions_interrupted),
    )
}

fn routing_counts(world: &World) -> (u64, u64, u64, u64) {
    let llm = routing_total(world, |r| r.llm_decisions);
    let fallbacks = routing_total(world, |r| r.llm_fallbacks);
    (
        routing_total(world, |r| r.local_decisions),
        llm + fallbacks,
        llm,
        fallbacks,
    )
}

pub fn render_summary(world: &World) -> String {
    let counts = event_counts(world);
    let (local, attempts, llm, fallbacks) = routing_counts(world);
    let (intentions, steps, completed, interrupted) = intention_counts(world);
    [
        "=== RUN SUMMARY ===".into(),
        format!("Elapsed: {}", world.tick),
        format!("Events: {}", world.events().len()),
        format!("Moves: {}", counts.moves),
        format!("Conversations: {}", counts.conversations),
        format!("Confrontations: {}", counts.confrontations),
        format!("Purchases: {}", counts.purchases),
        format!("Items used: {}", counts.items_used),
        format!("Rests: {}", counts.rests),
        format!("Work: {}", counts.work),
        format!("Goals completed: {}", counts.goals),
        format!("Treatments: {}", counts.treatments),
        format!("Deaths: {}", counts.deaths),
        format!("Rejected actions: {}", counts.rejected),
        format!("Local decisions: {local}"),
        format!("LLM attempts: {attempts}"),
        format!("LLM decisions: {llm}"),
        format!("LLM fallbacks: {fallbacks}"),
        format!("LLM intentions started: {intentions}"),
        format!("Local intention steps: {steps}"),
        format!("LLM intentions completed: {completed}"),
        format!("LLM intentions interrupted: {interrupted}"),
    ]
    .join("\n")
}

pub fn render_event(world: &World, event: &Event) -> String {
    let time = format!("{:02}:{:02}", event.tick.hour(), event.tick.minute());
    match &event.kind {
        EventKind::TownEventStarted { kind, ends_at } => {
            format!("{time}  The {kind} began (until {ends_at})")
        }
        EventKind::TownEventEnded { kind } => format!("{time}  The {kind} ended"),
        EventKind::Moved { agent, to, .. } => format!(
            "{time}  {} entered {}",
            agent_name(world, *agent),
            location_name(world, *to)
        ),
        EventKind::Spoke {
            speaker,
            listener,
            tone,
            message,
        } => format!(
            "{time}  {} spoke to {} [{tone}]\n       \"{}\"",
            agent_name(world, *speaker),
            agent_name(world, *listener),
            message
        ),
        EventKind::Confronted {
            accuser,
            target,
            outcome,
            ..
        } => format!(
            "{time}  {} confronted {}: {outcome}",
            agent_name(world, *accuser),
            agent_name(world, *target)
        ),
        EventKind::Observed { observer, target } => format!(
            "{time}  {} observed {}",
            agent_name(world, *observer),
            target_name(world, target)
        ),
        EventKind::Purchased {
            agent,
            offering,
            cost,
        } => format!(
            "{time}  {} bought {offering} for {cost} coins",
            agent_name(world, *agent)
        ),
        EventKind::ItemUsed { agent, item } => {
            format!("{time}  {} used a {item}", agent_name(world, *agent))
        }
        EventKind::Treated { agent, cost } => format!(
            "{time}  {} received treatment for {cost} coins",
            agent_name(world, *agent)
        ),
        EventKind::Rested { agent } => format!("{time}  {} rested", agent_name(world, *agent)),
        EventKind::Worked {
            agent,
            wage,
            stock_produced,
        } => format!(
            "{time}  {} worked and earned {wage} coins{}",
            agent_name(world, *agent),
            if *stock_produced == 0 {
                String::new()
            } else {
                format!(" and produced {stock_produced} stock")
            }
        ),
        EventKind::GoalCompleted { agent, goal } => format!(
            "{time}  {} completed goal: {goal}",
            agent_name(world, *agent)
        ),
        EventKind::Waited { agent } => format!("{time}  {} waited", agent_name(world, *agent)),
        EventKind::Died { agent, cause } => {
            format!("{time}  {} died from {cause}", agent_name(world, *agent))
        }
        EventKind::DiseaseInfected { agent, source } => format!(
            "{time}  {} became infected{}",
            agent_name(world, *agent),
            source
                .map(|source| format!(" after contact with {}", agent_name(world, source)))
                .unwrap_or_default()
        ),
        EventKind::DiseaseSymptoms { agent } => format!(
            "{time}  {} developed Briar fever symptoms",
            agent_name(world, *agent)
        ),
        EventKind::DiseaseRecovered { agent } => format!(
            "{time}  {} started recovering from Briar fever",
            agent_name(world, *agent)
        ),
        EventKind::DiseaseImmunityExpired { agent } => format!(
            "{time}  {}'s Briar fever immunity expired",
            agent_name(world, *agent)
        ),
        EventKind::ActionRejected { agent, reason } => format!(
            "{time}  {}'s action was rejected: {reason}",
            agent_name(world, *agent)
        ),
    }
}

fn most_urgent_need(agent: &Agent) -> (&'static str, f32) {
    [
        ("money", agent.needs.money),
        ("food", agent.needs.food),
        ("company", agent.needs.companionship),
        ("safety", agent.needs.safety),
        ("status", agent.needs.status),
        ("energy", agent.needs.energy),
    ]
    .into_iter()
    .min_by(|left, right| left.1.total_cmp(&right.1))
    .expect("resident has needs")
}

fn strongest_relationship(world: &World, agent: &Agent) -> String {
    agent
        .relationships
        .iter()
        .max_by(|left, right| left.1.score().total_cmp(&right.1.score()))
        .map(|(id, relationship)| {
            format!("{} {:+.1}", agent_name(world, *id), relationship.score())
        })
        .unwrap_or_else(|| "—".into())
}

fn intention_name(world: &World, goal: &IntentionGoal) -> String {
    match goal {
        IntentionGoal::Visit { destination } => {
            format!("visit {}", location_name(world, *destination))
        }
        IntentionGoal::Purchase { destination } => {
            format!("buy at {}", location_name(world, *destination))
        }
        IntentionGoal::Rest => "rest".into(),
        IntentionGoal::Work => "work".into(),
        IntentionGoal::SeekTreatment => "seek treatment".into(),
        IntentionGoal::Talk { target, .. } => format!("talk to {}", agent_name(world, *target)),
        IntentionGoal::Observe { target } => format!("observe {}", target_name(world, target)),
        IntentionGoal::Confront { target, .. } => {
            format!("confront {}", agent_name(world, *target))
        }
    }
}

fn clipped(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        value.into()
    } else {
        value.chars().take(width - 1).collect::<String>() + "…"
    }
}

fn target_name(world: &World, target: &ObservationTarget) -> String {
    match target {
        ObservationTarget::Agent(id) => agent_name(world, *id),
        ObservationTarget::Location(id) => location_name(world, *id),
    }
}

fn agent_name(world: &World, id: AgentId) -> String {
    world
        .agents
        .get(&id)
        .map(|agent| agent.name.clone())
        .unwrap_or_else(|| id.to_string())
}

fn location_name(world: &World, id: LocationId) -> String {
    world
        .locations
        .get(&id)
        .map(|location| location.name.clone())
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::{render_dashboard, render_event, render_run, render_run_since};
    use crate::{
        decision::RandomDecisionEngine,
        runner::run_simulation,
        sim::{Tick, World},
    };

    #[test]
    fn dashboard_renders_active_town_events() {
        let mut world = World::briar_glen(0).expect("town");
        world
            .advance_to(Tick(8 * 60 / Tick::MINUTES))
            .expect("storm");
        assert!(render_dashboard(&world).contains("Town event: storm (72 ticks remaining)"));
        assert!(
            render_event(&world, world.events().last().expect("start")).contains("storm began")
        );
    }

    #[test]
    fn dashboard_shows_full_resident_finances_needs_and_inventory() {
        let mut world = World::briar_glen(9).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let agent = world.agents.get_mut(&actor).expect("resident");
        let name = agent.name.clone();
        agent.balance = 37;
        agent.mood = -0.25;
        agent.inventory.meals = 2;
        agent.inventory.supplies = 1;
        agent.inventory.repair_kits = 3;
        agent.needs.money = 0.11;
        agent.needs.food = 0.22;
        agent.needs.energy = 0.33;
        agent.needs.safety = 0.44;
        agent.needs.companionship = 0.55;
        agent.needs.status = 0.66;

        let dashboard = render_dashboard(&world);
        let row = dashboard
            .lines()
            .find(|line| line.starts_with(&name))
            .expect("resident row");
        assert!(dashboard.contains("Needs % Mn/F/E/S/C/St"));
        assert!(dashboard.contains("Goal"));
        assert!(dashboard.contains("Intention"));
        assert!(dashboard.contains("Strongest tie"));
        assert!(row.contains("37"));
        assert!(row.contains("-0.25"));
        assert!(row.contains("2/1/3"));
        assert!(row.contains("11/22/33/44/55/66"));
    }

    #[tokio::test]
    async fn rendering_is_deterministic_and_readable() {
        let world = World::briar_glen(42).expect("town");
        let mut engine = RandomDecisionEngine::new(42);
        let world = run_simulation(world, 20, &mut engine)
            .await
            .expect("simulation");
        let rendered = render_run(&world);
        assert_eq!(rendered, render_run(&world));
        assert!(rendered.contains("Briar Glen\nSeed: 42\nAgents: 8"));
        assert!(rendered.contains("=== RUN SUMMARY ==="));
        assert!(rendered.contains("Items used:"));
        assert!(rendered.contains("used a meal"));

        let resumed = render_run_since(&world, 10);
        assert!(!resumed.contains("07:05  "));
        assert!(resumed.contains(&render_event(&world, &world.events()[10])));
        assert!(resumed.contains(&format!("Events: {}", world.events().len())));

        let dashboard = render_dashboard(&world);
        assert!(dashboard.contains("=== Briar Glen — Day 1"));
        assert!(dashboard.contains("RESIDENTS"));
        assert!(dashboard.contains("Needs % Mn/F/E/S/C/St"));
        assert!(dashboard.contains("M/S/R"));
        assert!(dashboard.contains("LLM attempts: 0"));
        assert!(dashboard.contains("Goal"));
        assert!(dashboard.contains("Intention"));
        assert!(dashboard.contains("Strongest tie"));
        assert!(dashboard.contains("BUSINESSES"));
        assert!(dashboard.contains("Revenue"));
        assert!(dashboard.contains("Wages"));
        assert!(dashboard.contains(&world.agents.values().next().expect("resident").name));
        assert!(dashboard.contains("RECENT EVENTS"));
    }
}
