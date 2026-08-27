use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::AgentObservation,
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

        let action = match rng.random_range(0..4) {
            0 if !observation.current_location.connected.is_empty() => {
                let index = rng.random_range(0..observation.current_location.connected.len());
                ProposedAction::Move {
                    destination: observation.current_location.connected[index].id,
                }
            }
            1 if !observation.visible_agents.is_empty() => {
                let index = rng.random_range(0..observation.visible_agents.len());
                ProposedAction::Talk {
                    target: observation.visible_agents[index].id,
                    message: "Good to see you.".into(),
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
        };
        Ok(action)
    }
}
