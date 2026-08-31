use super::*;

/// Seed+setup where a theft succeeds with a 0.75 roll: victim busy, one idle
/// onlooker, everyone else busy, thief at honesty 0 / impulsiveness 0.
fn success_world() -> (World, AgentId, AgentId, AgentId) {
    for seed in 0..256 {
        let mut world = World::briar_glen(seed).expect("town");
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
        let mut world = World::briar_glen(seed).expect("town");
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
