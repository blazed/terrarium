use crate::{cognition::AgentObservation, sim::ProposedAction};
use std::future::Future;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecisionError {
    #[error("decision engine unavailable: {0}")]
    Unavailable(String),
}

pub trait DecisionEngine {
    fn decide<'a>(
        &'a mut self,
        observation: &'a AgentObservation,
    ) -> impl Future<Output = Result<ProposedAction, DecisionError>> + Send + 'a;
}
