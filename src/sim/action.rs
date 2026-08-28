use super::{AgentId, Event, LocationId, Tick};
use serde::{Deserialize, Serialize};

pub const MAX_TALK_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogueTone {
    Friendly,
    Supportive,
    #[default]
    Neutral,
    Tense,
}

impl std::fmt::Display for DialogueTone {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Friendly => "friendly",
            Self::Supportive => "supportive",
            Self::Neutral => "neutral",
            Self::Tense => "tense",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum ObservationTarget {
    Agent(AgentId),
    Location(LocationId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "goal", rename_all = "snake_case")]
pub enum IntentionGoal {
    Visit {
        destination: LocationId,
    },
    Purchase {
        destination: LocationId,
    },
    Rest,
    Work,
    Talk {
        target: AgentId,
        tone: DialogueTone,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intention {
    pub goal: IntentionGoal,
    pub expires_at: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ProposedAction {
    Move {
        destination: LocationId,
    },
    Talk {
        target: AgentId,
        tone: DialogueTone,
        message: String,
    },
    Confront {
        target: AgentId,
        claim: super::EventId,
    },
    Observe {
        target: ObservationTarget,
    },
    Purchase,
    Rest,
    Work,
    Pursue {
        intention: IntentionGoal,
    },
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
    #[error("agent {0} cannot target themselves")]
    SelfTarget(AgentId),
    #[error("agent {target} is not present with actor {actor}")]
    NotCoLocated { actor: AgentId, target: AgentId },
    #[error("location {target} is not visible from {current}")]
    LocationNotVisible {
        current: LocationId,
        target: LocationId,
    },
    #[error("location {0} is closed")]
    LocationClosed(LocationId),
    #[error("no open route from {from} to {destination}")]
    NoRoute {
        from: LocationId,
        destination: LocationId,
    },
    #[error("agent cannot purchase at location {0}")]
    CannotPurchaseHere(LocationId),
    #[error("business at location {0} is sold out")]
    SoldOut(LocationId),
    #[error("purchase costs {cost} coins but agent has {available}")]
    InsufficientFunds { cost: u64, available: u64 },
    #[error("economy balance overflow")]
    EconomyOverflow,
    #[error("business at {location} cannot cover the {wage}-coin wage; cash is {available}")]
    InsolventEmployer {
        location: LocationId,
        wage: u64,
        available: u64,
    },
    #[error("agent cannot rest at location {0}")]
    CannotRestHere(LocationId),
    #[error("agent cannot work at location {0}")]
    CannotWorkHere(LocationId),
    #[error("agent cannot work at hour {0:02}:00")]
    OutsideWorkingHours(u64),
    #[error("actor does not know rumor claim {0}")]
    UnknownClaim(super::EventId),
    #[error("rumor claim {claim} is not about target {target}")]
    ClaimNotAboutTarget {
        claim: super::EventId,
        target: AgentId,
    },
    #[error("message cannot be empty")]
    EmptyMessage,
    #[error("message must be a single printable line")]
    InvalidMessage,
    #[error("message cannot exceed {max} characters")]
    MessageTooLong { max: usize },
    #[error("world membership for actor {0} is inconsistent")]
    InvalidMembership(AgentId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResult {
    Success(Vec<Event>),
    Rejected(ActionRejection),
}

#[cfg(test)]
mod tests {
    use super::{DialogueTone, ProposedAction};

    #[test]
    fn talk_requires_a_known_tone() {
        let json = |tone: &str| {
            format!(
                r#"{{"action":"talk","target":"00000000-0000-0000-0000-000000000000","tone":"{tone}","message":"Hello"}}"#
            )
        };
        assert!(matches!(
            serde_json::from_str(&json("supportive")).expect("valid talk"),
            ProposedAction::Talk {
                tone: DialogueTone::Supportive,
                ..
            }
        ));
        assert!(serde_json::from_str::<ProposedAction>(&json("hostile")).is_err());
    }
}
