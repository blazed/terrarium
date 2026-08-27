use crate::sim::{AgentId, Event, EventKind, LocationId, ObservationTarget, World};

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

pub fn render_summary(world: &World) -> String {
    let count = |matches: fn(&EventKind) -> bool| {
        world
            .events()
            .iter()
            .filter(|event| matches(&event.kind))
            .count()
    };
    let moves = count(|kind| matches!(kind, EventKind::Moved { .. }));
    let conversations = count(|kind| matches!(kind, EventKind::Spoke { .. }));
    let confrontations = count(|kind| matches!(kind, EventKind::Confronted { .. }));
    let meals = count(|kind| matches!(kind, EventKind::Ate { .. }));
    let rests = count(|kind| matches!(kind, EventKind::Rested { .. }));
    let work = count(|kind| matches!(kind, EventKind::Worked { .. }));
    let goals = count(|kind| matches!(kind, EventKind::GoalCompleted { .. }));
    let rejected = count(|kind| matches!(kind, EventKind::ActionRejected { .. }));
    [
        "=== RUN SUMMARY ===".into(),
        format!("Elapsed: {}", world.tick),
        format!("Events: {}", world.events().len()),
        format!("Moves: {moves}"),
        format!("Conversations: {conversations}"),
        format!("Confrontations: {confrontations}"),
        format!("Meals: {meals}"),
        format!("Rests: {rests}"),
        format!("Work: {work}"),
        format!("Goals completed: {goals}"),
        format!("Rejected actions: {rejected}"),
    ]
    .join("\n")
}

pub fn render_event(world: &World, event: &Event) -> String {
    let time = format!("{:02}:{:02}", event.tick.hour(), event.tick.minute());
    match &event.kind {
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
        EventKind::Ate { agent } => format!("{time}  {} ate", agent_name(world, *agent)),
        EventKind::Rested { agent } => format!("{time}  {} rested", agent_name(world, *agent)),
        EventKind::Worked { agent } => format!("{time}  {} worked", agent_name(world, *agent)),
        EventKind::GoalCompleted { agent, goal } => format!(
            "{time}  {} completed goal: {goal}",
            agent_name(world, *agent)
        ),
        EventKind::Waited { agent } => format!("{time}  {} waited", agent_name(world, *agent)),
        EventKind::ActionRejected { agent, reason } => format!(
            "{time}  {}'s action was rejected: {reason}",
            agent_name(world, *agent)
        ),
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
    use super::{render_event, render_run, render_run_since};
    use crate::{decision::RandomDecisionEngine, runner::run_simulation, sim::World};

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

        let resumed = render_run_since(&world, 10);
        assert!(!resumed.contains("07:05  "));
        assert!(resumed.contains(&render_event(&world, &world.events()[10])));
        assert!(resumed.contains(&format!("Events: {}", world.events().len())));
    }
}
