use super::{ActionRejection, AgentId, EventId, LocationId, ObservationTarget, Tick};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub tick: Tick,
    pub location: Option<LocationId>,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    Moved {
        agent: AgentId,
        from: LocationId,
        to: LocationId,
    },
    Spoke {
        speaker: AgentId,
        listener: AgentId,
        message: String,
    },
    Observed {
        observer: AgentId,
        target: ObservationTarget,
    },
    Waited {
        agent: AgentId,
    },
    ActionRejected {
        agent: AgentId,
        reason: ActionRejection,
    },
}
