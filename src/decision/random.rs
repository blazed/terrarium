use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::{AgentObservation, VisibleAgent},
    sim::{GoalKind, ObservationTarget, ProposedAction},
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
        let connected_to = |target| {
            observation
                .current_location
                .connected
                .iter()
                .any(|location| location.id == target)
        };
        let move_or_wait = |target| {
            if observation.current_location.id == target {
                ProposedAction::Wait
            } else if connected_to(target) {
                ProposedAction::Move {
                    destination: target,
                }
            } else {
                ProposedAction::Wait
            }
        };
        let relationship_score = |agent: &VisibleAgent| {
            let relationship = agent.relationship;
            relationship.affection
                + relationship.trust
                + relationship.respect
                + relationship.attraction
                - relationship.fear
                - relationship.suspicion
        };
        let preferred_companion = || {
            observation
                .visible_agents
                .iter()
                .max_by(|left, right| {
                    relationship_score(left).total_cmp(&relationship_score(right))
                })
                .expect("visible agents checked")
        };

        if needs.food < 0.25 {
            if observation.current_location.id == observation.self_description.home.id
                || observation.current_location.serves_food
            {
                return Ok(ProposedAction::Eat);
            }
            if let Some(location) = observation
                .current_location
                .connected
                .iter()
                .find(|location| location.serves_food)
            {
                return Ok(ProposedAction::Move {
                    destination: location.id,
                });
            }
            return Ok(move_or_wait(observation.self_description.home.id));
        }
        if needs.energy < 0.2 + 0.1 * personality.neuroticism {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    move_or_wait(observation.self_description.home.id)
                },
            );
        }
        if needs.companionship < 0.2 + 0.1 * personality.agreeableness
            && !observation.visible_agents.is_empty()
        {
            let companion = preferred_companion();
            return Ok(ProposedAction::Talk {
                target: companion.id,
                message: dialogue(observation, companion),
            });
        }
        if needs.safety < 0.1 + 0.2 * personality.neuroticism {
            return Ok(ProposedAction::Observe {
                target: ObservationTarget::Location(observation.current_location.id),
            });
        }

        let hour = observation.tick.hour();
        if !(7..21).contains(&hour) {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    move_or_wait(observation.self_description.home.id)
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
                GoalKind::Livelihood if (8..18).contains(&hour) => {
                    if let Some(workplace) = &observation.self_description.workplace {
                        if observation.current_location.id == workplace.id {
                            return Ok(ProposedAction::Work);
                        }
                        if connected_to(workplace.id) {
                            return Ok(ProposedAction::Move {
                                destination: workplace.id,
                            });
                        }
                    }
                }
                GoalKind::Community if !observation.visible_agents.is_empty() => {
                    let companion = preferred_companion();
                    return Ok(ProposedAction::Talk {
                        target: companion.id,
                        message: dialogue(observation, companion),
                    });
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
                GoalKind::Wellbeing if observation.current_location.serves_food => {
                    return Ok(ProposedAction::Eat);
                }
                _ => {}
            }
        }

        if (8..18).contains(&hour)
            && (needs.money < 0.75 || needs.status < 0.75)
            && let Some(workplace) = &observation.self_description.workplace
        {
            return Ok(if observation.current_location.id == workplace.id {
                ProposedAction::Work
            } else {
                move_or_wait(workplace.id)
            });
        }

        let weights = [
            0.5 + personality.openness + personality.impulsiveness,
            0.5 + personality.agreeableness,
            0.5 + personality.openness + personality.neuroticism,
            1.5 - personality.impulsiveness,
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
            0 if !observation.current_location.connected.is_empty() => {
                let index = rng.random_range(0..observation.current_location.connected.len());
                ProposedAction::Move {
                    destination: observation.current_location.connected[index].id,
                }
            }
            1 if !observation.visible_agents.is_empty() => {
                let companion = preferred_companion();
                ProposedAction::Talk {
                    target: companion.id,
                    message: dialogue(observation, companion),
                }
            }
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

fn dialogue(observation: &AgentObservation, companion: &VisibleAgent) -> String {
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

    match (dominant, alternate) {
        (0, true) => format!("What have you noticed around {location}, {name}?"),
        (0, false) => format!("What else might be worth exploring, {name}?"),
        (1, true) => format!("It's good to see you, {name}."),
        (1, false) => format!("How are you doing today, {name}?"),
        (2, true) => format!("Does everything seem all right here, {name}?"),
        (2, false) => format!("Do you feel safe around {location}, {name}?"),
        (3, true) => format!("I'd value your honest opinion, {name}."),
        (3, false) => format!("Let me speak plainly, {name}: how are things?"),
        (4, true) => format!("What are you working toward, {name}?"),
        (4, false) => format!("How is your work going, {name}?"),
        (5, true) => format!("What should we do next, {name}?"),
        (5, false) => format!("Let's try something different today, {name}."),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::{DecisionEngine, RandomDecisionEngine};
    use crate::{
        cognition::perceive,
        sim::{GoalKind, ObservationTarget, ProposedAction, Relationship, Tick, World},
    };

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
            ProposedAction::Move {
                destination: workplace
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
            ProposedAction::Move {
                destination: heading_home.self_description.home.id
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
            ProposedAction::Talk { target, message }
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
            ProposedAction::Talk { message, .. }
                if message == format!("It's good to see you, {companion_name}.")
        ));

        purposeful.self_description.personality.agreeableness = 0.0;
        purposeful.self_description.personality.ambition = 1.0;
        assert_eq!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Move {
                destination: purposeful
                    .self_description
                    .workplace
                    .as_ref()
                    .expect("workplace")
                    .id
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
