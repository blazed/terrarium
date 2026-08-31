use super::{
    ActionRejection, ActionResult, Activity, Agent, AgentId, BUSINESS_STARTING_CASH, Business,
    ConfrontationOutcome, DeathCause, Decision, DecisionSource, DialogueTone, DiseaseState, Event,
    EventId, EventKind, Goal, GoalKind, GoalTarget, Intention, IntentionGoal, Inventory, Item,
    LifeState, Location, LocationId, Loot, MAX_TALK_MESSAGE_CHARS, NEW_WORLD_START_HOUR, Needs,
    ObservationTarget, Occupation, Offering, OpeningHours, Personality, ProposedAction,
    Relationship, RoutingStats, Rumor, STARTING_STOCK, STOCK_PER_SHIFT, Tick, TownEvent,
    TownEventKind, WORK_WAGE, seeded_uuid,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

mod effects;
mod execute;
mod goals;
mod intention;
mod social;
mod time;
mod town;
mod validate;

pub(crate) use social::event_evidence;

const MEMORY_LIMIT: usize = 20;
const RUMOR_LIMIT: usize = 20;
const INTENTION_DURATION_TICKS: u64 = 36;
const GOAL_LIMIT: usize = 3;
const GOAL_DURATION_TICKS: u64 = Tick::PER_DAY;
const PATIENT_ZERO_TICK: u64 = Tick::PER_DAY + 8 * 60 / Tick::MINUTES;
const INCUBATION_TICKS: u64 = Tick::PER_DAY;
const SYMPTOMATIC_TICKS: u64 = 2 * Tick::PER_DAY;
const RECOVERY_TICKS: u64 = Tick::PER_DAY;
const IMMUNITY_TICKS: u64 = 3 * Tick::PER_DAY;
const DISEASE_DAMAGE_PER_TICK: f32 = 0.001;
const CLINIC_PRICE: u64 = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorldError {
    #[error("unknown agent {0}")]
    UnknownAgent(AgentId),
    #[error("unknown location {0}")]
    UnknownLocation(LocationId),
    #[error("invalid world state: {0}")]
    InvalidState(String),
    #[error("tick must move forward from {current:?} to {proposed:?}")]
    NonMonotonicTick { current: Tick, proposed: Tick },
    #[error("simulation tick overflow")]
    TickOverflow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub name: String,
    pub seed: u64,
    pub tick: Tick,
    pub agents: BTreeMap<AgentId, Agent>,
    pub locations: BTreeMap<LocationId, Location>,
    pub active_town_event: Option<TownEvent>,
    events: Vec<Event>,
}

impl World {
    pub fn from_snapshot(
        name: String,
        seed: u64,
        tick: Tick,
        agents: Vec<Agent>,
        locations: Vec<Location>,
        active_town_event: Option<TownEvent>,
        events: Vec<Event>,
    ) -> Result<Self, WorldError> {
        let mut agent_map = BTreeMap::new();
        for agent in agents {
            let id = agent.id;
            if agent_map.insert(id, agent).is_some() {
                return Err(WorldError::InvalidState(format!(
                    "duplicate agent {id} in checkpoint"
                )));
            }
        }
        let mut location_map = BTreeMap::new();
        for location in locations {
            let id = location.id;
            if location_map.insert(id, location).is_some() {
                return Err(WorldError::InvalidState(format!(
                    "duplicate location {id} in checkpoint"
                )));
            }
        }
        let world = Self {
            name,
            seed,
            tick,
            agents: agent_map,
            locations: location_map,
            active_town_event,
            events,
        };
        world.validate()?;
        world.validate_history()?;
        Ok(world)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn execute_decision(&mut self, actor: AgentId, decision: Decision) -> ActionResult {
        match decision.action {
            ProposedAction::Pursue { intention } => {
                self.start_intention(actor, intention, decision.source == DecisionSource::Llm)
            }
            action => self.execute(actor, action),
        }
    }
}

#[cfg(test)]
impl World {
    pub(crate) fn relocate(&mut self, agent: AgentId, destination: LocationId) {
        let source = self.agents[&agent].location;
        self.locations
            .get_mut(&source)
            .expect("source")
            .agents
            .remove(&agent);
        self.locations
            .get_mut(&destination)
            .expect("destination")
            .agents
            .insert(agent);
        self.agents.get_mut(&agent).expect("resident").location = destination;
    }
}

#[cfg(test)]
mod tests;
