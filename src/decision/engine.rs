use crate::{cognition::AgentObservation, sim::ProposedAction};
use std::future::Future;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecisionError {
    #[error("invalid decision-engine configuration: {0}")]
    Configuration(String),
    #[error("decision-engine request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("decision-engine JSON was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("decision-engine response contained no action output")]
    MissingChoice,
    #[error("decision engine proposed an action outside the current affordances")]
    UnavailableAction,
    #[error("decision engine unavailable: {0}")]
    Unavailable(String),
}

pub trait DecisionEngine {
    fn decide<'a>(
        &'a mut self,
        observation: &'a AgentObservation,
    ) -> impl Future<Output = Result<ProposedAction, DecisionError>> + Send + 'a;
}
