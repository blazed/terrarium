use crate::sim::{AgentId, LocationId, Needs, Occupation, Personality, Tick, World};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub tick: Tick,
    pub self_description: SelfDescription,
    pub current_location: LocationDescription,
    pub visible_agents: Vec<VisibleAgent>,
    pub goals: Vec<String>,
    pub relevant_memories: Vec<String>,
    pub beliefs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfDescription {
    pub id: AgentId,
    pub name: String,
    pub age: u32,
    pub occupation: Occupation,
    pub personality: Personality,
    pub needs: Needs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationDescription {
    pub id: LocationId,
    pub name: String,
    pub connected: Vec<LocationSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationSummary {
    pub id: LocationId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleAgent {
    pub id: AgentId,
    pub name: String,
    pub occupation: Occupation,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObservationError {
    #[error("unknown observer {0}")]
    UnknownAgent(AgentId),
    #[error("observer is at unknown location {0}")]
    UnknownLocation(LocationId),
    #[error("location contains unknown agent {0}")]
    InvalidVisibleAgent(AgentId),
}

pub fn perceive(world: &World, observer: AgentId) -> Result<AgentObservation, ObservationError> {
    let agent = world
        .agents
        .get(&observer)
        .ok_or(ObservationError::UnknownAgent(observer))?;
    let location = world
        .locations
        .get(&agent.location)
        .ok_or(ObservationError::UnknownLocation(agent.location))?;

    let connected = location
        .connected
        .iter()
        .map(|id| {
            let destination = world
                .locations
                .get(id)
                .ok_or(ObservationError::UnknownLocation(*id))?;
            Ok(LocationSummary {
                id: *id,
                name: destination.name.clone(),
            })
        })
        .collect::<Result<Vec<_>, ObservationError>>()?;
    let visible_agents = location
        .agents
        .iter()
        .filter(|id| **id != observer)
        .map(|id| {
            let visible = world
                .agents
                .get(id)
                .ok_or(ObservationError::InvalidVisibleAgent(*id))?;
            Ok(VisibleAgent {
                id: *id,
                name: visible.name.clone(),
                occupation: visible.occupation.clone(),
            })
        })
        .collect::<Result<Vec<_>, ObservationError>>()?;

    Ok(AgentObservation {
        tick: world.tick,
        self_description: SelfDescription {
            id: agent.id,
            name: agent.name.clone(),
            age: agent.age,
            occupation: agent.occupation.clone(),
            personality: agent.personality.clone(),
            needs: agent.needs.clone(),
        },
        current_location: LocationDescription {
            id: location.id,
            name: location.name.clone(),
            connected,
        },
        visible_agents,
        goals: agent.goals.iter().map(|goal| goal.0.clone()).collect(),
        relevant_memories: Vec::new(),
        beliefs: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::{ObservationError, perceive};
    use crate::sim::{ActionResult, AgentId, ProposedAction, World};
    use uuid::Uuid;

    #[test]
    fn observation_contains_only_local_agents() {
        let mut world = World::briar_glen(9).expect("town");
        let hidden = *world.agents.keys().next().expect("resident");
        let from = world.agents[&hidden].location;
        let destination = *world.locations[&from]
            .connected
            .iter()
            .next()
            .expect("connected location");
        assert!(matches!(
            world.execute(hidden, ProposedAction::Move { destination }),
            ActionResult::Success(_)
        ));
        let observer = *world
            .agents
            .keys()
            .find(|id| **id != hidden)
            .expect("other resident");

        let observation = perceive(&world, observer).expect("observation");
        assert!(
            observation
                .visible_agents
                .iter()
                .all(|agent| agent.id != hidden)
        );
        assert_eq!(observation.visible_agents.len(), 6);
        assert!(observation.relevant_memories.is_empty());
        assert!(observation.beliefs.is_empty());
    }

    #[test]
    fn observation_is_reproducible_and_rejects_unknown_observers() {
        let world = World::briar_glen(10).expect("town");
        let observer = *world.agents.keys().next().expect("resident");
        assert_eq!(
            perceive(&world, observer).expect("observation"),
            perceive(&world, observer).expect("observation")
        );

        let unknown = AgentId(Uuid::nil());
        assert_eq!(
            perceive(&world, unknown),
            Err(ObservationError::UnknownAgent(unknown))
        );
    }
}
