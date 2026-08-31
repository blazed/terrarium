use super::{
    ActionRejection, AgentId, DeathCause, DialogueTone, EventId, Item, LocationId, Loot,
    ObservationTarget, Offering, Tick, TownEventKind,
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
    ItemGiven {
        giver: AgentId,
        receiver: AgentId,
        item: Item,
    },
    Stole {
        thief: AgentId,
        victim: AgentId,
        loot: Loot,
    },
    TheftFailed {
        thief: AgentId,
        victim: AgentId,
        loot: Loot,
    },
    Robbed {
        victim: AgentId,
        loot: Loot,
    },
    Assaulted {
        attacker: AgentId,
        victim: AgentId,
    },
    Treated {
        agent: AgentId,
        cost: u64,
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
    Died {
        agent: AgentId,
        cause: DeathCause,
    },
    DiseaseInfected {
        agent: AgentId,
        source: Option<AgentId>,
    },
    DiseaseSymptoms {
        agent: AgentId,
    },
    DiseaseRecovered {
        agent: AgentId,
    },
    DiseaseImmunityExpired {
        agent: AgentId,
    },
    ActionRejected {
        agent: AgentId,
        reason: ActionRejection,
    },
}
