use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::{AgentObservation, RouteHint, VisibleAgent},
    sim::{DialogueTone, GoalKind, IntentionGoal, ObservationTarget, ProposedAction},
};
use rand::{Rng, SeedableRng, rngs::StdRng};

pub struct RandomDecisionEngine {
    seed: u64,
}

impl RandomDecisionEngine {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl DecisionEngine for RandomDecisionEngine {
    async fn decide(
        &mut self,
        observation: &AgentObservation,
    ) -> Result<ProposedAction, DecisionError> {
        let mut seed = [0; 32];
        seed[..8].copy_from_slice(&self.seed.to_le_bytes());
        seed[8..16].copy_from_slice(&observation.tick.0.to_le_bytes());
        seed[16..].copy_from_slice(observation.self_description.id.0.as_bytes());
        let mut rng = StdRng::from_seed(seed);

        let needs = &observation.self_description.needs;
        let personality = &observation.self_description.personality;
        let mood = observation.self_description.mood;
        let follow_route = |hint: Option<RouteHint>, goal: fn(RouteHint) -> IntentionGoal| {
            hint.filter(|hint| {
                observation
                    .action_affordances
                    .move_to
                    .contains(&hint.next_hop)
            })
            .map_or(ProposedAction::Wait, |hint| ProposedAction::Pursue {
                intention: goal(hint),
            })
        };
        let relationship_score = |agent: &VisibleAgent| {
            let relationship = agent.relationship;
            let belief = observation
                .beliefs
                .get(&agent.id)
                .map(|belief| {
                    0.5 * belief.confidence
                        * (belief.sociability - 0.5 + belief.reliability - 0.5 - belief.hostility)
                })
                .unwrap_or_default();
            relationship.affection
                + relationship.trust
                + relationship.respect
                + relationship.attraction
                - relationship.fear
                - relationship.suspicion
                + belief
        };
        let preferred_companion = || {
            observation
                .visible_agents
                .iter()
                .filter(|agent| observation.action_affordances.talk_to.contains(&agent.id))
                .max_by(|left, right| {
                    relationship_score(left).total_cmp(&relationship_score(right))
                })
        };

        if needs.food < 0.25 {
            if observation.action_affordances.can_eat {
                return Ok(ProposedAction::Eat);
            }
            return Ok(follow_route(observation.route_hints.food, |hint| {
                IntentionGoal::Eat {
                    destination: hint.destination,
                }
            }));
        }
        if needs.energy < 0.2 + 0.1 * personality.neuroticism {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, |_| IntentionGoal::Rest)
                },
            );
        }
        if needs.companionship < 0.2 + 0.1 * personality.agreeableness
            && let Some(companion) = preferred_companion()
        {
            return Ok(talk(observation, companion));
        }
        if needs.safety < 0.1 + 0.2 * personality.neuroticism {
            return Ok(ProposedAction::Observe {
                target: ObservationTarget::Location(observation.current_location.id),
            });
        }

        if let Some(confrontation) =
            observation
                .action_affordances
                .confront
                .iter()
                .max_by(|left, right| {
                    let confidence = |claim| {
                        observation
                            .rumors
                            .iter()
                            .find(|rumor| rumor.claim == claim)
                            .map_or(0.0, |rumor| rumor.confidence)
                    };
                    confidence(left.claim).total_cmp(&confidence(right.claim))
                })
        {
            return Ok(ProposedAction::Confront {
                target: confrontation.target,
                claim: confrontation.claim,
            });
        }

        let hour = observation.tick.hour();
        if !(7..21).contains(&hour) {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, |_| IntentionGoal::Rest)
                },
            );
        }

        let mut goal_priorities = [
            (GoalKind::Livelihood, personality.ambition),
            (GoalKind::Community, personality.agreeableness),
            (
                GoalKind::Exploration,
                (personality.openness + personality.impulsiveness) / 2.0,
            ),
            (GoalKind::Wellbeing, personality.neuroticism),
        ];
        goal_priorities.sort_by(|left, right| right.1.total_cmp(&left.1));
        for (goal_kind, _) in goal_priorities {
            if !observation
                .goals
                .iter()
                .any(|goal| goal.kind == goal_kind && goal.progress < 1.0)
            {
                continue;
            }
            match goal_kind {
                GoalKind::Livelihood => {
                    if let Some(workplace) = &observation.self_description.workplace
                        && workplace.is_open
                    {
                        if observation.action_affordances.can_work {
                            return Ok(ProposedAction::Work);
                        }
                        if observation.route_hints.workplace.is_some() {
                            return Ok(follow_route(observation.route_hints.workplace, |_| {
                                IntentionGoal::Work
                            }));
                        }
                    }
                }
                GoalKind::Community => {
                    if let Some(companion) = preferred_companion() {
                        return Ok(talk(observation, companion));
                    }
                }
                GoalKind::Exploration => {
                    return Ok(ProposedAction::Observe {
                        target: ObservationTarget::Location(observation.current_location.id),
                    });
                }
                GoalKind::Wellbeing
                    if observation.current_location.id == observation.self_description.home.id =>
                {
                    return Ok(if needs.food <= needs.energy {
                        ProposedAction::Eat
                    } else {
                        ProposedAction::Rest
                    });
                }
                GoalKind::Wellbeing if observation.action_affordances.can_eat => {
                    return Ok(ProposedAction::Eat);
                }
                GoalKind::Wellbeing if observation.route_hints.food.is_some() => {
                    return Ok(follow_route(observation.route_hints.food, |hint| {
                        IntentionGoal::Eat {
                            destination: hint.destination,
                        }
                    }));
                }
                _ => {}
            }
        }

        if (needs.money < 0.75 || needs.status < 0.75)
            && let Some(workplace) = &observation.self_description.workplace
            && workplace.is_open
        {
            return Ok(if observation.action_affordances.can_work {
                ProposedAction::Work
            } else {
                follow_route(observation.route_hints.workplace, |_| IntentionGoal::Work)
            });
        }

        let weights = [
            0.5 + personality.openness + personality.impulsiveness + mood.max(0.0),
            0.5 + personality.agreeableness + mood.max(0.0),
            0.5 + personality.openness + personality.neuroticism + (-mood).max(0.0),
            1.5 - personality.impulsiveness + (-mood).max(0.0),
        ];
        let mut choice = rng.random::<f32>() * weights.iter().sum::<f32>();
        let action = weights
            .iter()
            .position(|weight| {
                choice -= weight;
                choice <= 0.0
            })
            .unwrap_or(3);

        Ok(match action {
            0 if !observation.action_affordances.move_to.is_empty() => {
                let index = rng.random_range(0..observation.action_affordances.move_to.len());
                ProposedAction::Move {
                    destination: observation.action_affordances.move_to[index],
                }
            }
            1 if preferred_companion().is_some() => talk(
                observation,
                preferred_companion().expect("available companion"),
            ),
            2 => {
                let target = if observation.visible_agents.is_empty() || rng.random_bool(0.5) {
                    ObservationTarget::Location(observation.current_location.id)
                } else {
                    let index = rng.random_range(0..observation.visible_agents.len());
                    ObservationTarget::Agent(observation.visible_agents[index].id)
                };
                ProposedAction::Observe { target }
            }
            _ => ProposedAction::Wait,
        })
    }
}

fn talk(observation: &AgentObservation, companion: &VisibleAgent) -> ProposedAction {
    let tone = dialogue_tone(observation, companion);
    let personality = &observation.self_description.personality;
    let dominant = [
        personality.openness,
        personality.agreeableness,
        personality.neuroticism,
        personality.honesty,
        personality.ambition,
        personality.impulsiveness,
    ]
    .into_iter()
    .enumerate()
    .max_by(|left, right| left.1.total_cmp(&right.1))
    .expect("personality has traits")
    .0;
    let alternate = observation.tick.0.is_multiple_of(2);
    let name = &companion.name;
    let location = &observation.current_location.name;

    let message = match (tone, dominant, alternate) {
        (DialogueTone::Friendly, _, true) => format!("It's good to see you, {name}."),
        (DialogueTone::Friendly, _, false) => format!("How are you doing today, {name}?"),
        (DialogueTone::Supportive, _, true) => format!("How can I help you today, {name}?"),
        (DialogueTone::Supportive, _, false) => {
            format!("I'm here if you need support around {location}, {name}.")
        }
        (DialogueTone::Tense, _, true) => format!("We need to clear something up, {name}."),
        (DialogueTone::Tense, _, false) => format!("I don't trust this situation, {name}."),
        (DialogueTone::Neutral, 0, true) => {
            format!("What have you noticed around {location}, {name}?")
        }
        (DialogueTone::Neutral, 0, false) => {
            format!("What else might be worth exploring, {name}?")
        }
        (DialogueTone::Neutral, 1, true) => format!("It's good to see you, {name}."),
        (DialogueTone::Neutral, 1, false) => format!("How are you doing today, {name}?"),
        (DialogueTone::Neutral, 2, true) => {
            format!("Does everything seem all right here, {name}?")
        }
        (DialogueTone::Neutral, 2, false) => {
            format!("Do you feel safe around {location}, {name}?")
        }
        (DialogueTone::Neutral, 3, true) => format!("I'd value your honest opinion, {name}."),
        (DialogueTone::Neutral, 3, false) => {
            format!("Let me speak plainly, {name}: how are things?")
        }
        (DialogueTone::Neutral, 4, true) => format!("What are you working toward, {name}?"),
        (DialogueTone::Neutral, 4, false) => format!("How is your work going, {name}?"),
        (DialogueTone::Neutral, 5, true) => format!("What should we do next, {name}?"),
        (DialogueTone::Neutral, 5, false) => {
            format!("Let's try something different today, {name}.")
        }
        _ => unreachable!(),
    };
    ProposedAction::Talk {
        target: companion.id,
        tone,
        message,
    }
}

fn dialogue_tone(observation: &AgentObservation, companion: &VisibleAgent) -> DialogueTone {
    let personality = &observation.self_description.personality;
    let relationship = companion.relationship;
    let belief = observation
        .beliefs
        .get(&companion.id)
        .copied()
        .unwrap_or_default();
    let closeness = relationship.affection + relationship.trust + relationship.respect
        - relationship.fear
        - relationship.suspicion;

    if closeness < -0.5
        || belief.hostility * belief.confidence > 0.35
        || (observation.self_description.mood < -0.5 && personality.agreeableness < 0.7)
        || (personality.impulsiveness > 0.75 && personality.agreeableness < 0.35)
    {
        DialogueTone::Tense
    } else if belief.reliability * belief.confidence > 0.35
        || personality.agreeableness + personality.honesty >= 1.25
    {
        DialogueTone::Supportive
    } else if observation.self_description.mood > 0.35
        || closeness > 0.4
        || personality.agreeableness >= 0.55
    {
        DialogueTone::Friendly
    } else {
        DialogueTone::Neutral
    }
}

#[cfg(test)]
mod tests {
    use super::{DecisionEngine, RandomDecisionEngine};
    use crate::{
        cognition::{ConfrontationAffordance, RumorSummary, perceive},
        sim::{
            Belief, DialogueTone, EventId, GoalKind, IntentionGoal, ObservationTarget,
            ProposedAction, Relationship, Tick, World,
        },
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn confident_beliefs_shape_companion_and_tone() {
        let world = World::briar_glen(17).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs.food = 0.5;
        observation.self_description.needs.energy = 0.5;
        observation.self_description.needs.companionship = 0.1;
        observation.self_description.needs.safety = 0.5;
        for visible in &mut observation.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        let preferred = observation.visible_agents[0].id;
        observation.beliefs.insert(
            preferred,
            Belief {
                sociability: 1.0,
                reliability: 1.0,
                hostility: 0.0,
                confidence: 1.0,
            },
        );
        let mut engine = RandomDecisionEngine::new(17);
        assert!(matches!(
            engine.decide(&observation).await.expect("decision"),
            ProposedAction::Talk { target, .. } if target == preferred
        ));

        observation
            .visible_agents
            .retain(|agent| agent.id == preferred);
        observation.beliefs.insert(
            preferred,
            Belief {
                sociability: 0.5,
                reliability: 0.5,
                hostility: 1.0,
                confidence: 1.0,
            },
        );
        assert!(matches!(
            engine.decide(&observation).await.expect("decision"),
            ProposedAction::Talk {
                tone: DialogueTone::Tense,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn credible_rumors_trigger_confrontations() {
        let world = World::briar_glen(18).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs.food = 0.5;
        observation.self_description.needs.energy = 0.5;
        observation.self_description.needs.companionship = 0.5;
        observation.self_description.needs.safety = 0.5;
        let target = observation.visible_agents[0].id;
        let claim = EventId(Uuid::nil());
        observation.action_affordances.confront = vec![ConfrontationAffordance { target, claim }];
        observation.rumors = vec![RumorSummary {
            claim,
            subject: Some(target),
            report: "A known report".into(),
            source: "A resident".into(),
            depth: 1,
            confidence: 0.8,
            resolved: false,
        }];

        assert_eq!(
            RandomDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("decision"),
            ProposedAction::Confront { target, claim }
        );
    }

    #[tokio::test]
    async fn routines_follow_multi_hop_route_hints() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = world
            .agents
            .values()
            .find(|agent| agent.name == "Clara Voss")
            .map(|agent| agent.id)
            .expect("resident");
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 1.0;
        agent.needs.companionship = 1.0;
        agent.needs.safety = 1.0;
        agent.personality.ambition = 1.0;
        agent.personality.openness = 0.0;
        agent.personality.agreeableness = 0.0;
        agent.personality.neuroticism = 0.0;
        agent.personality.impulsiveness = 0.0;
        world.advance_to(Tick(8 * 12)).expect("morning");

        let observation = perceive(&world, actor).expect("observation");
        let next_hop = observation.route_hints.workplace.expect("route");
        assert_ne!(next_hop.next_hop, next_hop.destination);
        assert_eq!(
            next_hop.destination,
            observation
                .self_description
                .workplace
                .as_ref()
                .expect("workplace")
                .id
        );
        assert_eq!(
            RandomDecisionEngine::new(7)
                .decide(&observation)
                .await
                .expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );
    }

    #[tokio::test]
    async fn urgent_needs_and_time_drive_routines() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut engine = RandomDecisionEngine::new(7);

        let hungry = perceive(&world, actor).expect("observation");
        assert_eq!(
            engine.decide(&hungry).await.expect("decision"),
            ProposedAction::Eat
        );
        let mut tired = hungry.clone();
        tired.self_description.needs.food = 0.5;
        tired.self_description.needs.energy = 0.1;
        assert_eq!(
            engine.decide(&tired).await.expect("decision"),
            ProposedAction::Rest
        );

        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 0.5;
        agent.needs.companionship = 0.5;
        agent.needs.safety = 0.5;
        agent.personality.openness = 0.0;
        agent.personality.agreeableness = 0.0;
        agent.personality.neuroticism = 0.0;
        agent.personality.ambition = 1.0;
        agent.personality.impulsiveness = 0.0;
        world.advance_to(Tick(8 * 12)).expect("morning");
        let working = perceive(&world, actor).expect("observation");
        let workplace = working
            .self_description
            .workplace
            .as_ref()
            .expect("workplace")
            .id;
        assert_eq!(
            engine.decide(&working).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );

        world.execute(
            actor,
            ProposedAction::Move {
                destination: workplace,
            },
        );
        assert_eq!(
            engine
                .decide(&perceive(&world, actor).expect("work observation"))
                .await
                .expect("decision"),
            ProposedAction::Work
        );
        world.advance_to(Tick(21 * 12)).expect("night");
        let heading_home = perceive(&world, actor).expect("observation");
        assert_eq!(
            engine.decide(&heading_home).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Rest,
            }
        );

        let mut lonely = hungry;
        lonely.tick = Tick(12 * 12);
        lonely.self_description.needs.food = 0.5;
        lonely.self_description.needs.energy = 0.5;
        lonely.self_description.needs.companionship = 0.1;
        lonely.self_description.needs.safety = 0.5;
        for visible in &mut lonely.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        let preferred = lonely.visible_agents[0].id;
        lonely.visible_agents[0].relationship.affection = 1.0;
        let companion_name = lonely.visible_agents[0].name.clone();
        assert!(matches!(
            engine.decide(&lonely).await.expect("decision"),
            ProposedAction::Talk { target, message, .. }
                if target == preferred && message.contains(&companion_name)
        ));

        let mut purposeful = lonely;
        purposeful.self_description.needs.companionship = 0.5;
        for goal in &mut purposeful.goals {
            goal.progress = 1.0;
        }
        purposeful
            .goals
            .iter_mut()
            .find(|goal| goal.kind == GoalKind::Exploration)
            .expect("exploration goal")
            .progress = 0.0;
        assert_eq!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Observe {
                target: ObservationTarget::Location(purposeful.current_location.id)
            }
        );

        for goal in &mut purposeful.goals {
            goal.progress = 0.0;
        }
        let personality = &mut purposeful.self_description.personality;
        personality.openness = 0.0;
        personality.agreeableness = 1.0;
        personality.neuroticism = 0.0;
        personality.ambition = 0.0;
        personality.impulsiveness = 0.0;
        assert!(matches!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Talk {
                tone: DialogueTone::Supportive,
                message,
                ..
            } if message.contains(&companion_name)
        ));
        for visible in &mut purposeful.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        purposeful.self_description.personality.agreeableness = 0.6;
        purposeful.self_description.personality.honesty = 0.4;
        purposeful.self_description.mood = -1.0;
        assert!(matches!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Talk {
                tone: DialogueTone::Tense,
                ..
            }
        ));

        purposeful.self_description.mood = 0.0;
        purposeful.self_description.personality.agreeableness = 0.0;
        purposeful.self_description.personality.ambition = 1.0;
        assert_eq!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );

        purposeful.self_description.personality.ambition = 0.0;
        purposeful.self_description.personality.openness = 1.0;
        assert!(matches!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Observe { .. }
        ));
    }
}
