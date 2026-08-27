use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::{AgentObservation, VisibleAgent},
    sim::{ObservationTarget, ProposedAction},
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
                .id
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
        if needs.energy < 0.25 {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    move_or_wait(observation.self_description.home.id)
                },
            );
        }
        if needs.companionship < 0.25 && !observation.visible_agents.is_empty() {
            return Ok(ProposedAction::Talk {
                target: preferred_companion(),
                message: "Good to see you.".into(),
            });
        }
        if needs.safety < 0.2 {
            return Ok(ProposedAction::Observe {
                target: ObservationTarget::Location(observation.current_location.id),
            });
        }

        let hour = observation.tick.hour();
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
        if !(7..21).contains(&hour) {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    move_or_wait(observation.self_description.home.id)
                },
            );
        }

        Ok(match rng.random_range(0..4) {
            0 if !observation.current_location.connected.is_empty() => {
                let index = rng.random_range(0..observation.current_location.connected.len());
                ProposedAction::Move {
                    destination: observation.current_location.connected[index].id,
                }
            }
            1 if !observation.visible_agents.is_empty() => ProposedAction::Talk {
                target: preferred_companion(),
                message: "Good to see you.".into(),
            },
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

#[cfg(test)]
mod tests {
    use super::{DecisionEngine, RandomDecisionEngine};
    use crate::{
        cognition::perceive,
        sim::{ProposedAction, Relationship, Tick, World},
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
        assert_eq!(
            engine.decide(&lonely).await.expect("decision"),
            ProposedAction::Talk {
                target: preferred,
                message: "Good to see you.".into()
            }
        );
    }
}
