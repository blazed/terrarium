use super::*;

#[test]
fn event_log_keeps_insertion_order() {
    let mut world = World::from_spec(BRIAR_GLEN, 8).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let start = world.tick.0;
    world.advance_tick().expect("tick");
    world.execute(actor, ProposedAction::Wait);
    world.advance_tick().expect("tick");
    world.execute(actor, ProposedAction::Wait);

    assert_eq!(
        world
            .events()
            .iter()
            .map(|event| event.tick)
            .collect::<Vec<_>>(),
        vec![Tick(start + 1), Tick(start + 2)]
    );
    assert_ne!(world.events()[0].id, world.events()[1].id);
}

#[test]
fn unknown_actor_is_rejected_without_panicking() {
    let mut world = World::from_spec(BRIAR_GLEN, 7).expect("town");
    let unknown = AgentId(Uuid::nil());
    assert_eq!(
        world.execute(unknown, ProposedAction::Wait),
        ActionResult::Rejected(ActionRejection::UnknownActor(unknown))
    );
}

#[test]
fn critical_needs_damage_health_and_repair_recovers_injury() {
    let mut world = World::from_spec(BRIAR_GLEN, 19).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.health = 0.5;
    agent.needs.food = 0.0;
    agent.needs.energy = 1.0;
    agent.needs.safety = 0.0;
    agent.inventory.repair_kits = 1;
    world.advance_to(Tick(world.tick.0 + 100)).expect("advance");
    assert!(world.agents[&actor].health < 0.5);
    assert!(world.agents[&actor].injury);
    world.execute(actor, ProposedAction::UseRepairKit);
    assert!(world.agents[&actor].health > 0.5);
    assert!(!world.agents[&actor].injury);
}

#[test]
fn health_zero_emits_one_death_and_removes_membership() {
    let mut world = World::from_spec(BRIAR_GLEN, 20).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let other = *world.agents.keys().find(|id| **id != actor).expect("other");
    let location = world.agents[&actor].location;
    world.agents.get_mut(&other).expect("other").intention = Some(Intention {
        goal: IntentionGoal::Talk {
            target: actor,
            tone: DialogueTone::Supportive,
            message: "How are you?".into(),
        },
        expires_at: Tick(world.tick.0 + 10),
    });
    world.agents.get_mut(&actor).expect("resident").health = 0.000001;
    world.agents.get_mut(&actor).expect("resident").needs.food = 0.0;
    world.advance_tick().expect("advance");
    assert!(matches!(
        world.agents[&actor].life,
        LifeState::Dead {
            cause: DeathCause::Starvation,
            ..
        }
    ));
    assert!(!world.locations[&location].agents.contains(&actor));
    assert!(world.agents[&other].intention.is_none());
    assert_eq!(
        world
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Died { agent, .. } if agent == actor))
            .count(),
        1
    );
    assert_eq!(
        world.execute(actor, ProposedAction::Wait),
        ActionResult::Rejected(ActionRejection::AgentDead(actor))
    );
    world.validate().expect("valid dead resident history");
}

#[test]
fn briar_fever_is_deterministic_and_infection_is_hidden_from_memories() {
    let mut world = World::from_spec(BRIAR_GLEN, 1).expect("town");
    world
        .advance_to(Tick(PATIENT_ZERO_TICK))
        .expect("patient zero");
    let patient_zero = world
        .events()
        .iter()
        .find_map(|event| match event.kind {
            EventKind::DiseaseInfected {
                agent,
                source: None,
            } => Some(agent),
            _ => None,
        })
        .expect("patient zero event");
    assert!(matches!(
        world.agents[&patient_zero].disease,
        DiseaseState::Incubating { .. }
    ));
    assert!(world.agents.values().all(|agent| {
        agent
            .memories
            .iter()
            .all(|event| !matches!(event.kind, EventKind::DiseaseInfected { .. }))
    }));

    world
        .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
        .expect("symptoms");
    assert!(matches!(
        world.agents[&patient_zero].disease,
        DiseaseState::Symptomatic { .. }
    ));
    assert!(world.events().iter().any(|event| matches!(
        event.kind,
        EventKind::DiseaseSymptoms { agent } if agent == patient_zero
    )));
    assert!(world.events().iter().any(|event| matches!(
        event.kind,
        EventKind::DiseaseInfected {
            source: Some(source),
            ..
        } if source == patient_zero
    )));
}

#[test]
fn briar_fever_recovers_and_immunity_expires() {
    let mut world = World::from_spec(BRIAR_GLEN, 1).expect("town");
    world
        .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
        .expect("symptoms");
    let patient_zero = world
        .events()
        .iter()
        .find_map(|event| match event.kind {
            EventKind::DiseaseInfected {
                agent,
                source: None,
            } => Some(agent),
            _ => None,
        })
        .expect("patient zero");
    world
        .advance_to(Tick(
            PATIENT_ZERO_TICK + INCUBATION_TICKS + SYMPTOMATIC_TICKS,
        ))
        .expect("recovery");
    assert!(matches!(
        world.agents[&patient_zero].disease,
        DiseaseState::Recovering { .. }
    ));
    world
        .advance_to(Tick(
            PATIENT_ZERO_TICK + INCUBATION_TICKS + SYMPTOMATIC_TICKS + RECOVERY_TICKS,
        ))
        .expect("immunity");
    assert!(matches!(
        world.agents[&patient_zero].disease,
        DiseaseState::Immune { .. }
    ));
    world
        .advance_to(Tick(
            PATIENT_ZERO_TICK
                + INCUBATION_TICKS
                + SYMPTOMATIC_TICKS
                + RECOVERY_TICKS
                + IMMUNITY_TICKS,
        ))
        .expect("immunity expiry");
    assert_eq!(
        world.agents[&patient_zero].disease,
        DiseaseState::Susceptible
    );
    assert!(world.events().iter().any(|event| matches!(
        event.kind,
        EventKind::DiseaseImmunityExpired { agent } if agent == patient_zero
    )));
}

#[test]
fn briar_fever_only_spreads_between_co_located_residents() {
    let mut world = World::from_spec(BRIAR_GLEN, 1).expect("town");
    world
        .advance_to(Tick(PATIENT_ZERO_TICK))
        .expect("infection");
    let patient_zero = world
        .events()
        .iter()
        .find_map(|event| match event.kind {
            EventKind::DiseaseInfected {
                agent,
                source: None,
            } => Some(agent),
            _ => None,
        })
        .expect("patient zero");
    world
        .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
        .expect("symptoms");
    let target = world
        .agents
        .values()
        .find(|agent| {
            agent.id != patient_zero && matches!(agent.disease, DiseaseState::Susceptible)
        })
        .expect("susceptible target")
        .id;
    let from = world.agents[&target].location;
    let to = *world.locations[&from]
        .connected
        .iter()
        .next()
        .expect("neighbor");
    world
        .locations
        .get_mut(&from)
        .expect("from")
        .agents
        .remove(&target);
    world
        .locations
        .get_mut(&to)
        .expect("to")
        .agents
        .insert(target);
    world.agents.get_mut(&target).expect("target").location = to;
    world.validate().expect("separated residents");
    let infection_count = world
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::DiseaseInfected { agent, source: Some(_ ) } if agent == target))
            .count();
    world
        .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS + 12))
        .expect("transmission window");
    assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::DiseaseInfected { agent, source: Some(_) } if agent == target))
                .count(),
            infection_count
        );
}

#[test]
fn symptomatic_disease_can_cause_death_once() {
    let mut world = World::from_spec(BRIAR_GLEN, 22).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let location = world.agents[&actor].location;
    let now = world.tick;
    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.health = 0.0005;
    agent.needs.food = 1.0;
    agent.needs.energy = 1.0;
    agent.needs.safety = 1.0;
    agent.disease = DiseaseState::Symptomatic {
        until: Tick(now.0 + 10),
    };
    world.advance_tick().expect("disease damage");
    assert!(matches!(
        world.agents[&actor].life,
        LifeState::Dead {
            cause: DeathCause::Disease,
            ..
        }
    ));
    assert_eq!(world.agents[&actor].disease, DiseaseState::Susceptible);
    assert!(!world.locations[&location].agents.contains(&actor));
    assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::Died { agent, cause: DeathCause::Disease } if agent == actor))
                .count(),
            1
        );
    world.validate().expect("dead resident history");
}

#[test]
fn dead_residents_are_excluded_from_observation_and_scheduling() {
    let mut world = World::from_spec(BRIAR_GLEN, 21).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let location = world.agents[&actor].location;
    world.agents.get_mut(&actor).expect("resident").life = LifeState::Dead {
        tick: world.tick,
        cause: DeathCause::Injury,
    };
    world.agents.get_mut(&actor).expect("resident").health = 0.0;
    for agent in world.agents.values_mut() {
        agent.goals.clear();
    }
    world
        .locations
        .get_mut(&location)
        .expect("location")
        .agents
        .remove(&actor);
    assert!(matches!(
        crate::cognition::perceive(&world, actor),
        Err(crate::cognition::ObservationError::AgentDead(id)) if id == actor
    ));
    assert!(!Scheduler.agents_to_act(&world).contains(&actor));
    world.validate().expect("valid dead resident history");
}
