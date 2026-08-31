use super::*;

#[test]
fn work_and_activity_locations_are_authoritative() {
    let mut world = World::briar_glen(3).expect("town");
    let actor = *world
        .agents
        .values()
        .find(|agent| agent.workplace.is_some())
        .map(|agent| &agent.id)
        .expect("worker");
    let workplace = world.agents[&actor].workplace.expect("workplace");
    world.advance_to(Tick(8 * 12)).expect("morning");
    assert!(matches!(
        world.execute(
            actor,
            ProposedAction::Move {
                destination: workplace
            }
        ),
        ActionResult::Success(_)
    ));

    let needs = world.agents[&actor].needs.clone();
    assert!(matches!(
        world.execute(actor, ProposedAction::Work),
        ActionResult::Success(_)
    ));
    assert!(world.agents[&actor].needs.money > needs.money);
    assert!(matches!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Success(_)
    ));
    assert_eq!(
        world.execute(actor, ProposedAction::Rest),
        ActionResult::Rejected(ActionRejection::CannotRestHere(workplace))
    );

    world.advance_to(Tick(18 * 12)).expect("evening");
    assert_eq!(
        world.execute(actor, ProposedAction::Work),
        ActionResult::Rejected(ActionRejection::LocationClosed(workplace))
    );
}

#[test]
fn town_identity_is_seeded() {
    assert_eq!(
        World::briar_glen(7).expect("town").agents,
        World::briar_glen(7).expect("town").agents
    );
    assert_ne!(
        World::briar_glen(7).expect("town").agents.keys().next(),
        World::briar_glen(8).expect("town").agents.keys().next()
    );
}

#[test]
fn closed_locations_reject_entry_and_activity_but_allow_departure() {
    let mut world = World::briar_glen(5).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let home = world.agents[&actor].home;
    let tavern = world
        .locations
        .values()
        .find(|location| location.name == "The Crooked Lantern")
        .map(|location| location.id)
        .expect("tavern");

    assert_eq!(
        world.execute(
            actor,
            ProposedAction::Move {
                destination: tavern,
            }
        ),
        ActionResult::Rejected(ActionRejection::LocationClosed(tavern))
    );

    world.advance_to(Tick(12 * 12)).expect("opening time");
    assert!(matches!(
        world.execute(
            actor,
            ProposedAction::Move {
                destination: tavern,
            }
        ),
        ActionResult::Success(_)
    ));
    world.advance_to(Tick(23 * 12)).expect("closing time");
    assert_eq!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Rejected(ActionRejection::LocationClosed(tavern))
    );
    assert!(matches!(
        world.execute(actor, ProposedAction::Move { destination: home }),
        ActionResult::Success(_)
    ));
}

#[test]
fn movement_updates_both_sides_and_records_an_event() {
    let mut world = World::briar_glen(4).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let from = world.agents[&actor].location;
    let destination = *world.locations[&from]
        .connected
        .iter()
        .find(|id| world.locations[id].is_open(world.tick.hour()))
        .expect("open connected location");

    assert!(matches!(
        world.execute(actor, ProposedAction::Move { destination }),
        ActionResult::Success(_)
    ));
    assert_eq!(world.agents[&actor].location, destination);
    assert!(!world.locations[&from].agents.contains(&actor));
    assert!(world.locations[&destination].agents.contains(&actor));
    assert!(matches!(world.events[0].kind, EventKind::Moved { .. }));
    world
        .validate()
        .expect("movement should preserve invariants");
}

#[test]
fn only_present_agents_remember_a_conversation() {
    let mut world = World::briar_glen(41).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let remote = residents[0];
    let speaker = residents[1];
    let listener = residents[2];
    let home = world.agents[&remote].location;
    let destination = *world.locations[&home]
        .connected
        .iter()
        .find(|id| world.locations[id].is_open(world.tick.hour()))
        .expect("open destination");
    world.execute(remote, ProposedAction::Move { destination });
    for agent in world.agents.values_mut() {
        agent.memories.clear();
    }

    world.execute(
        speaker,
        ProposedAction::Talk {
            target: listener,
            tone: DialogueTone::Neutral,
            message: "Did you hear the bells?".into(),
        },
    );

    assert!(world.agents[&remote].memories.is_empty());
    assert!(world.agents[&remote].beliefs.is_empty());
    assert!(!world.agents[&speaker].beliefs.contains_key(&speaker));
    let listener_belief = world.agents[&listener].beliefs[&speaker];
    assert!(listener_belief.sociability > 0.5);
    assert_eq!(listener_belief.confidence, 0.15);
    assert!(
        world.agents[&speaker]
            .memories
            .iter()
            .any(|memory| matches!(memory.kind, EventKind::Spoke { .. }))
    );
    assert!(
        world.agents[&residents[3]]
            .memories
            .iter()
            .any(|memory| matches!(memory.kind, EventKind::Spoke { .. }))
    );

    world.append_event(
        Some(home),
        EventKind::Worked {
            agent: speaker,
            wage: WORK_WAGE,
            stock_produced: 0,
        },
    );
    let belief = world.agents[&listener].beliefs[&speaker];
    assert!(belief.reliability > 0.5);
    assert_eq!(belief.confidence, 0.3);
    world
        .advance_to(Tick(world.tick.0 + 10))
        .expect("time advances");
    assert!((world.agents[&listener].beliefs[&speaker].confidence - 0.298).abs() < f32::EPSILON);
    world.validate().expect("valid beliefs");

    world
        .agents
        .get_mut(&listener)
        .expect("listener")
        .beliefs
        .get_mut(&speaker)
        .expect("belief")
        .confidence = 1.1;
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn conversations_propagate_bounded_degrading_rumors() {
    let mut world = World::briar_glen(42).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let subject = residents[0];
    let first_listener = residents[2];
    let second_listener = residents[3];
    let fact = world.append_event(
        None,
        EventKind::Worked {
            agent: subject,
            wage: WORK_WAGE,
            stock_produced: 0,
        },
    );

    world.execute(
        subject,
        ProposedAction::Talk {
            target: first_listener,
            tone: DialogueTone::Neutral,
            message: "Work went well today.".into(),
        },
    );
    let rumor = &world.agents[&first_listener].rumors[0];
    assert_eq!(rumor.event, fact);
    assert_eq!(rumor.source, subject);
    assert_eq!(rumor.depth, 1);
    assert!(rumor.confidence > 0.0 && rumor.confidence <= 1.0);
    assert!(world.agents[&first_listener].beliefs[&subject].reliability > 0.5);

    let first_confidence = rumor.confidence;
    world.execute(
        subject,
        ProposedAction::Talk {
            target: first_listener,
            tone: DialogueTone::Neutral,
            message: "As I was saying.".into(),
        },
    );
    assert_eq!(world.agents[&first_listener].rumors.len(), 1);

    world.execute(
        first_listener,
        ProposedAction::Talk {
            target: second_listener,
            tone: DialogueTone::Neutral,
            message: "I heard work went well.".into(),
        },
    );
    let retelling = world.agents[&second_listener]
        .rumors
        .iter()
        .find(|rumor| rumor.event.id == fact.id)
        .expect("retold rumor");
    assert_eq!(retelling.source, first_listener);
    assert_eq!(retelling.depth, 2);
    assert!(retelling.confidence < first_confidence);
    let retelling_confidence = retelling.confidence;
    world.validate().expect("valid rumors");

    world
        .agents
        .get_mut(&second_listener)
        .expect("listener")
        .rumors[0]
        .confidence = 1.1;
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    world
        .agents
        .get_mut(&second_listener)
        .expect("listener")
        .rumors[0]
        .confidence = retelling_confidence;
    world
        .agents
        .get_mut(&second_listener)
        .expect("listener")
        .rumors[0]
        .event
        .id = crate::sim::EventId(Uuid::nil());
    assert!(matches!(
        world.validate_history(),
        Err(WorldError::InvalidState(_))
    ));
}
