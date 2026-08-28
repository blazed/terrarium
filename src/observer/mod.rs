use crate::sim::{
    Agent, AgentId, Event, EventKind, IntentionGoal, LocationId, ObservationTarget, World,
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
    // ponytail: recount the small event log; cache counters if long live runs make redraws slow.
    let mut counts = [0; 8];
    for event in world.events() {
        let index = match &event.kind {
            EventKind::Moved { .. } => 0,
            EventKind::Spoke { .. } => 1,
            EventKind::Confronted { .. } => 2,
            EventKind::Purchased { .. } => 3,
            EventKind::Rested { .. } => 4,
            EventKind::Worked { .. } => 5,
            EventKind::GoalCompleted { .. } => 6,
            EventKind::ActionRejected { .. } => 7,
            EventKind::Observed { .. } | EventKind::Waited { .. } => continue,
        };
        counts[index] += 1;
    }

    let mut lines = vec![
        format!("=== {} — {} ===", world.name, world.tick),
        format!(
            "Events: {} | Moves: {} | Talks: {} | Confrontations: {} | Rejected: {}",
            world.events().len(),
            counts[0],
            counts[1],
            counts[2],
            counts[7]
        ),
        format!(
            "Purchases: {} | Rests: {} | Work: {} | Goals completed: {}",
            counts[3], counts[4], counts[5], counts[6]
        ),
        String::new(),
        "RESIDENTS".into(),
        format!(
            "{:<12} {:<14} {:<9} {:>6} {:>5} {:<12} {:<20} {:<14} {:<15} {}",
            "Name",
            "Location",
            "Activity",
            "Mood",
            "Coins",
            "Urgent",
            "Goal",
            "Intention",
            "Strongest tie",
            "B/R"
        ),
    ];
    for agent in world.agents.values() {
        let location = location_name(world, agent.location);
        let activity = agent
            .activity
            .map(|activity| format!("{:?}", activity.kind))
            .unwrap_or_else(|| "Idle".into());
        let (need, value) = most_urgent_need(agent);
        let goal = agent
            .goals
            .first()
            .map(|goal| format!("{} [{}/{}]", goal.description, goal.progress, goal.required))
            .unwrap_or_else(|| "—".into());
        let intention = agent
            .intention
            .as_ref()
            .map(|intention| intention_name(world, &intention.goal))
            .unwrap_or_else(|| "—".into());
        let relationship = strongest_relationship(world, agent);
        let rumors = agent.rumors.iter().filter(|rumor| !rumor.resolved).count();
        lines.push(format!(
            "{:<12} {:<14} {:<9} {:>+6.2} {:>5} {:<12} {:<20} {:<14} {:<15} {}/{}",
            clipped(&agent.name, 12),
            clipped(&location, 14),
            activity,
            agent.mood,
            agent.balance,
            format!("{need} {}%", (value * 100.0).round() as u8),
            clipped(&goal, 20),
            clipped(&intention, 14),
            clipped(&relationship, 15),
            agent.beliefs.len(),
            rumors,
        ));
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
        lines.push(render_event(world, event).replace('\n', " / "));
    }
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
    let purchases = count(|kind| matches!(kind, EventKind::Purchased { .. }));
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
        format!("Purchases: {purchases}"),
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
        EventKind::Purchased {
            agent,
            offering,
            cost,
        } => format!(
            "{time}  {} bought {offering} for {cost} coins",
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
        EventKind::ActionRejected { agent, reason } => format!(
            "{time}  {}'s action was rejected: {reason}",
            agent_name(world, *agent)
        ),
    }
}

fn strongest_relationship(world: &World, agent: &Agent) -> String {
    agent
        .relationships
        .iter()
        .max_by(|left, right| {
            let score = |relationship: &crate::sim::Relationship| {
                relationship.affection + relationship.trust + relationship.respect
                    - relationship.suspicion
            };
            score(left.1).total_cmp(&score(right.1))
        })
        .map(|(id, relationship)| {
            format!(
                "{} {:+.1}",
                agent_name(world, *id),
                relationship.affection + relationship.trust + relationship.respect
                    - relationship.suspicion
            )
        })
        .unwrap_or_else(|| "—".into())
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
        IntentionGoal::Talk { target, .. } => format!("talk to {}", agent_name(world, *target)),
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

        let dashboard = render_dashboard(&world);
        assert!(dashboard.contains("=== Briar Glen — Day 1"));
        assert!(dashboard.contains("RESIDENTS"));
        assert!(dashboard.contains("Strongest tie"));
        assert!(dashboard.contains("Coins"));
        assert!(dashboard.contains("B/R"));
        assert!(dashboard.contains("BUSINESSES"));
        assert!(dashboard.contains("Revenue"));
        assert!(dashboard.contains("Wages"));
        assert!(dashboard.contains(&world.agents.values().next().expect("resident").name));
        assert!(dashboard.contains("RECENT EVENTS"));
    }
}
