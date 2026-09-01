use super::*;

/// Seed+setup where a theft succeeds with a 0.75 roll: victim busy, one idle
/// onlooker, everyone else busy, thief at honesty 0 / impulsiveness 0.
fn success_world() -> (World, AgentId, AgentId, AgentId) {
    for seed in 0..256 {
        let mut world = World::from_spec(BRIAR_GLEN, seed).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let thief = residents[0];
        let victim = residents[1];
        let idle_witness = residents[5];
        if world.seeded_roll(thief, victim, 0.75) {
            for id in residents
                .iter()
                .copied()
                .filter(|id| *id != thief && *id != victim && *id != idle_witness)
            {
                world.agents.get_mut(&id).expect("resident").activity = Some(Activity {
                    kind: ActivityKind::Resting,
                    until: Tick(10_000),
                });
            }
            world.agents.get_mut(&victim).expect("victim").activity = Some(Activity {
                kind: ActivityKind::Resting,
                until: Tick(10_000),
            });
            let thief_state = world.agents.get_mut(&thief).expect("thief");
            thief_state.personality.honesty = 0.0;
            thief_state.personality.impulsiveness = 0.0;
            return (world, thief, victim, idle_witness);
        }
    }
    panic!("no seed produced a 0.75 theft success within 256 trials");
}

/// Seed+setup where a theft fails with a 0.05 roll: everyone idle together.
fn failure_world() -> (World, AgentId, AgentId) {
    for seed in 0..256 {
        let mut world = World::from_spec(BRIAR_GLEN, seed).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let thief = residents[0];
        let victim = residents[1];
        if !world.seeded_roll(thief, victim, 0.05) {
            let thief_state = world.agents.get_mut(&thief).expect("thief");
            thief_state.personality.honesty = 1.0;
            thief_state.personality.impulsiveness = 1.0;
            let victim_state = world.agents.get_mut(&victim).expect("victim");
            victim_state.personality.honesty = 1.0;
            victim_state.personality.impulsiveness = 1.0;
            return (world, thief, victim);
        }
    }
    panic!("no seed produced a 0.05 theft failure within 256 trials");
}

#[test]
fn stealing_coins_transfers_balance_and_emits_stole() {
    let (mut world, thief, victim, _idle) = success_world();
    world.agents.get_mut(&victim).expect("victim").balance = 30;
    let balance_before = world.agents[&victim].balance;
    let safety_before = world.agents[&victim].needs.safety;
    let mood_before = world.agents[&thief].mood;

    let result = world.execute(
        thief,
        ProposedAction::Steal {
            target: victim,
            loot: Loot::Coins(10),
        },
    );
    assert!(matches!(
        result,
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Stole { loot: Loot::Coins(10), .. })
                && matches!(events[1].kind, EventKind::Robbed { loot: Loot::Coins(10), .. })
    ));
    assert_eq!(world.agents[&victim].balance, balance_before - 10);
    assert_eq!(world.agents[&thief].balance, 20 + 10);
    assert!(world.agents[&victim].needs.safety < safety_before);
    assert!(world.agents[&thief].mood > mood_before);
    assert!(matches!(
        world.agents[&thief].activity,
        Some(Activity {
            kind: ActivityKind::Stealing,
            ..
        })
    ));
}

#[test]
fn stealing_an_item_transfers_inventory() {
    let (mut world, thief, victim, _idle) = success_world();
    world
        .agents
        .get_mut(&victim)
        .expect("victim")
        .inventory
        .meals = 2;
    assert!(matches!(
        world.execute(thief, ProposedAction::Steal {
            target: victim,
            loot: Loot::Item(Item::Meal),
        }),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Stole { loot: Loot::Item(Item::Meal), .. })
    ));
    assert_eq!(world.agents[&victim].inventory.meals, 1);
    assert_eq!(world.agents[&thief].inventory.meals, 1);
}

#[test]
fn failed_theft_emits_theft_failed_and_warns_the_whole_location() {
    let (mut world, thief, victim) = failure_world();
    world.agents.get_mut(&victim).expect("victim").balance = 20;
    let mood_before = world.agents[&thief].mood;
    let safety_before = world.agents[&victim].needs.safety;

    assert!(matches!(
        world.execute(thief, ProposedAction::Steal {
            target: victim,
            loot: Loot::Coins(5),
        }),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::TheftFailed { .. })
    ));
    assert_eq!(world.agents[&victim].balance, 20);
    assert!(world.agents[&thief].mood < mood_before);
    assert!(world.agents[&victim].needs.safety < safety_before);
    for resident in world.agents.keys().copied().collect::<Vec<_>>() {
        assert!(
            world.agents[&resident]
                .memories
                .iter()
                .any(|event| matches!(event.kind, EventKind::TheftFailed { .. })),
            "resident {resident} should remember the failed theft"
        );
    }
}

#[test]
fn theft_rejections_are_atomic() {
    let (mut world, thief, victim) = failure_world();
    let stranger = *world
        .agents
        .keys()
        .find(|id| **id != thief && **id != victim)
        .expect("third resident");
    let tavern = world
        .locations
        .values()
        .find(|location| location.name == "The Crooked Lantern")
        .map(|location| location.id)
        .expect("tavern");
    world.relocate(stranger, tavern);

    assert_eq!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: thief,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Rejected(ActionRejection::SelfTarget(thief))
    );
    assert_eq!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: AgentId(Uuid::nil()),
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Rejected(ActionRejection::UnknownAgent(AgentId(Uuid::nil())))
    );

    world.agents.get_mut(&victim).expect("victim").life = LifeState::Dead {
        tick: world.tick,
        cause: DeathCause::Disease,
    };
    assert_eq!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Rejected(ActionRejection::AgentDead(victim))
    );

    assert_eq!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: stranger,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Rejected(ActionRejection::NotCoLocated {
            actor: thief,
            target: stranger
        })
    );

    let (mut world, thief, victim) = failure_world();
    world.agents.get_mut(&victim).expect("victim").balance = 0;
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Rejected(ActionRejection::LootNotOwned { .. })
    ));
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Item(Item::Medicine),
            }
        ),
        ActionResult::Rejected(ActionRejection::LootNotOwned { .. })
    ));

    let (mut world, thief, victim, _idle) = success_world();
    world
        .agents
        .get_mut(&victim)
        .expect("victim")
        .inventory
        .meals = 1;
    world.agents.get_mut(&thief).expect("thief").inventory.meals = MAX_ITEMS_PER_KIND;
    assert_eq!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Item(Item::Meal),
            }
        ),
        ActionResult::Rejected(ActionRejection::InventoryFull(Item::Meal))
    );
}

#[test]
fn successful_theft_is_seen_by_witnesses_but_never_by_the_victim() {
    let (mut world, thief, victim, idle_witness) = success_world();
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let busy_witness = residents
        .iter()
        .find(|id| {
            **id != thief
                && **id != victim
                && **id != idle_witness
                && world.agents[id].activity.is_some()
        })
        .copied()
        .expect("busy witness");

    world.execute(
        thief,
        ProposedAction::Steal {
            target: victim,
            loot: Loot::Coins(5),
        },
    );

    let remembers_stole = |agent: AgentId| {
        world.agents[&agent]
            .memories
            .iter()
            .any(|event| matches!(event.kind, EventKind::Stole { .. }))
    };
    assert!(remembers_stole(thief));
    assert!(remembers_stole(idle_witness));
    assert!(!remembers_stole(victim));
    assert!(!remembers_stole(busy_witness));
    assert!(
        world.agents[&victim]
            .memories
            .iter()
            .any(|event| matches!(event.kind, EventKind::Robbed { .. }))
    );
    assert!(
        world.agents[&victim]
            .beliefs
            .get(&thief)
            .is_none_or(|belief| belief.hostility < 0.2)
    );
    // The idle witness learned hostility about the thief.
    assert!(world.agents[&idle_witness].beliefs[&thief].hostility > 0.5);
}

#[test]
fn checkpoint_round_trip_preserves_crime_events_and_history() {
    let (mut world, thief, victim, _idle) = success_world();
    world.agents.get_mut(&victim).expect("victim").balance = 20;
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(7),
            }
        ),
        ActionResult::Success(_)
    ));

    let restored = World::from_snapshot(
        world.name.clone(),
        world.seed,
        world.tick,
        world.agents.values().cloned().collect(),
        world.locations.values().cloned().collect(),
        world.active_town_event,
        world.events.clone(),
    )
    .expect("round trip accepts crime events");
    assert_eq!(restored.events(), world.events());
    let crime_events = restored
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::Stole { .. } | EventKind::TheftFailed { .. } | EventKind::Robbed { .. }
            )
        })
        .count();
    assert_eq!(crime_events, 2);
}

#[test]
fn assault_injures_victim_and_drops_their_relationship() {
    let mut world = World::from_spec(BRIAR_GLEN, 21).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let attacker = residents[0];
    let victim = residents[1];
    world
        .agents
        .get_mut(&attacker)
        .expect("attacker")
        .personality
        .agreeableness = 1.0;
    let health_before = world.agents[&victim].health;
    let safety_before = world.agents[&victim].needs.safety;
    let attacker_mood_before = world.agents[&attacker].mood;
    let victim_mood_before = world.agents[&victim].mood;

    assert!(matches!(
        world.execute(attacker, ProposedAction::Attack { target: victim }),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Assaulted { .. })
    ));
    let victim_state = &world.agents[&victim];
    assert_eq!(victim_state.health, health_before - 0.35);
    assert!(victim_state.injury);
    assert!(victim_state.needs.safety < safety_before);
    assert!(victim_state.mood < victim_mood_before);
    // Remorseful (agreeable) attacker's mood drops.
    assert!(world.agents[&attacker].mood < attacker_mood_before);
    let relationship = victim_state.relationships[&attacker];
    assert!(relationship.affection < 0.0);
    assert!(relationship.trust < 0.0);
    assert!(relationship.respect < 0.0);
    assert!(relationship.suspicion > 0.0);
    // Both participants got the Fighting activity.
    assert!(world.agents[&attacker].activity.is_some());
    assert!(world.agents[&victim].activity.is_some());
}

#[test]
fn unremorseful_attacker_keeps_mood() {
    let mut world = World::from_spec(BRIAR_GLEN, 22).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let attacker = residents[0];
    let victim = residents[1];
    world
        .agents
        .get_mut(&attacker)
        .expect("attacker")
        .personality
        .agreeableness = 0.0;
    let mood_before = world.agents[&attacker].mood;
    world.execute(attacker, ProposedAction::Attack { target: victim });
    assert_eq!(world.agents[&attacker].mood, mood_before);
}

#[test]
fn assault_witnesses_learn_hostility_about_the_attacker() {
    let mut world = World::from_spec(BRIAR_GLEN, 23).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let attacker = residents[0];
    let victim = residents[1];
    let witness = residents[3];

    world.execute(attacker, ProposedAction::Attack { target: victim });

    for resident in [victim, witness] {
        let belief = world.agents[&resident].beliefs[&attacker];
        assert!(
            belief.hostility > 0.5,
            "resident {resident} learned weak hostility"
        );
    }
    // The attacker does not learn about themselves.
    assert!(
        world.agents[&attacker]
            .beliefs
            .get(&attacker)
            .is_none_or(|belief| belief.hostility == 0.0)
    );
}

#[test]
fn assault_rejections_are_atomic() {
    let mut world = World::from_spec(BRIAR_GLEN, 24).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let attacker = residents[0];
    let victim = residents[1];

    assert_eq!(
        world.execute(attacker, ProposedAction::Attack { target: attacker }),
        ActionResult::Rejected(ActionRejection::SelfTarget(attacker))
    );
    assert_eq!(
        world.execute(
            attacker,
            ProposedAction::Attack {
                target: AgentId(Uuid::nil())
            }
        ),
        ActionResult::Rejected(ActionRejection::UnknownAgent(AgentId(Uuid::nil())))
    );

    world.agents.get_mut(&victim).expect("victim").life = LifeState::Dead {
        tick: world.tick,
        cause: DeathCause::Disease,
    };
    assert_eq!(
        world.execute(attacker, ProposedAction::Attack { target: victim }),
        ActionResult::Rejected(ActionRejection::AgentDead(victim))
    );

    let mut world = World::from_spec(BRIAR_GLEN, 25).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let attacker = residents[0];
    let tavern = world
        .locations
        .values()
        .find(|location| location.name == "The Crooked Lantern")
        .map(|location| location.id)
        .expect("tavern");
    world.relocate(residents[2], tavern);
    assert_eq!(
        world.execute(
            attacker,
            ProposedAction::Attack {
                target: residents[2]
            }
        ),
        ActionResult::Rejected(ActionRejection::NotCoLocated {
            actor: attacker,
            target: residents[2],
        })
    );
}

#[test]
fn assault_round_trips_and_self_assault_is_rejected_by_history() {
    let mut world = World::from_spec(BRIAR_GLEN, 26).expect("town");
    let attacker = *world.agents.keys().next().expect("resident");
    let victim = *world.agents.keys().nth(1).expect("resident");
    world.execute(attacker, ProposedAction::Attack { target: victim });
    let restored = World::from_snapshot(
        world.name.clone(),
        world.seed,
        world.tick,
        world.agents.values().cloned().collect(),
        world.locations.values().cloned().collect(),
        world.active_town_event,
        world.events.clone(),
    )
    .expect("valid assault round trips");
    assert_eq!(restored.events(), world.events());

    let mut invalid = World::from_spec(BRIAR_GLEN, 27).expect("town");
    let attacker = *invalid.agents.keys().next().expect("resident");
    invalid.append_event(
        None,
        EventKind::Assaulted {
            attacker,
            victim: attacker,
        },
    );
    assert!(matches!(
        World::from_snapshot(
            invalid.name.clone(),
            invalid.seed,
            invalid.tick,
            invalid.agents.values().cloned().collect(),
            invalid.locations.values().cloned().collect(),
            invalid.active_town_event,
            invalid.events.clone(),
        ),
        Err(WorldError::InvalidState(_))
    ));
}

#[test]
fn theft_roll_is_deterministic_per_seed_and_tick() {
    let (mut first, thief, victim, _first_idle) = success_world();
    first.agents.get_mut(&victim).expect("victim").balance = 20;
    let (mut second, thief2, victim2, _second_idle) = success_world();

    assert_eq!(
        first.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(5),
            }
        ),
        second.execute(
            thief2,
            ProposedAction::Steal {
                target: victim2,
                loot: Loot::Coins(5),
            }
        )
    );
    assert_eq!(first.events(), second.events());
}

#[test]
fn sheriff_arrests_a_witnessed_thief_and_the_term_expires() {
    let (mut world, thief, victim) = failure_world();
    let sheriff = world
        .agents
        .values()
        .find(|agent| agent.occupation == Occupation::Sheriff)
        .expect("sheriff")
        .id;
    // Everyone is idle together at Riverside Houses, so the attempt is witnessed
    // by everyone present, including the sheriff.
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Success(_)
    ));
    let attempt = world
        .events()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TheftFailed { .. }))
        .expect("failed attempt")
        .clone();
    assert!(
        world.agents[&sheriff]
            .memories
            .iter()
            .any(|memory| memory.id == attempt.id)
    );

    let cash_before = town_hall(&world).business.expect("business").cash;

    assert!(matches!(
        world.execute(
            sheriff,
            ProposedAction::Arrest {
                target: thief,
                claim: attempt.id,
            },
        ),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Arrested { fine: 10, .. })
    ));
    let jail = world
        .locations
        .values()
        .find(|location| location.name == "Jail")
        .expect("jail")
        .id;
    assert_eq!(world.agents[&thief].location, jail);
    assert!(world.locations[&jail].agents.contains(&thief));
    let activity = world.agents[&thief].activity.expect("jailed activity");
    assert_eq!(activity.kind, ActivityKind::Jailed);
    assert_eq!(activity.until, Tick(world.tick.0 + JAIL_TICKS));
    assert_eq!(world.agents[&thief].balance, 20 - 10);
    assert_eq!(
        town_hall(&world).business.expect("business").cash,
        cash_before + 10
    );
    // The scheduler skips the prisoner: they get no decision turns.
    assert!(!Scheduler.agents_to_act(&world).contains(&thief));

    // Only expiry frees them, back home and actionable again.
    world
        .advance_to(Tick(world.tick.0 + JAIL_TICKS))
        .expect("term elapses");
    assert!(
        world
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::Released { agent } if agent == thief))
    );
    assert_eq!(world.agents[&thief].location, world.agents[&thief].home);
    assert!(world.agents[&thief].activity.is_none());
    assert!(Scheduler.agents_to_act(&world).contains(&thief));
}

#[test]
fn arrests_need_sheriff_co_location_and_witnessed_or_rumored_basis() {
    let (mut world, thief, victim) = failure_world();
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let sheriff = world
        .agents
        .values()
        .find(|agent| agent.occupation == Occupation::Sheriff)
        .expect("sheriff")
        .id;
    let civilian = residents
        .iter()
        .copied()
        .find(|id| *id != sheriff && *id != thief)
        .expect("civilian");
    let tavern = world
        .locations
        .values()
        .find(|location| location.name == "The Crooked Lantern")
        .expect("tavern")
        .id;
    // The sheriff is away when the theft fails, so nobody but the participants
    // and onlookers at Riverside learn of it.
    world.relocate(sheriff, tavern);
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Success(_)
    ));
    let attempt = world
        .events()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TheftFailed { .. }))
        .expect("failed attempt")
        .clone();

    // A civilian cannot arrest at all.
    assert!(matches!(
        world.execute(
            civilian,
            ProposedAction::Arrest {
                target: thief,
                claim: attempt.id,
            },
        ),
        ActionResult::Rejected(ActionRejection::NotSheriff)
    ));

    // The sheriff must be co-located with their target.
    assert!(matches!(
        world.execute(
            sheriff,
            ProposedAction::Arrest {
                target: thief,
                claim: attempt.id,
            },
        ),
        ActionResult::Rejected(ActionRejection::NotCoLocated { .. })
    ));

    // Co-located but without a witness memory or credible rumor: no legal basis.
    world.relocate(sheriff, world.agents[&thief].location);
    match world.execute(
        sheriff,
        ProposedAction::Arrest {
            target: thief,
            claim: attempt.id,
        },
    ) {
        ActionResult::Rejected(ActionRejection::NoLegalBasis(claim)) => {
            assert_eq!(claim, attempt.id);
        }
        other => panic!("expected no legal basis, got {other:?}"),
    }

    // A credible crime rumor (confidence >= 0.6) makes the arrest legal.
    world
        .agents
        .get_mut(&sheriff)
        .expect("sheriff")
        .rumors
        .push(Rumor {
            event: attempt.clone(),
            source: victim,
            depth: 0,
            confidence: 0.8,
            resolved: false,
        });
    assert!(matches!(
        world.execute(
            sheriff,
            ProposedAction::Arrest {
                target: thief,
                claim: attempt.id,
            },
        ),
        ActionResult::Success(_)
    ));
}

#[test]
fn urgent_needs_and_storms_do_not_free_prisoners() {
    let (mut world, thief, victim) = failure_world();
    let sheriff = world
        .agents
        .values()
        .find(|agent| agent.occupation == Occupation::Sheriff)
        .expect("sheriff")
        .id;
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Success(_)
    ));
    let attempt = world
        .events()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TheftFailed { .. }))
        .expect("failed attempt")
        .clone();
    world.execute(
        sheriff,
        ProposedAction::Arrest {
            target: thief,
            claim: attempt.id,
        },
    );
    // Starving and caught in a storm: still confined.
    let prisoner = world.agents.get_mut(&thief).expect("prisoner");
    prisoner.needs.food = 0.05;
    prisoner.needs.energy = 0.05;
    prisoner.needs.safety = 0.05;
    world.active_town_event = Some(TownEvent {
        kind: TownEventKind::Storm,
        starts_at: world.tick,
        ends_at: Tick(world.tick.0 + 100),
    });
    world
        .advance_to(Tick(world.tick.0 + 24))
        .expect("storm passes");
    assert_eq!(
        world.agents[&thief].activity.expect("still jailed").kind,
        ActivityKind::Jailed
    );
    let jail = world
        .locations
        .values()
        .find(|location| location.name == "Jail")
        .expect("jail")
        .id;
    assert_eq!(world.agents[&thief].location, jail);
    assert_eq!(
        world.agents[&thief].activity.expect("still jailed").kind,
        ActivityKind::Jailed
    );

    world
        .advance_to(world.agents[&thief].activity.expect("jailed").until)
        .expect("term elapses");
    assert_eq!(world.agents[&thief].location, world.agents[&thief].home);
    assert!(world.agents[&thief].activity.is_none());
}

#[test]
fn fine_is_skipped_when_the_prisoner_cannot_pay() {
    let (mut world, thief, victim) = failure_world();
    let sheriff = world
        .agents
        .values()
        .find(|agent| agent.occupation == Occupation::Sheriff)
        .expect("sheriff")
        .id;
    world.agents.get_mut(&thief).expect("thief").balance = 5;
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Success(_)
    ));
    let attempt = world
        .events()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TheftFailed { .. }))
        .expect("failed attempt")
        .clone();
    let cash_before = world
        .locations
        .values()
        .find(|location| {
            location
                .business
                .is_some_and(|business| business.offering == Offering::CivicServices)
        })
        .expect("town hall")
        .business
        .expect("business")
        .cash;
    assert!(matches!(
        world.execute(
            sheriff,
            ProposedAction::Arrest {
                target: thief,
                claim: attempt.id,
            },
        ),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Arrested { fine: 0, .. })
    ));
    assert_eq!(world.agents[&thief].balance, 5);
    let cash_after = world
        .locations
        .values()
        .find(|location| {
            location
                .business
                .is_some_and(|business| business.offering == Offering::CivicServices)
        })
        .expect("town hall")
        .business
        .expect("business")
        .cash;
    assert_eq!(cash_after, cash_before);
}

#[test]
fn checkpoint_round_trip_preserves_the_sentence_and_releases_after_resume() {
    let (mut world, thief, victim) = failure_world();
    let sheriff = world
        .agents
        .values()
        .find(|agent| agent.occupation == Occupation::Sheriff)
        .expect("sheriff")
        .id;
    assert!(matches!(
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            }
        ),
        ActionResult::Success(_)
    ));
    let attempt = world
        .events()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TheftFailed { .. }))
        .expect("failed attempt")
        .clone();
    world.execute(
        sheriff,
        ProposedAction::Arrest {
            target: thief,
            claim: attempt.id,
        },
    );
    let jailed_until = world.agents[&thief].activity.expect("jailed").until;

    let mut restored = World::from_snapshot(
        world.name.clone(),
        world.seed,
        world.tick,
        world.agents.values().cloned().collect(),
        world.locations.values().cloned().collect(),
        world.active_town_event,
        world.events.clone(),
    )
    .expect("round trip accepts the sentence");
    assert_eq!(
        restored.agents[&thief].activity.expect("jailed").until,
        jailed_until
    );
    restored
        .advance_to(jailed_until)
        .expect("term elapses after resume");
    assert!(
        restored
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::Released { agent } if agent == thief))
    );
    assert_eq!(
        restored.agents[&thief].location,
        restored.agents[&thief].home
    );
    assert!(restored.agents[&thief].activity.is_none());
}

fn town_hall(world: &World) -> &Location {
    world
        .locations
        .values()
        .find(|location| {
            location
                .business
                .is_some_and(|business| business.offering == Offering::CivicServices)
        })
        .expect("town hall")
}

#[tokio::test]
async fn sheriff_arrests_follow_witnessed_crimes_across_many_seeds() {
    // 10 seeds x 30 days with the local engine: the sheriff must make at least
    // one arrest; every arrest names the Sheriff as officer, the claim's subject
    // as prisoner, and rests on a witnessed memory or credible rumor (>= 0.6).
    use crate::{decision::LocalDecisionEngine, runner::run_simulation, sim::event_evidence};
    let mut arrests = 0;
    for seed in 0..10 {
        let world = World::from_spec(BRIAR_GLEN, seed).expect("town");
        let mut engine = LocalDecisionEngine::new(seed);
        let world = run_simulation(world, 30 * Tick::PER_DAY, &mut engine)
            .await
            .expect("simulation");
        for event in world.events() {
            let EventKind::Arrested {
                officer,
                prisoner,
                claim,
                ..
            } = &event.kind
            else {
                continue;
            };
            arrests += 1;
            assert_eq!(
                world.agents[officer].occupation,
                Occupation::Sheriff,
                "only the sheriff arrests"
            );
            let Some(claimed_event) = world.events().iter().find(|event| event.id == *claim) else {
                panic!("arrest claim {claim} is not a stored event");
            };
            let Some((subject, ..)) = event_evidence(&claimed_event.kind) else {
                panic!("arrest claim {claim} is not a crime event");
            };
            // Legality at arrest time is enforced by World::execute (NoLegalBasis);
            // memories may have been evicted by run end, so only the durable facts
            // (sheriff officer, claim subject) are asserted here.
            assert_eq!(subject, *prisoner, "prisoner must be the claim's subject");
        }
    }
    assert!(arrests >= 1, "expected at least one arrest across 10 seeds");
}
