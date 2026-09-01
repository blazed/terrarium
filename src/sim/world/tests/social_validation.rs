use super::*;

#[test]
fn confrontations_confirm_deny_and_reject_invalid_claims() {
    fn world_with_rumor(honesty: f32) -> (World, AgentId, AgentId, EventId) {
        let mut world = World::from_spec(BRIAR_GLEN, 43).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let target = residents[0];
        let accuser = residents[2];
        world.relocate(accuser, world.agents[&target].location);
        world
            .agents
            .get_mut(&target)
            .expect("target")
            .personality
            .honesty = honesty;
        let fact = world.append_event(
            None,
            EventKind::Worked {
                agent: target,
                wage: WORK_WAGE,
                stock_produced: 0,
            },
        );
        world.execute(
            target,
            ProposedAction::Talk {
                target: accuser,
                tone: DialogueTone::Neutral,
                message: "Work went well.".into(),
            },
        );
        (world, accuser, target, fact.id)
    }

    let (mut world, accuser, target, claim) = world_with_rumor(1.0);
    let old_confidence = world.agents[&accuser].rumors[0].confidence;
    let result = world.execute(accuser, ProposedAction::Confront { target, claim });
    assert!(matches!(
        result,
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Confronted {
                outcome: ConfrontationOutcome::Confirmed,
                ..
            })
    ));
    assert!(world.agents[&accuser].rumors[0].confidence > old_confidence);

    let (mut world, accuser, target, claim) = world_with_rumor(0.0);
    let old_trust = world.agents[&target]
        .relationships
        .get(&accuser)
        .expect("conversation relationship")
        .trust;
    assert!(matches!(
        world.execute(accuser, ProposedAction::Confront { target, claim }),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Confronted {
                outcome: ConfrontationOutcome::Denied,
                ..
            })
    ));
    assert!(world.agents[&target].relationships[&accuser].trust < old_trust);
    assert!(matches!(
        world.execute(
            accuser,
            ProposedAction::Confront {
                target,
                claim: EventId(Uuid::nil()),
            },
        ),
        ActionResult::Rejected(ActionRejection::UnknownClaim(_))
    ));

    world.agents.get_mut(&accuser).expect("accuser").rumors[0].resolved = false;
    let wrong_target = world
        .agents
        .keys()
        .copied()
        .find(|id| *id != accuser && *id != target)
        .expect("third resident");
    world.relocate(wrong_target, world.agents[&accuser].location);
    assert!(matches!(
        world.execute(
            accuser,
            ProposedAction::Confront {
                target: wrong_target,
                claim,
            },
        ),
        ActionResult::Rejected(ActionRejection::ClaimNotAboutTarget { .. })
    ));

    let (mut world, accuser, target, claim) = world_with_rumor(0.5);
    assert!(matches!(
        world.execute(accuser, ProposedAction::Confront { target, claim }),
        ActionResult::Success(ref events)
            if matches!(events[0].kind, EventKind::Confronted {
                outcome: ConfrontationOutcome::Challenged,
                ..
            })
    ));
}

#[test]
fn conversations_build_bounded_mutual_relationships() {
    let mut world = World::from_spec(BRIAR_GLEN, 42).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let speaker = residents[0];
    let listener = residents[2];
    world.relocate(listener, world.agents[&speaker].location);
    assert!(!world.agents[&speaker].relationships.contains_key(&listener));
    assert!(!world.agents[&listener].relationships.contains_key(&speaker));

    for _ in 0..200 {
        assert!(matches!(
            world.execute(
                speaker,
                ProposedAction::Talk {
                    target: listener,
                    tone: DialogueTone::Neutral,
                    message: "Good to see you.".into(),
                }
            ),
            ActionResult::Success(_)
        ));
    }

    let speaker_view = world.agents[&speaker].relationships[&listener];
    let listener_view = world.agents[&listener].relationships[&speaker];
    assert_eq!(speaker_view.affection, 1.0);
    assert_eq!(listener_view.affection, 1.0);
    assert_eq!(speaker_view.suspicion, -1.0);
    assert!(speaker_view.is_normalized() && listener_view.is_normalized());
    world.validate().expect("valid relationships");
}

#[test]
fn agreeable_honest_speakers_build_relationships_faster() {
    let mut warm = World::from_spec(BRIAR_GLEN, 42).expect("town");
    let residents = warm.agents.keys().copied().collect::<Vec<_>>();
    let speaker = residents[0];
    let listener = residents[2];
    warm.relocate(listener, warm.agents[&speaker].location);
    let mut cold = warm.clone();
    warm.agents
        .get_mut(&speaker)
        .expect("speaker")
        .personality
        .agreeableness = 1.0;
    warm.agents
        .get_mut(&speaker)
        .expect("speaker")
        .personality
        .honesty = 1.0;
    cold.agents
        .get_mut(&speaker)
        .expect("speaker")
        .personality
        .agreeableness = 0.0;
    cold.agents
        .get_mut(&speaker)
        .expect("speaker")
        .personality
        .honesty = 0.0;
    let talk = ProposedAction::Talk {
        target: listener,
        tone: DialogueTone::Neutral,
        message: "Good to see you.".into(),
    };

    warm.execute(speaker, talk.clone());
    cold.execute(speaker, talk);

    let warm_view = warm.agents[&speaker].relationships[&listener];
    let cold_view = cold.agents[&speaker].relationships[&listener];
    assert!(warm_view.affection > cold_view.affection);
    assert!(warm_view.trust > cold_view.trust);
    assert!(warm_view.suspicion < cold_view.suspicion);
}

#[test]
fn dialogue_tone_changes_relationship_effects() {
    let mut neutral = World::from_spec(BRIAR_GLEN, 42).expect("town");
    let residents = neutral.agents.keys().copied().collect::<Vec<_>>();
    let speaker = residents[0];
    let listener = residents[2];
    neutral.relocate(listener, neutral.agents[&speaker].location);
    let mut friendly = neutral.clone();
    let mut supportive = neutral.clone();
    let mut tense = neutral;

    for (world, tone) in [
        (&mut friendly, DialogueTone::Friendly),
        (&mut supportive, DialogueTone::Supportive),
        (&mut tense, DialogueTone::Tense),
    ] {
        world.execute(
            speaker,
            ProposedAction::Talk {
                target: listener,
                tone,
                message: "Hello.".into(),
            },
        );
    }

    assert!(friendly.agents[&speaker].mood > 0.0);
    assert!(supportive.agents[&listener].mood > supportive.agents[&speaker].mood);
    assert!(tense.agents[&speaker].mood < 0.0);
    let friendly = friendly.agents[&speaker].relationships[&listener];
    let supportive = supportive.agents[&speaker].relationships[&listener];
    let tense = tense.agents[&speaker].relationships[&listener];
    assert!(friendly.affection > supportive.affection);
    assert!(supportive.trust > friendly.trust);
    assert!(tense.affection < 0.0 && tense.trust < 0.0 && tense.suspicion > 0.0);
}

#[test]
fn dialogue_is_trimmed_and_bounded_to_one_printable_line() {
    let mut world = World::from_spec(BRIAR_GLEN, 43).expect("town");
    let residents = world.agents.keys().copied().collect::<Vec<_>>();
    let actor = residents[0];
    let listener = residents[1];
    world.relocate(listener, world.agents[&actor].location);

    for (message, rejection) in [
        ("   ".into(), ActionRejection::EmptyMessage),
        ("hello\nthere".into(), ActionRejection::InvalidMessage),
        (
            "x".repeat(MAX_TALK_MESSAGE_CHARS + 1),
            ActionRejection::MessageTooLong {
                max: MAX_TALK_MESSAGE_CHARS,
            },
        ),
    ] {
        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Talk {
                    target: listener,
                    tone: DialogueTone::Neutral,
                    message,
                },
            ),
            ActionResult::Rejected(rejection)
        );
    }

    let result = world.execute(
        actor,
        ProposedAction::Talk {
            target: listener,
            tone: DialogueTone::Friendly,
            message: "  A concise greeting.  ".into(),
        },
    );
    assert!(matches!(
        result,
        ActionResult::Success(events)
            if matches!(&events[0].kind, EventKind::Spoke { message, .. } if message == "A concise greeting.")
    ));
}

#[test]
fn invalid_relationship_targets_and_self_talk_are_rejected() {
    let mut world = World::from_spec(BRIAR_GLEN, 43).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    assert_eq!(
        world.execute(
            actor,
            ProposedAction::Talk {
                target: actor,
                tone: DialogueTone::Neutral,
                message: "Hello, me.".into(),
            }
        ),
        ActionResult::Rejected(ActionRejection::SelfTarget(actor))
    );

    world
        .agents
        .get_mut(&actor)
        .expect("resident")
        .relationships
        .insert(actor, Relationship::NEUTRAL);
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));

    world
        .agents
        .get_mut(&actor)
        .expect("resident")
        .relationships
        .remove(&actor);
    world
        .agents
        .get_mut(&actor)
        .expect("resident")
        .relationships
        .insert(AgentId(Uuid::nil()), Relationship::NEUTRAL);
    assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
}

#[test]
fn memories_keep_only_the_latest_twenty_events() {
    let mut world = World::from_spec(BRIAR_GLEN, 42).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let location = world.agents[&actor].location;

    for _ in 0..21 {
        world.execute(
            actor,
            ProposedAction::Observe {
                target: ObservationTarget::Location(location),
            },
        );
    }

    assert_eq!(world.agents[&actor].memories.len(), 20);
    assert_eq!(
        world.agents[&actor].memories,
        world.events()[world.events().len() - 20..]
    );
}

#[test]
fn rejected_action_changes_only_history_and_actor_mood() {
    let mut world = World::from_spec(BRIAR_GLEN, 5).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let agents_before = world.agents.clone();
    let locations_before = world.locations.clone();
    let absent = AgentId(Uuid::nil());

    assert_eq!(
        world.execute(
            actor,
            ProposedAction::Talk {
                target: absent,
                tone: DialogueTone::Neutral,
                message: "Hello".into(),
            },
        ),
        ActionResult::Rejected(ActionRejection::UnknownAgent(absent))
    );
    assert_eq!(world.agents[&actor].mood, -0.06);
    world.agents.get_mut(&actor).expect("actor").mood = 0.0;
    assert_eq!(world.agents, agents_before);
    assert_eq!(world.locations, locations_before);
    assert!(matches!(
        world.events[0].kind,
        EventKind::ActionRejected { .. }
    ));
}

#[test]
fn disconnected_move_is_rejected_without_mutation() {
    let mut world = World::from_spec(BRIAR_GLEN, 6).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let from = world.agents[&actor].location;
    let destination = *world
        .locations
        .keys()
        .find(|id| **id != from && !world.locations[&from].connected.contains(id))
        .expect("disconnected location");
    let agents_before = world.agents.clone();
    let locations_before = world.locations.clone();

    assert!(matches!(
        world.execute(actor, ProposedAction::Move { destination }),
        ActionResult::Rejected(ActionRejection::Disconnected { .. })
    ));
    assert_eq!(world.agents[&actor].mood, -0.06);
    world.agents.get_mut(&actor).expect("actor").mood = 0.0;
    assert_eq!(world.agents, agents_before);
    assert_eq!(world.locations, locations_before);
}

#[test]
fn known_but_absent_agent_cannot_be_addressed() {
    let mut world = World::from_spec(BRIAR_GLEN, 7).expect("town");
    let actor = *world.agents.keys().next().expect("resident");
    let target = *world
        .agents
        .keys()
        .find(|id| **id != actor)
        .expect("other resident");
    let from = world.agents[&target].location;
    let destination = *world.locations[&from]
        .connected
        .iter()
        .find(|id| world.locations[id].is_open(world.tick.hour()))
        .expect("open connected location");
    assert!(matches!(
        world.execute(target, ProposedAction::Move { destination }),
        ActionResult::Success(_)
    ));
    assert_eq!(
        world.execute(
            actor,
            ProposedAction::Talk {
                target,
                tone: DialogueTone::Neutral,
                message: "Can you hear me?".into(),
            }
        ),
        ActionResult::Rejected(ActionRejection::NotCoLocated { actor, target })
    );
}

#[test]
fn crime_rumors_spread_louder_than_neutral_gossip() {
    // Identical speaker/trust profiles and listener across both worlds: a crime
    // memory gets a higher shared confidence than a neutral one (crime base 1.0
    // vs 0.9), so the sheriff hears about thefts before ordinary chatter.
    fn shared_confidence(crime: bool) -> f32 {
        let mut world = World::from_spec(BRIAR_GLEN, 44).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[1];
        world
            .agents
            .get_mut(&speaker)
            .expect("speaker")
            .personality
            .honesty = 0.8;
        let event = if crime {
            let thief = residents[2];
            let victim = residents[3];
            world.append_event(
                None,
                EventKind::Stole {
                    thief,
                    victim,
                    loot: Loot::Coins(1),
                },
            )
        } else {
            world.append_event(
                None,
                EventKind::Worked {
                    agent: speaker,
                    wage: WORK_WAGE,
                    stock_produced: 0,
                },
            )
        };
        world
            .agents
            .get_mut(&speaker)
            .expect("speaker")
            .memories
            .push(event);
        world.share_rumor(speaker, listener);
        world.agents[&listener].rumors[0].confidence
    }
    assert!(
        shared_confidence(true) > shared_confidence(false),
        "crime gossip must spread louder than neutral gossip"
    );
}
