use crate::{
    observer::render_event,
    sim::{ConfrontationOutcome, DialogueTone, Event, EventId, EventKind, World},
};
use std::collections::{BTreeMap, BTreeSet};

pub fn render_chronicle(world: &World, all: bool) -> String {
    let mut days = BTreeMap::<u64, Vec<&Event>>::new();
    for event in world.events() {
        days.entry(event.tick.day()).or_default().push(event);
    }

    // ponytail: checkpoints do not retain rumor acquisition events; count surviving claims
    // on their claim day until propagation is persisted.
    let mut rumors = BTreeMap::<u64, BTreeSet<EventId>>::new();
    for rumor in world.agents.values().flat_map(|agent| &agent.rumors) {
        rumors
            .entry(rumor.event.tick.day())
            .or_default()
            .insert(rumor.event.id);
    }

    let mut lines = vec![
        format!("{} Chronicle", world.name),
        format!("Seed: {}", world.seed),
    ];
    for (day, events) in days {
        let deaths = events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Died { .. }))
            .count();
        let town_events = events
            .iter()
            .filter_map(|event| match event.kind {
                EventKind::TownEventStarted { kind, .. } => Some(format!("{kind} began")),
                EventKind::TownEventEnded { kind } => Some(format!("{kind} ended")),
                _ => None,
            })
            .collect::<Vec<_>>();
        let relationship = events
            .iter()
            .filter_map(|event| relationship_significance(event).map(|score| (score, *event)))
            .max_by_key(|(score, _)| *score)
            .map_or_else(
                || "none".to_owned(),
                |(_, event)| {
                    render_event(world, event)
                        .lines()
                        .next()
                        .and_then(|line| line.split_once("  ").map(|(_, description)| description))
                        .unwrap_or("none")
                        .to_owned()
                },
            );
        lines.extend([
            String::new(),
            format!("=== Day {day} ==="),
            format!(
                "Summary: deaths: {deaths} | new rumors: {} | town event: {} | biggest relationship change: {relationship}",
                rumors.get(&day).map_or(0, BTreeSet::len),
                if town_events.is_empty() {
                    "none".into()
                } else {
                    town_events.join(", ")
                }
            ),
        ]);
        lines.extend(
            events
                .into_iter()
                .filter(|event| all || !is_hidden(&event.kind))
                .map(|event| render_event(world, event)),
        );
    }
    lines.join("\n")
}

fn is_hidden(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Waited { .. } | EventKind::Observed { .. } | EventKind::ActionRejected { .. }
    )
}

// ponytail: event history has no relationship deltas; rank social event types until
// exact deltas are persisted.
fn relationship_significance(event: &Event) -> Option<u8> {
    match event.kind {
        EventKind::Spoke { tone, .. } => Some(match tone {
            DialogueTone::Neutral => 1,
            DialogueTone::Friendly => 2,
            DialogueTone::Supportive => 3,
            DialogueTone::Tense => 4,
        }),
        EventKind::Confronted { outcome, .. } => Some(match outcome {
            ConfrontationOutcome::Confirmed => 2,
            ConfrontationOutcome::Challenged => 3,
            ConfrontationOutcome::Denied => 4,
        }),
        EventKind::ItemGiven { .. } => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::render_chronicle;
    use crate::sim::{ProposedAction, Tick, World};

    #[test]
    fn chronicle_groups_days_summarizes_and_filters_routine_events() {
        let mut world = World::briar_glen(413).expect("town");
        let residents = world.agents.keys().copied().take(2).collect::<Vec<_>>();
        world.execute(residents[0], ProposedAction::Wait);
        world.tick = Tick(world.tick.0 + Tick::PER_DAY);
        world.execute(residents[1], ProposedAction::Wait);

        let chronicle = render_chronicle(&world, false);
        assert!(chronicle.contains("=== Day 1 ==="));
        assert!(chronicle.contains("=== Day 2 ==="));
        assert!(chronicle.contains(
            "Summary: deaths: 0 | new rumors: 0 | town event: none | biggest relationship change:"
        ));
        assert!(!chronicle.contains(" waited"));
        assert!(render_chronicle(&world, true).contains(" waited"));
    }
}
