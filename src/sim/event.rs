use super::{
    ActionRejection, AgentId, DialogueTone, EventId, Item, LocationId, ObservationTarget, Offering,
    Tick, TownEventKind,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub tick: Tick,
    pub location: Option<LocationId>,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfrontationOutcome {
    Confirmed,
    Denied,
    Challenged,
}

impl std::fmt::Display for ConfrontationOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Confirmed => "confirmed",
            Self::Denied => "denied",
            Self::Challenged => "challenged",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    TownEventStarted {
        kind: TownEventKind,
        ends_at: Tick,
    },
    TownEventEnded {
        kind: TownEventKind,
    },
    Moved {
        agent: AgentId,
        from: LocationId,
        to: LocationId,
    },
    Spoke {
        speaker: AgentId,
        listener: AgentId,
        #[serde(default)]
        tone: DialogueTone,
        message: String,
    },
    Confronted {
        accuser: AgentId,
        target: AgentId,
        claim: EventId,
        outcome: ConfrontationOutcome,
    },
    Observed {
        observer: AgentId,
        target: ObservationTarget,
    },
    Purchased {
        agent: AgentId,
        offering: Offering,
        cost: u64,
    },
    ItemUsed {
        agent: AgentId,
        item: Item,
    },
    Rested {
        agent: AgentId,
    },
    Worked {
        agent: AgentId,
        wage: u64,
        stock_produced: u32,
    },
    GoalCompleted {
        agent: AgentId,
        goal: String,
    },
    Waited {
        agent: AgentId,
    },
    ActionRejected {
        agent: AgentId,
        reason: ActionRejection,
    },
}

#[cfg(test)]
mod tests {
    use super::{AgentId, DialogueTone, EventKind};
    use uuid::Uuid;

    #[test]
    fn legacy_dialogue_defaults_to_neutral() {
        let event = EventKind::Spoke {
            speaker: AgentId(Uuid::nil()),
            listener: AgentId(Uuid::max()),
            tone: DialogueTone::Neutral,
            message: "Hello".into(),
        };
        let mut value = serde_json::to_value(event).expect("serialize event");
        value.as_object_mut().expect("event object").remove("tone");

        assert!(matches!(
            serde_json::from_value(value).expect("legacy event"),
            EventKind::Spoke {
                tone: DialogueTone::Neutral,
                ..
            }
        ));
    }
}
