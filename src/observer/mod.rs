use crate::sim::{AgentId, Event, EventKind, LocationId, ObservationTarget, World};

pub fn render_run(world: &World) -> String {
    let mut lines = vec![
        world.name.clone(),
        format!("Seed: {}", world.seed),
        format!("Agents: {}", world.agents.len()),
        String::new(),
    ];
    let mut last_day = 0;
    for event in world.events() {
        if event.tick.day() != last_day {
            last_day = event.tick.day();
            lines.push(format!("Day {last_day}"));
            lines.push(String::new());
        }
        lines.push(render_event(world, event));
    }

    let moves = world
        .events()
        .iter()
        .filter(|event| matches!(event.kind, EventKind::Moved { .. }))
        .count();
    let conversations = world
        .events()
        .iter()
        .filter(|event| matches!(event.kind, EventKind::Spoke { .. }))
        .count();
    let rejected = world
        .events()
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ActionRejected { .. }))
        .count();
    lines.extend([
        String::new(),
        "=== RUN SUMMARY ===".into(),
        format!("Elapsed: {}", world.tick),
        format!("Events: {}", world.events().len()),
        format!("Moves: {moves}"),
        format!("Conversations: {conversations}"),
        format!("Rejected actions: {rejected}"),
    ]);
    lines.join("\n")
}

fn render_event(world: &World, event: &Event) -> String {
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
            message,
        } => format!(
            "{time}  {} spoke to {}\n       \"{}\"",
            agent_name(world, *speaker),
            agent_name(world, *listener),
            message
        ),
        EventKind::Observed { observer, target } => format!(
            "{time}  {} observed {}",
            agent_name(world, *observer),
            target_name(world, target)
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
    use super::render_run;
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
    }
}
