use super::{AgentId, Event, LocationId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum ObservationTarget {
    Agent(AgentId),
    Location(LocationId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ProposedAction {
    Move { destination: LocationId },
    Talk { target: AgentId, message: String },
    Observe { target: ObservationTarget },
    Wait,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ActionRejection {
    #[error("unknown actor {0}")]
    UnknownActor(AgentId),
    #[error("unknown agent {0}")]
    UnknownAgent(AgentId),
    #[error("unknown location {0}")]
    UnknownLocation(LocationId),
    #[error("destination {destination} is not connected to {from}")]
    Disconnected {
        from: LocationId,
        destination: LocationId,
    },
    #[error("agent {target} is not present with actor {actor}")]
    NotCoLocated { actor: AgentId, target: AgentId },
    #[error("location {target} is not visible from {current}")]
    LocationNotVisible {
        current: LocationId,
        target: LocationId,
    },
    #[error("message cannot be empty")]
    EmptyMessage,
    #[error("world membership for actor {0} is inconsistent")]
    InvalidMembership(AgentId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResult {
    Success(Vec<Event>),
    Rejected(ActionRejection),
}
