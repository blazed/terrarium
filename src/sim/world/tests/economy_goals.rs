use super::*;

#[test]
fn inventory_capacity_and_missing_items_reject_atomically() {
    let mut world = World::from_spec(BRIAR_GLEN, 22).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let meal_business = world
        .locations
        .values()
        .find(|location| {
            location
                .business
                .is_some_and(|business| business.offering == Offering::Meal)
                && location.is_open(world.tick.hour())
        })
        .expect("open meal business")
        .id;
    world.relocate(actor, meal_business);

    assert_eq!(
        world.execute(actor, ProposedAction::ConsumeMeal),
        ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Meal))
    );
    assert_eq!(
        world.execute(actor, ProposedAction::UseSupplies),
        ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Supplies))
    );
    assert_eq!(
        world.execute(actor, ProposedAction::UseRepairKit),
        ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::RepairKit))
    );

    world
        .agents
        .get_mut(&actor)
        .expect("resident")
        .inventory
        .meals = MAX_ITEMS_PER_KIND;
    let before_agent = world.agents[&actor].clone();
    let before_location = world.locations[&meal_business].clone();
    assert_eq!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Rejected(ActionRejection::InventoryFull(Item::Meal))
    );
    assert_eq!(world.agents[&actor].balance, before_agent.balance);
    assert_eq!(world.agents[&actor].inventory, before_agent.inventory);
    assert_eq!(world.locations[&meal_business], before_location);

    world
        .agents
        .get_mut(&actor)
        .expect("resident")
        .inventory
        .meals = MAX_ITEMS_PER_KIND + 1;
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn clinic_sells_medicine_and_provides_paid_treatment() {
    let mut world = World::from_spec(BRIAR_GLEN, 31).expect("town");
    world.advance_to(Tick(8 * 12)).expect("clinic opening");
    let clinic = world.clinic_location().expect("one clinic");
    let actor = *world.agents.keys().next().expect("resident");
    world.relocate(actor, clinic);
    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.balance = 100;
    agent.health = 0.3;
    agent.injury = true;

    let business_before = world.locations[&clinic].business.expect("clinic business");
    assert_eq!(business_before.offering, Offering::Medicine);
    assert!(matches!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Success(_)
    ));
    assert_eq!(world.agents[&actor].inventory.medicine, 1);
    assert!(matches!(
        world.execute(actor, ProposedAction::UseMedicine),
        ActionResult::Success(_)
    ));
    assert_eq!(world.agents[&actor].inventory.medicine, 0);
    assert!(world.agents[&actor].health > 0.3);
    assert!(!world.agents[&actor].injury);

    assert!(matches!(
        world.execute(actor, ProposedAction::Purchase),
        ActionResult::Success(_)
    ));
    let recovery_until = Tick(world.tick.0 + RECOVERY_TICKS);
    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.health = 0.2;
    agent.disease = DiseaseState::Symptomatic {
        until: Tick(world.tick.0 + SYMPTOMATIC_TICKS),
    };
    assert!(matches!(
        world.execute(actor, ProposedAction::UseMedicine),
        ActionResult::Success(ref events)
            if events.iter().any(|event| matches!(
                event.kind,
                EventKind::DiseaseRecovered { agent } if agent == actor
            ))
    ));
    assert_eq!(
        world.agents[&actor].disease,
        DiseaseState::Recovering {
            until: recovery_until
        }
    );

    let agent = world.agents.get_mut(&actor).expect("resident");
    agent.health = 0.2;
    agent.injury = true;
    let balance_before = agent.balance;
    let stock_before = world.locations[&clinic].business.expect("clinic").stock;
    assert!(matches!(
        world.execute(actor, ProposedAction::SeekTreatment),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Treated { cost, .. } if cost == business_before.price)
    ));
    assert_eq!(
        world.agents[&actor].balance,
        balance_before - business_before.price
    );
    assert_eq!(
        world.locations[&clinic].business.expect("clinic").stock,
        stock_before - 1
    );
    assert!(world.agents[&actor].health > 0.2);
    assert!(!world.agents[&actor].injury);
    world.validate().expect("valid treated world");
}

#[test]
fn treatment_requires_the_clinic_and_a_medical_need() {
    let mut world = World::from_spec(BRIAR_GLEN, 32).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    world.agents.get_mut(&actor).expect("resident").balance = 100;
    assert_eq!(
        world.execute(actor, ProposedAction::SeekTreatment),
        ActionResult::Rejected(ActionRejection::CannotSeekTreatmentHere(
            world.agents[&actor].location
        ))
    );
    let clinic = world.clinic_location().expect("clinic");
    world.relocate(actor, clinic);
    assert_eq!(
        world.execute(actor, ProposedAction::SeekTreatment),
        ActionResult::Rejected(ActionRejection::NoMedicalNeed)
    );
    assert_eq!(
        world.execute(actor, ProposedAction::UseMedicine),
        ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Medicine))
    );
}

#[test]
fn every_market_offering_transfers_value_and_work_restocks_it() {
    for offering in [
        Offering::Meal,
        Offering::Supplies,
        Offering::Repairs,
        Offering::Medicine,
        Offering::CivicServices,
    ] {
        let mut world = World::from_spec(BRIAR_GLEN, 21).expect("town");
        world.advance_to(Tick(8 * 12)).expect("business hours");
        let location = world
            .locations
            .values()
            .find(|location| {
                location.is_open(world.tick.hour())
                    && location
                        .business
                        .is_some_and(|business| business.offering == offering)
            })
            .map(|location| location.id)
            .expect("open offering");
        let actor = world
            .agents
            .values()
            .find(|agent| agent.workplace == Some(location))
            .map(|agent| agent.id)
            .expect("worker");
        world.relocate(actor, location);
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.balance = 100;
        agent.needs.food = 0.1;
        agent.needs.safety = 0.1;
        agent.needs.status = 0.1;
        agent.needs.companionship = 0.1;
        if offering == Offering::Medicine {
            agent.health = 0.4;
            agent.injury = true;
        }
        let before_health = agent.health;
        let before_needs = agent.needs.clone();
        let before = world.locations[&location].business.expect("business");

        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Purchased {
                    offering: actual,
                    cost,
                    ..
                } if actual == offering && cost == before.price)
        ));
        let after_purchase = world.locations[&location].business.expect("business");
        assert_eq!(world.agents[&actor].balance, 100 - before.price);
        assert_eq!(after_purchase.cash, before.cash + before.price);
        assert_eq!(after_purchase.revenue, before.revenue + before.price);
        assert_eq!(after_purchase.stock, before.stock - 1);
        match offering {
            Offering::Meal => {
                assert_eq!(world.agents[&actor].inventory.meals, 1);
                world.execute(actor, ProposedAction::ConsumeMeal);
                assert!(world.agents[&actor].needs.food > before_needs.food);
            }
            Offering::Supplies => {
                assert_eq!(world.agents[&actor].inventory.supplies, 1);
                world.execute(actor, ProposedAction::UseSupplies);
                assert!(world.agents[&actor].needs.safety > before_needs.safety);
            }
            Offering::Repairs => {
                assert_eq!(world.agents[&actor].inventory.repair_kits, 1);
                world.execute(actor, ProposedAction::UseRepairKit);
                assert!(world.agents[&actor].needs.safety > before_needs.safety);
            }
            Offering::Medicine => {
                assert_eq!(world.agents[&actor].inventory.medicine, 1);
                assert!(matches!(
                    world.execute(actor, ProposedAction::UseMedicine),
                    ActionResult::Success(_)
                ));
                assert!(world.agents[&actor].health > before_health);
                assert!(!world.agents[&actor].injury);
            }
            Offering::CivicServices => {
                assert!(world.agents[&actor].needs.status > before_needs.status);
                assert!(world.agents[&actor].needs.companionship > before_needs.companionship);
            }
        }

        assert!(matches!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Worked {
                    stock_produced: STOCK_PER_SHIFT,
                    ..
                })
        ));
        assert_eq!(
            world.locations[&location]
                .business
                .expect("restocked")
                .stock,
            after_purchase.stock + STOCK_PER_SHIFT
        );
    }
}

#[test]
fn events_change_mood_and_time_returns_it_toward_neutral() {
    let mut world = World::from_spec(BRIAR_GLEN, 4).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let location = world.agents[&actor].location;

    for _ in 0..4 {
        world.execute(
            actor,
            ProposedAction::Observe {
                target: ObservationTarget::Location(location),
            },
        );
    }
    assert!((world.agents[&actor].mood - 0.08).abs() < f32::EPSILON * 4.0);

    world.execute(
        actor,
        ProposedAction::Talk {
            target: actor,
            tone: DialogueTone::Neutral,
            message: "Hello, me.".into(),
        },
    );
    assert!((world.agents[&actor].mood - 0.02).abs() < f32::EPSILON * 4.0);

    world
        .advance_to(Tick(world.tick.0 + 10))
        .expect("time advances");
    assert_eq!(world.agents[&actor].mood, 0.0);
    world.validate().expect("bounded mood");
    world.agents.get_mut(&actor).expect("actor").mood = 1.1;
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn contextual_goals_match_exact_targets_and_refresh() {
    let mut world = World::from_spec(BRIAR_GLEN, 4).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let actor = residents[0];
    let listener = residents[1];
    let other = residents[2];
    let expires_at = Tick(world.tick.0 + Tick::PER_DAY);
    world.agents.get_mut(&actor).expect("actor").goals = vec![Goal::new(
        "Speak twice with the intended resident",
        GoalKind::Community,
        GoalTarget::Talk { resident: listener },
        2,
        expires_at,
    )];

    world.execute(
        actor,
        ProposedAction::Talk {
            target: other,
            tone: DialogueTone::Neutral,
            message: "Hello.".into(),
        },
    );
    assert_eq!(world.agents[&actor].goals[0].progress, 0);

    for _ in 0..2 {
        world.execute(
            actor,
            ProposedAction::Talk {
                target: listener,
                tone: DialogueTone::Neutral,
                message: "Hello.".into(),
            },
        );
    }
    assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::GoalCompleted { agent, .. } if agent == actor))
                .count(),
            1
        );
    assert_eq!(world.agents[&actor].goals.len(), GOAL_LIMIT);
    assert!(
        world.agents[&actor]
            .goals
            .iter()
            .all(|goal| goal.description != "Speak twice with the intended resident")
    );
    assert!(world.agents[&listener].memories.iter().any(
        |event| matches!(event.kind, EventKind::GoalCompleted { agent, .. } if agent == actor)
    ));

    let home = world.agents[&actor].home;
    world.agents.get_mut(&actor).expect("actor").goals = vec![Goal::new(
        "Expiring goal",
        GoalKind::Exploration,
        GoalTarget::Visit { destination: home },
        1,
        Tick(world.tick.0 + 1),
    )];
    world
        .advance_to(Tick(world.tick.0 + 1))
        .expect("goal expiry");
    assert!(
        world.agents[&actor]
            .goals
            .iter()
            .all(|goal| goal.description != "Expiring goal")
    );

    world.agents.get_mut(&actor).expect("actor").goals[0].progress = 1;
    world.agents.get_mut(&actor).expect("actor").goals[0].required = 1;
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn multi_hop_intentions_continue_and_clear() {
    let mut world = World::from_spec(BRIAR_GLEN, 3).expect("town");
    world.advance_to(Tick(12 * 12)).expect("noon");
    let actor = *world.agents.keys().next().expect("resident");
    let destination = world
        .locations
        .values()
        .find(|location| location.name == "Town Hall")
        .expect("town hall")
        .id;
    assert!(
        !world.locations[&world.agents[&actor].location]
            .connected
            .contains(&destination)
    );

    assert!(matches!(
        world.execute(
            actor,
            ProposedAction::Pursue {
                intention: IntentionGoal::Visit { destination },
            },
        ),
        ActionResult::Success(_)
    ));
    assert!(world.agents[&actor].intention.is_some());

    while world.agents[&actor].intention.is_some() {
        let until = world.agents[&actor]
            .activity
            .expect("travel activity")
            .until;
        world.advance_to(until).expect("finish route step");
        world.continue_intention(actor);
    }

    assert_eq!(world.agents[&actor].location, destination);
    assert_eq!(
        world
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Moved { agent, .. } if agent == actor))
            .count(),
        2
    );
    world.validate().expect("valid completed intention");
}

#[test]
fn llm_rest_and_work_intentions_continue_until_a_boundary() {
    let mut world = World::from_spec(BRIAR_GLEN, 3).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let home = world.agents[&actor].home;
    world.relocate(actor, home);
    world.agents.get_mut(&actor).expect("resident").needs.energy = 0.2;
    world.execute(
        actor,
        ProposedAction::Pursue {
            intention: IntentionGoal::Rest,
        },
    );
    assert!(world.agents[&actor].intention.is_none());
    world.agents.get_mut(&actor).expect("resident").needs.energy = 0.2;

    world.execute_decision(
        actor,
        Decision::llm(ProposedAction::Pursue {
            intention: IntentionGoal::Rest,
        }),
    );
    assert!(world.agents[&actor].intention.is_some());
    assert!(world.continue_intention(actor).is_some());
    assert!(world.agents[&actor].intention.is_some());
    assert!(world.continue_intention(actor).is_some());
    assert!(world.agents[&actor].intention.is_none());
    assert_eq!(world.agents[&actor].routing.llm_intentions_started, 1);
    assert_eq!(world.agents[&actor].routing.llm_intention_steps, 2);
    assert_eq!(world.agents[&actor].routing.llm_intentions_completed, 1);

    let workplace = world.agents[&actor].workplace.expect("workplace");
    world.relocate(actor, workplace);
    world.execute_decision(
        actor,
        Decision::llm(ProposedAction::Pursue {
            intention: IntentionGoal::Work,
        }),
    );
    assert!(world.continue_intention(actor).is_some());
    assert!(world.agents[&actor].intention.is_some());
    world.agents.get_mut(&actor).expect("resident").needs.safety = 0.05;
    assert!(world.continue_intention(actor).is_none());
    assert_eq!(world.agents[&actor].routing.llm_intentions_interrupted, 1);

    world.agents.get_mut(&actor).expect("resident").needs.safety = 1.0;
    world.execute_decision(
        actor,
        Decision::llm(ProposedAction::Pursue {
            intention: IntentionGoal::Work,
        }),
    );
    let expires_at = world.agents[&actor]
        .intention
        .as_ref()
        .expect("work intention")
        .expires_at;
    world.advance_to(expires_at).expect("work boundary");
    assert!(world.agents[&actor].intention.is_none());
    assert_eq!(world.agents[&actor].routing.llm_intentions_completed, 2);
    world.validate().expect("valid intention telemetry");
}

#[test]
fn invalid_and_expired_intentions_clear_safely() {
    let mut world = World::from_spec(BRIAR_GLEN, 3).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let unknown = crate::sim::LocationId(Uuid::nil());
    assert!(matches!(
        world.execute(
            actor,
            ProposedAction::Pursue {
                intention: IntentionGoal::Visit {
                    destination: unknown,
                },
            },
        ),
        ActionResult::Rejected(ActionRejection::UnknownLocation(id)) if id == unknown
    ));
    assert_eq!(world.agents[&actor].intention, None);

    world.agents.get_mut(&actor).expect("resident").intention = Some(Intention {
        goal: IntentionGoal::Rest,
        expires_at: world.tick,
    });
    assert_eq!(world.continue_intention(actor), None);
    assert_eq!(world.agents[&actor].intention, None);
}
