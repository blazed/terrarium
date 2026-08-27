use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::AgentObservation,
    sim::{ObservationTarget, ProposedAction},
};
use rand::{Rng, SeedableRng, rngs::StdRng};

pub struct RandomDecisionEngine {
    rng: StdRng,
}

impl RandomDecisionEngine {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }
}

impl DecisionEngine for RandomDecisionEngine {
    async fn decide(
        &mut self,
        observation: &AgentObservation,
    ) -> Result<ProposedAction, DecisionError> {
        let action = match self.rng.random_range(0..4) {
            0 if !observation.current_location.connected.is_empty() => {
                let index = self
                    .rng
                    .random_range(0..observation.current_location.connected.len());
                ProposedAction::Move {
                    destination: observation.current_location.connected[index].id,
                }
            }
            1 if !observation.visible_agents.is_empty() => {
                let index = self.rng.random_range(0..observation.visible_agents.len());
                ProposedAction::Talk {
                    target: observation.visible_agents[index].id,
                    message: "Good to see you.".into(),
                }
            }
            2 => {
                let target = if observation.visible_agents.is_empty() || self.rng.random_bool(0.5) {
                    ObservationTarget::Location(observation.current_location.id)
                } else {
                    let index = self.rng.random_range(0..observation.visible_agents.len());
                    ObservationTarget::Agent(observation.visible_agents[index].id)
                };
                ProposedAction::Observe { target }
            }
            _ => ProposedAction::Wait,
        };
        Ok(action)
    }
}
