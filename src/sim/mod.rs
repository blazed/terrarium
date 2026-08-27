mod action;
mod agent;
mod event;
mod location;
mod relationship;
mod scheduler;
mod world;

pub use action::{
    ActionRejection, ActionResult, DialogueTone, MAX_TALK_MESSAGE_CHARS, ObservationTarget,
    ProposedAction,
};
pub use agent::{
    Activity, ActivityKind, Agent, Belief, Goal, GoalKind, Needs, Occupation, Personality, Rumor,
};
pub use event::{ConfrontationOutcome, Event, EventKind};
pub use location::{Location, OpeningHours};
pub use relationship::Relationship;
pub use scheduler::Scheduler;
pub use world::{World, WorldError};

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub Uuid);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

domain_id!(AgentId);
domain_id!(LocationId);
domain_id!(EventId);

pub const NEW_WORLD_START_HOUR: u64 = 7;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Tick(pub u64);

impl Tick {
    pub const PER_DAY: u64 = 288;
    pub const MINUTES: u64 = 5;

    pub fn day(self) -> u64 {
        self.0 / Self::PER_DAY + 1
    }

    pub fn hour(self) -> u64 {
        (self.0 % Self::PER_DAY) * Self::MINUTES / 60
    }

    pub fn minute(self) -> u64 {
        (self.0 % Self::PER_DAY) * Self::MINUTES % 60
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Day {} — {:02}:{:02}",
            self.day(),
            self.hour(),
            self.minute()
        )
    }
}

pub(crate) fn seeded_uuid(kind: u8, seed: u64, index: u32) -> Uuid {
    Uuid::from_u128(((kind as u128) << 120) | ((seed as u128) << 32) | index as u128)
}

#[cfg(test)]
mod tests {
    use super::Tick;

    #[test]
    fn tick_formats_simulated_time() {
        assert_eq!(Tick(0).to_string(), "Day 1 — 00:00");
        assert_eq!(Tick(511).to_string(), "Day 2 — 18:35");
    }
}
