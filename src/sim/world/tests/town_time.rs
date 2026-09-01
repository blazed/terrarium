use super::*;

#[test]
fn briar_glen_has_consistent_residents() {
    let world = World::from_spec(BRIAR_GLEN, 814_921).expect("town should construct");
    assert_eq!(world.agents.len(), 8);
    assert_eq!(world.locations.len(), 10);
    assert_eq!(
        world
            .locations
            .values()
            .map(|location| location.agents.len())
            .sum::<usize>(),
        8
    );
    world.validate().expect("town should be valid");
}

#[test]
fn town_events_start_end_and_change_conditions() {
    let mut storm = World::from_spec(BRIAR_GLEN, 0).expect("town");
    let home = storm.agents.values().next().expect("resident").home;
    let non_home = storm
        .locations
        .keys()
        .copied()
        .find(|location| !storm.agents.values().any(|agent| agent.home == *location))
        .expect("non-home location");
    let safety = storm.agents.values().next().expect("resident").needs.safety;
    storm
        .advance_to(Tick(8 * 60 / Tick::MINUTES))
        .expect("storm starts");
    assert_eq!(
        storm.active_town_event.expect("active").kind,
        TownEventKind::Storm
    );
    assert!(storm.is_location_open(home));
    assert!(!storm.is_location_open(non_home));
    assert!(safety - storm.agents.values().next().expect("resident").needs.safety > 0.001);
    assert!(matches!(
        storm.events().last().expect("start").kind,
        EventKind::TownEventStarted {
            kind: TownEventKind::Storm,
            ..
        }
    ));

    let ends_at = storm.active_town_event.expect("active").ends_at;
    storm.advance_to(ends_at).expect("storm ends");
    assert_eq!(storm.active_town_event, None);
    assert!(matches!(
        storm.events().last().expect("end").kind,
        EventKind::TownEventEnded {
            kind: TownEventKind::Storm
        }
    ));

    storm.active_town_event = Some(TownEvent {
        kind: TownEventKind::Festival,
        starts_at: storm.tick,
        ends_at: Tick(storm.tick.0 + 1),
    });
    assert!(matches!(storm.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn festivals_and_market_conditions_modify_existing_actions() {
    let mut festival = World::from_spec(BRIAR_GLEN, 1).expect("town");
    let residents = festival.agents.keys().copied().take(2).collect::<Vec<_>>();
    let speaker = residents[0];
    let listener = residents[1];
    let home = festival.agents[&speaker].home;
    festival.relocate(speaker, home);
    festival.relocate(listener, home);
    festival
        .advance_to(Tick(9 * 60 / Tick::MINUTES))
        .expect("festival starts");
    festival
        .agents
        .get_mut(&speaker)
        .expect("speaker")
        .needs
        .companionship = 0.0;
    festival
        .agents
        .get_mut(&speaker)
        .expect("speaker")
        .needs
        .status = 0.0;
    festival.execute(
        speaker,
        ProposedAction::Talk {
            target: listener,
            tone: DialogueTone::Neutral,
            message: "Enjoy the festival!".into(),
        },
    );
    assert!(festival.agents[&speaker].needs.companionship > 0.17);
    assert!(festival.agents[&speaker].needs.status >= 0.015);

    for (seed, expected_kind, expected_stock) in [
        (2, TownEventKind::Shortage, STOCK_PER_SHIFT / 2),
        (3, TownEventKind::MarketDay, STOCK_PER_SHIFT * 2),
    ] {
        let mut world = World::from_spec(BRIAR_GLEN, seed).expect("town");
        let (worker, workplace) = world
            .agents
            .iter()
            .find_map(|(id, agent)| agent.workplace.map(|workplace| (*id, workplace)))
            .expect("worker");
        world.relocate(worker, workplace);
        world
            .advance_to(Tick((8 + seed) * 60 / Tick::MINUTES))
            .expect("event starts");
        assert_eq!(world.active_town_event.expect("active").kind, expected_kind);
        assert!(matches!(
            world.execute(worker, ProposedAction::Work),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Worked { stock_produced, .. } if stock_produced == expected_stock)
        ));
    }
}

#[test]
fn goals_are_contextual_and_seeded() {
    let first = World::from_spec(BRIAR_GLEN, 11).expect("town");
    let repeated = World::from_spec(BRIAR_GLEN, 11).expect("same town");
    let different = World::from_spec(BRIAR_GLEN, 12).expect("different town");
    let goals = |world: &World| {
        world
            .agents
            .values()
            .map(|agent| agent.goals.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(goals(&first), goals(&repeated));
    assert_ne!(goals(&first), goals(&different));
    assert!(first.agents.values().all(|agent| {
        !agent.goals.is_empty()
            && agent.goals.len() <= GOAL_LIMIT
            && agent
                .goals
                .iter()
                .all(|goal| goal.expires_at == Tick(first.tick.0 + Tick::PER_DAY))
    }));
    let descriptions = first
        .agents
        .values()
        .flat_map(|agent| agent.goals.iter().map(|goal| goal.description.as_str()))
        .collect::<BTreeSet<_>>();
    assert!(descriptions.len() > GOAL_LIMIT);
}

#[test]
fn tick_only_moves_forward() {
    let mut world = World::from_spec(BRIAR_GLEN, 1).expect("town should construct");
    let start = world.tick;
    world.advance_to(Tick(start.0 + 2)).expect("forward tick");
    assert_eq!(
        world.advance_to(Tick(start.0 + 1)),
        Err(WorldError::NonMonotonicTick {
            current: Tick(start.0 + 2),
            proposed: Tick(start.0 + 1),
        })
    );
}

#[test]
fn activities_last_until_completion_and_urgent_needs_interrupt_them() {
    let mut world = World::from_spec(BRIAR_GLEN, 2).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let start = world.tick;
    let food_business = world
        .locations
        .values()
        .find(|location| location.business.is_some() && location.is_open(world.tick.hour()))
        .expect("open food business")
        .id;
    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.needs.food = 1.0;
    agent.needs.energy = 1.0;
    agent.needs.safety = 1.0;

    assert!(matches!(
        world.execute(actor, ProposedAction::Work),
        ActionResult::Rejected(ActionRejection::CannotWorkHere(_))
    ));
    assert_eq!(world.agents[&actor].activity, None);

    world.relocate(actor, food_business);
    assert!(matches!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Success(_)
    ));
    assert_eq!(
        world.agents[&actor].activity,
        Some(Activity {
            kind: ActivityKind::Shopping,
            until: Tick(start.0 + 3),
        })
    );
    world.advance_to(Tick(start.0 + 2)).expect("activity time");
    assert!(world.agents[&actor].activity.is_some());
    world.advance_to(Tick(start.0 + 3)).expect("completion");
    assert_eq!(world.agents[&actor].activity, None);

    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.activity = Some(Activity {
        kind: ActivityKind::Working,
        until: Tick(start.0 + 15),
    });
    agent.needs.food = 0.05;
    world.advance_tick().expect("interruption");
    assert_eq!(world.agents[&actor].activity, None);
}

#[test]
fn time_and_successful_actions_update_needs() {
    let mut world = World::from_spec(BRIAR_GLEN, 2).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let actor = residents[0];
    let listener = residents[1];
    let food_business = world
        .locations
        .values()
        .find(|location| location.business.is_some() && location.is_open(world.tick.hour()))
        .expect("open food business")
        .id;
    world.relocate(actor, food_business);
    world.relocate(listener, food_business);
    let before = world.agents[&actor].needs.clone();

    world.advance_tick().expect("tick");
    let decayed = &world.agents[&actor].needs;
    assert!(decayed.food < before.food);
    assert!(decayed.energy < before.energy);
    assert!(decayed.companionship < before.companionship);

    let companionship = decayed.companionship;
    world.execute(
        actor,
        ProposedAction::Talk {
            target: listener,
            tone: DialogueTone::Neutral,
            message: "Hello.".into(),
        },
    );
    assert!(world.agents[&actor].needs.companionship > companionship);

    let needs = world.agents[&actor].needs.clone();
    world.execute(actor, ProposedAction::Purchase);
    assert_eq!(world.agents[&actor].needs, needs);
    assert_eq!(world.agents[&actor].inventory.meals, 1);
    world.execute(actor, ProposedAction::ConsumeMeal);
    assert!(world.agents[&actor].needs.food > needs.food);
    assert_eq!(world.agents[&actor].inventory.meals, 0);
    let energy = world.agents[&actor].needs.energy;
    let safety = world.agents[&actor].needs.safety;
    let home = world.agents[&actor].home;
    world.relocate(actor, home);
    world.execute(actor, ProposedAction::Rest);
    assert!(world.agents[&actor].needs.energy > energy);
    assert!(world.agents[&actor].needs.safety > safety);
    assert!(
        world.agents[&listener].memories.iter().any(
            |event| matches!(event.kind, EventKind::Purchased { agent, .. } if agent == actor)
        )
    );
    assert!(world.agents[&listener].memories.iter().any(
            |event| matches!(event.kind, EventKind::ItemUsed { agent, item: Item::Meal } if agent == actor)
        ));
    world.validate().expect("normalized needs");
}

#[test]
fn work_and_purchases_transfer_coins_and_reject_atomically() {
    let mut world = World::from_spec(BRIAR_GLEN, 12).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let business = world.agents[&actor].workplace.expect("workplace");
    assert!(world.locations[&business].business.is_some());
    world.relocate(actor, business);

    let balance = world.agents[&actor].balance;
    let initial_stock = world.locations[&business].business.expect("business").stock;
    assert!(matches!(
        world.execute(actor, ProposedAction::Work),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Worked {
                wage: WORK_WAGE,
                stock_produced: STOCK_PER_SHIFT,
                ..
            })
    ));
    assert_eq!(world.agents[&actor].balance, balance + WORK_WAGE);
    assert_eq!(
        world.locations[&business].business.expect("business").stock,
        initial_stock + STOCK_PER_SHIFT
    );

    let stock = world.locations[&business].business.expect("business").stock;
    assert!(matches!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Purchased { cost: 5, .. })
    ));
    assert_eq!(world.agents[&actor].balance, balance + WORK_WAGE - 5);
    assert_eq!(
        world.locations[&business].business.expect("business"),
        Business {
            offering: Offering::Meal,
            price: 5,
            cash: BUSINESS_STARTING_CASH - WORK_WAGE + 5,
            stock: stock - 1,
            revenue: 5,
            wages_paid: WORK_WAGE,
        }
    );

    world.agents.get_mut(&actor).expect("resident").balance = 0;
    let before = world.clone();
    assert_eq!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Rejected(ActionRejection::InsufficientFunds {
            cost: 5,
            available: 0,
        })
    );
    assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
    assert_eq!(world.locations[&business], before.locations[&business]);

    world.agents.get_mut(&actor).expect("resident").balance = 5;
    world
        .locations
        .get_mut(&business)
        .expect("business")
        .business
        .as_mut()
        .expect("ledger")
        .stock = 0;
    let before = world.clone();
    assert_eq!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Rejected(ActionRejection::SoldOut(business))
    );
    assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
    assert_eq!(world.locations[&business], before.locations[&business]);

    let ledger = world
        .locations
        .get_mut(&business)
        .expect("business")
        .business
        .as_mut()
        .expect("ledger");
    ledger.cash = WORK_WAGE - 1;
    let before = world.clone();
    assert_eq!(
        world.execute(actor, ProposedAction::Work),
        ActionResult::Rejected(ActionRejection::InsolventEmployer {
            location: business,
            wage: WORK_WAGE,
            available: WORK_WAGE - 1,
        })
    );
    assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
    assert_eq!(world.locations[&business], before.locations[&business]);

    let ledger = world
        .locations
        .get_mut(&business)
        .expect("business")
        .business
        .as_mut()
        .expect("ledger");
    ledger.cash = u64::MAX;
    ledger.stock = 1;
    let before = world.clone();
    assert_eq!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Rejected(ActionRejection::EconomyOverflow)
    );
    assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
    assert_eq!(world.locations[&business], before.locations[&business]);
}

#[test]
fn mutual_aid_transfers_needed_items_and_rejects_atomically() {
    let mut world = World::from_spec(BRIAR_GLEN, 8).expect("town");
    let residents = world.agents.keys().copied().take(2).collect::<Vec<_>>();
    let giver = residents[0];
    let receiver = residents[1];
    let location = world.agents[&giver].location;
    world.relocate(receiver, location);
    world.agents.get_mut(&giver).expect("giver").inventory.meals = 2;
    world
        .agents
        .get_mut(&receiver)
        .expect("receiver")
        .needs
        .food = 0.1;

    let event = match world.execute(
        giver,
        ProposedAction::Give {
            target: receiver,
            item: Item::Meal,
        },
    ) {
        ActionResult::Success(events) => events.into_iter().next().expect("aid event"),
        result => panic!("unexpected result: {result:?}"),
    };
    assert!(matches!(
        event.kind,
        EventKind::ItemGiven {
            giver: actual_giver,
            receiver: actual_receiver,
            item: Item::Meal,
        } if actual_giver == giver && actual_receiver == receiver
    ));
    assert_eq!(world.agents[&giver].inventory.meals, 1);
    assert_eq!(world.agents[&receiver].inventory.meals, 1);
    assert!(world.agents[&receiver].relationships[&giver].trust > 0.0);
    assert!(
        world.agents[&receiver]
            .memories
            .iter()
            .any(|memory| memory.id == event.id)
    );

    world
        .agents
        .get_mut(&receiver)
        .expect("receiver")
        .needs
        .food = 1.0;
    let inventories = (
        world.agents[&giver].inventory,
        world.agents[&receiver].inventory,
    );
    assert_eq!(
        world.execute(
            giver,
            ProposedAction::Give {
                target: receiver,
                item: Item::Meal,
            },
        ),
        ActionResult::Rejected(ActionRejection::ItemNotNeeded {
            target: receiver,
            item: Item::Meal,
        })
    );
    assert_eq!(
        (
            world.agents[&giver].inventory,
            world.agents[&receiver].inventory,
        ),
        inventories
    );
    world.validate().expect("valid aid state");
}
