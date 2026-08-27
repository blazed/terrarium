use crate::sim::{
    AgentId, Event, EventKind, GoalKind, LocationId, Needs, ObservationTarget, Occupation,
    Personality, Relationship, Tick, World,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub tick: Tick,
    pub self_description: SelfDescription,
    pub current_location: LocationDescription,
    pub visible_agents: Vec<VisibleAgent>,
    pub goals: Vec<GoalStatus>,
    pub relevant_memories: Vec<String>,
    pub beliefs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalStatus {
    pub description: String,
    pub kind: GoalKind,
    pub progress: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfDescription {
    pub id: AgentId,
    pub name: String,
    pub age: u32,
    pub occupation: Occupation,
    pub home: LocationSummary,
    pub workplace: Option<LocationSummary>,
    pub personality: Personality,
    pub needs: Needs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationDescription {
    pub id: LocationId,
    pub name: String,
    pub serves_food: bool,
    pub connected: Vec<LocationSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationSummary {
    pub id: LocationId,
    pub name: String,
    pub serves_food: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleAgent {
    pub id: AgentId,
    pub name: String,
    pub occupation: Occupation,
    pub relationship: Relationship,
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

    let summarize_location = |id: LocationId| {
        let location = world
            .locations
            .get(&id)
            .ok_or(ObservationError::UnknownLocation(id))?;
        Ok(LocationSummary {
            id,
            name: location.name.clone(),
            serves_food: location.serves_food,
        })
    };
    let home = summarize_location(agent.home)?;
    let workplace = agent.workplace.map(summarize_location).transpose()?;
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
                serves_food: destination.serves_food,
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
                relationship: agent
                    .relationships
                    .get(id)
                    .copied()
                    .unwrap_or(Relationship::NEUTRAL),
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
            home,
            workplace,
            personality: agent.personality.clone(),
            needs: agent.needs.clone(),
        },
        current_location: LocationDescription {
            id: location.id,
            name: location.name.clone(),
            serves_food: location.serves_food,
            connected,
        },
        visible_agents,
        goals: agent
            .goals
            .iter()
            .map(|goal| GoalStatus {
                description: goal.description.clone(),
                kind: goal.kind,
                progress: goal.progress,
            })
            .collect(),
        relevant_memories: agent
            .memories
            .iter()
            .map(|memory| describe_memory(world, observer, memory))
            .collect(),
        beliefs: Vec::new(),
    })
}

fn describe_memory(world: &World, observer: AgentId, event: &Event) -> String {
    let agent_name = |id: AgentId| {
        world
            .agents
            .get(&id)
            .map_or_else(|| id.to_string(), |agent| agent.name.clone())
    };
    let location_name = |id: LocationId| {
        world
            .locations
            .get(&id)
            .map_or_else(|| id.to_string(), |location| location.name.clone())
    };
    let description = match &event.kind {
        EventKind::Moved { agent, from, to } if *agent == observer => format!(
            "You moved from {} to {}.",
            location_name(*from),
            location_name(*to)
        ),
        EventKind::Moved { agent, from, to } => format!(
            "{} moved from {} to {}.",
            agent_name(*agent),
            location_name(*from),
            location_name(*to)
        ),
        EventKind::Spoke {
            speaker,
            listener,
            message,
        } if *speaker == observer => {
            format!("You said to {}: {message:?}", agent_name(*listener))
        }
        EventKind::Spoke {
            speaker,
            listener,
            message,
        } if *listener == observer => {
            format!("{} said to you: {message:?}", agent_name(*speaker))
        }
        EventKind::Spoke {
            speaker,
            listener,
            message,
        } => format!(
            "{} said to {}: {message:?}",
            agent_name(*speaker),
            agent_name(*listener)
        ),
        EventKind::Observed {
            observer: actor,
            target,
        } => {
            let subject = if *actor == observer {
                "You".into()
            } else {
                agent_name(*actor)
            };
            let target = match target {
                ObservationTarget::Agent(agent) if *agent == observer => "you".into(),
                ObservationTarget::Agent(agent) => agent_name(*agent),
                ObservationTarget::Location(location) => location_name(*location),
            };
            format!("{subject} observed {target}.")
        }
        EventKind::Ate { agent } if *agent == observer => "You ate.".into(),
        EventKind::Ate { agent } => format!("{} ate.", agent_name(*agent)),
        EventKind::Rested { agent } if *agent == observer => "You rested.".into(),
        EventKind::Rested { agent } => format!("{} rested.", agent_name(*agent)),
        EventKind::Worked { agent } if *agent == observer => "You worked.".into(),
        EventKind::Worked { agent } => format!("{} worked.", agent_name(*agent)),
        EventKind::GoalCompleted { agent, goal } if *agent == observer => {
            format!("You completed your goal: {goal}.")
        }
        EventKind::GoalCompleted { agent, goal } => {
            format!("{} completed their goal: {goal}.", agent_name(*agent))
        }
        EventKind::Waited { agent } if *agent == observer => "You waited.".into(),
        EventKind::Waited { agent } => format!("{} waited.", agent_name(*agent)),
        EventKind::ActionRejected { agent, .. } if *agent == observer => {
            "Your attempted action was rejected.".into()
        }
        EventKind::ActionRejected { agent, .. } => {
            format!("{} had an action rejected.", agent_name(*agent))
        }
    };
    format!("{}: {description}", event.tick)
}

#[cfg(test)]
mod tests {
    use super::{ObservationError, perceive};
    use crate::sim::{
        ActionResult, AgentId, GoalKind, ObservationTarget, ProposedAction, Relationship, World,
    };
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

        let current = world.agents[&observer].location;
        world.execute(
            observer,
            ProposedAction::Observe {
                target: ObservationTarget::Location(current),
            },
        );
        let observation = perceive(&world, observer).expect("observation");
        assert_eq!(observation.goals.len(), 4);
        assert_eq!(
            observation
                .goals
                .iter()
                .find(|goal| goal.kind == GoalKind::Exploration)
                .expect("exploration goal")
                .progress,
            0.25
        );
        assert_eq!(
            observation.current_location.serves_food,
            world.locations[&observation.current_location.id].serves_food
        );
        assert!(
            observation
                .current_location
                .connected
                .iter()
                .all(|location| {
                    location.serves_food == world.locations[&location.id].serves_food
                })
        );
        assert!(
            observation
                .visible_agents
                .iter()
                .all(|agent| agent.id != hidden)
        );
        assert_eq!(observation.visible_agents.len(), 6);
        assert!(observation.relevant_memories[0].contains("moved from"));
        assert!(observation.beliefs.is_empty());
    }

    #[test]
    fn visible_relationships_are_observer_relative() {
        let mut world = World::briar_glen(12).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let observer = residents[0];
        let target = residents[2];
        let stranger = residents[3];
        world
            .agents
            .get_mut(&observer)
            .expect("observer")
            .relationships
            .insert(
                target,
                Relationship {
                    affection: 0.8,
                    ..Relationship::NEUTRAL
                },
            );
        world
            .agents
            .get_mut(&target)
            .expect("target")
            .relationships
            .insert(
                observer,
                Relationship {
                    affection: -0.8,
                    ..Relationship::NEUTRAL
                },
            );

        let observation = perceive(&world, observer).expect("observation");
        assert_eq!(
            observation
                .visible_agents
                .iter()
                .find(|agent| agent.id == target)
                .expect("target")
                .relationship
                .affection,
            0.8
        );
        assert_eq!(
            observation
                .visible_agents
                .iter()
                .find(|agent| agent.id == stranger)
                .expect("stranger")
                .relationship,
            Relationship::NEUTRAL
        );
    }

    #[test]
    fn memories_are_relative_and_do_not_include_unseen_events() {
        let mut world = World::briar_glen(11).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let hidden = residents[0];
        let speaker = residents[1];
        let listener = residents[2];
        let home = world.agents[&hidden].location;
        let destination = *world.locations[&home]
            .connected
            .iter()
            .next()
            .expect("destination");
        world.execute(hidden, ProposedAction::Move { destination });
        world
            .agents
            .get_mut(&hidden)
            .expect("hidden")
            .memories
            .clear();
        world.execute(
            speaker,
            ProposedAction::Talk {
                target: listener,
                message: "The lantern is lit.".into(),
            },
        );

        let speaker_memory = &perceive(&world, speaker)
            .expect("speaker")
            .relevant_memories[1];
        let listener_memory = &perceive(&world, listener)
            .expect("listener")
            .relevant_memories[1];
        assert!(speaker_memory.contains("You said to"));
        assert!(listener_memory.contains("said to you"));
        assert!(
            perceive(&world, hidden)
                .expect("hidden")
                .relevant_memories
                .is_empty()
        );
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
