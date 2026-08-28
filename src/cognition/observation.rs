use crate::sim::{
    Activity, AgentId, Belief, Event, EventId, EventKind, Goal, LocationId, Needs,
    ObservationTarget, Occupation, OpeningHours, Personality, Relationship, Tick, World,
    event_evidence,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub tick: Tick,
    pub local_time: LocalTime,
    pub self_description: SelfDescription,
    pub current_location: LocationDescription,
    pub visible_agents: Vec<VisibleAgent>,
    pub action_affordances: ActionAffordances,
    pub route_hints: RouteHints,
    pub goals: Vec<Goal>,
    pub relevant_memories: Vec<String>,
    pub rumors: Vec<RumorSummary>,
    pub beliefs: BTreeMap<AgentId, Belief>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RumorSummary {
    pub claim: EventId,
    pub subject: Option<AgentId>,
    pub report: String,
    pub source: String,
    pub depth: u8,
    pub confidence: f32,
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfrontationAffordance {
    pub target: AgentId,
    pub claim: EventId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionAffordances {
    pub move_to: Vec<LocationId>,
    pub talk_to: Vec<AgentId>,
    pub confront: Vec<ConfrontationAffordance>,
    pub can_eat: bool,
    pub can_rest: bool,
    pub can_work: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteHints {
    pub home: Option<LocationId>,
    pub workplace: Option<LocationId>,
    pub food: Option<LocationId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTime {
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
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
    pub activity: Option<Activity>,
    pub mood: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationDescription {
    pub id: LocationId,
    pub name: String,
    pub serves_food: bool,
    pub opening_hours: Option<OpeningHours>,
    pub is_open: bool,
    pub connected: Vec<LocationSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationSummary {
    pub id: LocationId,
    pub name: String,
    pub serves_food: bool,
    pub opening_hours: Option<OpeningHours>,
    pub is_open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleAgent {
    pub id: AgentId,
    pub name: String,
    pub occupation: Occupation,
    pub activity: Option<Activity>,
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
            opening_hours: location.opening_hours,
            is_open: location.is_open(world.tick.hour()),
        })
    };
    let home = summarize_location(agent.home)?;
    let workplace = agent.workplace.map(summarize_location).transpose()?;
    let connected = location
        .connected
        .iter()
        .map(|id| summarize_location(*id))
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
                activity: visible.activity,
                relationship: agent
                    .relationships
                    .get(id)
                    .copied()
                    .unwrap_or(Relationship::NEUTRAL),
            })
        })
        .collect::<Result<Vec<_>, ObservationError>>()?;

    let action_affordances = ActionAffordances {
        move_to: connected
            .iter()
            .filter(|location| location.is_open)
            .map(|location| location.id)
            .collect(),
        talk_to: visible_agents
            .iter()
            .filter(|agent| agent.activity.is_none())
            .map(|agent| agent.id)
            .collect(),
        confront: agent
            .rumors
            .iter()
            .filter(|rumor| !rumor.resolved && rumor.confidence >= 0.4)
            .filter_map(|rumor| {
                let target = rumor_subject(&rumor.event.kind)?;
                visible_agents
                    .iter()
                    .any(|agent| agent.id == target && agent.activity.is_none())
                    .then_some(ConfrontationAffordance {
                        target,
                        claim: rumor.event.id,
                    })
            })
            .collect(),
        can_eat: location.id == agent.home
            || location.serves_food && location.is_open(world.tick.hour()),
        can_rest: location.id == agent.home,
        can_work: agent.workplace == Some(location.id) && location.is_open(world.tick.hour()),
    };
    let route_hints = RouteHints {
        home: next_hop(world, location.id, BTreeSet::from([agent.home])),
        workplace: agent
            .workplace
            .and_then(|workplace| next_hop(world, location.id, BTreeSet::from([workplace]))),
        food: next_hop(
            world,
            location.id,
            world
                .locations
                .values()
                .filter(|candidate| {
                    candidate.id == agent.home
                        || candidate.serves_food && candidate.is_open(world.tick.hour())
                })
                .map(|candidate| candidate.id)
                .collect(),
        ),
    };

    Ok(AgentObservation {
        tick: world.tick,
        local_time: LocalTime {
            day: world.tick.day(),
            hour: world.tick.hour(),
            minute: world.tick.minute(),
        },
        self_description: SelfDescription {
            id: agent.id,
            name: agent.name.clone(),
            age: agent.age,
            occupation: agent.occupation.clone(),
            home,
            workplace,
            personality: agent.personality.clone(),
            needs: agent.needs.clone(),
            activity: agent.activity,
            mood: agent.mood,
        },
        current_location: LocationDescription {
            id: location.id,
            name: location.name.clone(),
            serves_food: location.serves_food,
            opening_hours: location.opening_hours,
            is_open: location.is_open(world.tick.hour()),
            connected,
        },
        visible_agents,
        action_affordances,
        route_hints,
        goals: agent.goals.clone(),
        relevant_memories: agent
            .memories
            .iter()
            .map(|memory| describe_memory(world, observer, memory))
            .collect(),
        rumors: agent
            .rumors
            .iter()
            .map(|rumor| RumorSummary {
                claim: rumor.event.id,
                subject: rumor_subject(&rumor.event.kind),
                report: describe_memory(world, observer, &rumor.event),
                source: world.agents[&rumor.source].name.clone(),
                depth: rumor.depth,
                confidence: rumor.confidence,
                resolved: rumor.resolved,
            })
            .collect(),
        beliefs: agent.beliefs.clone(),
    })
}

fn rumor_subject(kind: &EventKind) -> Option<AgentId> {
    event_evidence(kind).map(|(subject, ..)| subject)
}

fn next_hop(world: &World, start: LocationId, targets: BTreeSet<LocationId>) -> Option<LocationId> {
    if targets.contains(&start) {
        return None;
    }

    let mut visited = BTreeSet::from([start]);
    let mut queue = VecDeque::new();
    for neighbor in &world.locations.get(&start)?.connected {
        if world.locations.get(neighbor)?.is_open(world.tick.hour()) {
            visited.insert(*neighbor);
            queue.push_back((*neighbor, *neighbor));
        }
    }

    while let Some((location, first_hop)) = queue.pop_front() {
        if targets.contains(&location) {
            return Some(first_hop);
        }
        for neighbor in &world.locations.get(&location)?.connected {
            if !visited.contains(neighbor)
                && world.locations.get(neighbor)?.is_open(world.tick.hour())
            {
                visited.insert(*neighbor);
                queue.push_back((*neighbor, first_hop));
            }
        }
    }
    None
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
            tone,
            message,
        } if *speaker == observer => {
            format!(
                "You said to {} [{tone}]: {message:?}",
                agent_name(*listener)
            )
        }
        EventKind::Spoke {
            speaker,
            listener,
            tone,
            message,
        } if *listener == observer => {
            format!("{} said to you [{tone}]: {message:?}", agent_name(*speaker))
        }
        EventKind::Spoke {
            speaker,
            listener,
            tone,
            message,
        } => format!(
            "{} said to {} [{tone}]: {message:?}",
            agent_name(*speaker),
            agent_name(*listener)
        ),
        EventKind::Confronted {
            accuser,
            target,
            claim,
            outcome,
        } => {
            let accuser = if *accuser == observer {
                "You".into()
            } else {
                agent_name(*accuser)
            };
            let target = if *target == observer {
                "you".into()
            } else {
                agent_name(*target)
            };
            let claim = world
                .events()
                .iter()
                .find(|event| event.id == *claim)
                .map_or_else(
                    || claim.to_string(),
                    |event| describe_memory(world, observer, event),
                );
            format!("{accuser} confronted {target} about {claim} The claim was {outcome}.")
        }
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
    use super::{ObservationError, next_hop, perceive};
    use crate::sim::{
        ActionResult, Activity, ActivityKind, AgentId, Belief, DialogueTone, EventKind, GoalKind,
        ObservationTarget, OpeningHours, ProposedAction, Relationship, Rumor, Tick, World,
    };
    use std::collections::BTreeSet;
    use uuid::Uuid;

    #[test]
    fn observation_contains_only_local_agents() {
        let mut world = World::briar_glen(9).expect("town");
        let hidden = *world.agents.keys().next().expect("resident");
        let from = world.agents[&hidden].location;
        let destination = *world.locations[&from]
            .connected
            .iter()
            .find(|id| world.locations[id].is_open(world.tick.hour()))
            .expect("open connected location");
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
        assert_eq!(observation.local_time.day, 1);
        assert_eq!(observation.local_time.hour, 7);
        assert_eq!(observation.local_time.minute, 0);
        let work_hours = observation
            .self_description
            .workplace
            .expect("workplace")
            .opening_hours
            .expect("work hours");
        assert_eq!(work_hours.opens_at_hour, 8);
        assert_eq!(work_hours.closes_at_hour, 18);
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
                    let source = &world.locations[&location.id];
                    location.serves_food == source.serves_food
                        && location.opening_hours == source.opening_hours
                        && location.is_open == source.is_open(world.tick.hour())
                })
        );
        assert!(
            observation
                .visible_agents
                .iter()
                .all(|agent| agent.id != hidden)
        );
        assert_eq!(
            observation.self_description.mood,
            world.agents[&observer].mood
        );
        assert_eq!(observation.visible_agents.len(), 6);
        assert_eq!(
            observation.action_affordances.move_to,
            observation
                .current_location
                .connected
                .iter()
                .filter(|location| location.is_open)
                .map(|location| location.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            observation.action_affordances.talk_to,
            observation
                .visible_agents
                .iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            observation.action_affordances.can_eat,
            observation.current_location.id == observation.self_description.home.id
                || observation.current_location.serves_food && observation.current_location.is_open
        );
        assert_eq!(
            observation.action_affordances.can_rest,
            observation.current_location.id == observation.self_description.home.id
        );
        assert!(!observation.action_affordances.can_work);
        assert!(
            [
                observation.route_hints.home,
                observation.route_hints.workplace,
                observation.route_hints.food,
            ]
            .into_iter()
            .flatten()
            .all(|next_hop| observation.action_affordances.move_to.contains(&next_hop))
        );
        assert!(
            serde_json::to_value(&observation.visible_agents[0])
                .expect("visible agent JSON")
                .get("mood")
                .is_none()
        );
        assert!(observation.relevant_memories[0].contains("moved from"));
        assert!(observation.beliefs.is_empty());
    }

    #[test]
    fn observations_show_current_activities_and_exclude_busy_talk_targets() {
        let mut world = World::briar_glen(12).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let observer = residents[0];
        let visible = residents[1];
        let activity = Activity {
            kind: ActivityKind::Working,
            until: Tick(world.tick.0 + 12),
        };
        world.agents.get_mut(&observer).expect("observer").activity = Some(Activity {
            kind: ActivityKind::Waiting,
            until: Tick(world.tick.0 + 1),
        });
        world.agents.get_mut(&visible).expect("visible").activity = Some(activity);

        let observation = perceive(&world, observer).expect("observation");
        assert_eq!(
            observation.self_description.activity.map(|a| a.kind),
            Some(ActivityKind::Waiting)
        );
        assert_eq!(
            observation
                .visible_agents
                .iter()
                .find(|agent| agent.id == visible)
                .and_then(|agent| agent.activity),
            Some(activity)
        );
        assert!(!observation.action_affordances.talk_to.contains(&visible));
    }

    #[test]
    fn routes_are_deterministic_and_use_only_open_locations() {
        let mut world = World::briar_glen(12).expect("town");
        world.tick = Tick(12 * 12);
        let location = |name: &str| {
            world
                .locations
                .values()
                .find(|location| location.name == name)
                .map(|location| location.id)
                .expect("location")
        };
        let mill = location("Abandoned Mill");
        let houses = location("Riverside Houses");
        let store = location("General Store");
        let target = BTreeSet::from([store]);

        world.locations.get_mut(&mill).expect("mill").opening_hours = Some(OpeningHours {
            opens_at_hour: 0,
            closes_at_hour: 1,
        });
        assert_eq!(next_hop(&world, mill, target.clone()), Some(houses));

        for location in world.locations.values_mut() {
            if location.id != mill && location.id != store {
                location.opening_hours = Some(OpeningHours {
                    opens_at_hour: 0,
                    closes_at_hour: 1,
                });
            }
        }
        assert_eq!(next_hop(&world, mill, target), None);
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
        world
            .agents
            .get_mut(&observer)
            .expect("observer")
            .beliefs
            .insert(
                target,
                Belief {
                    sociability: 0.9,
                    confidence: 0.8,
                    ..Belief::default()
                },
            );
        world
            .agents
            .get_mut(&target)
            .expect("target")
            .beliefs
            .insert(
                observer,
                Belief {
                    hostility: 0.9,
                    confidence: 0.8,
                    ..Belief::default()
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
        assert_eq!(observation.beliefs[&target].sociability, 0.9);
        assert_eq!(observation.beliefs[&target].hostility, 0.0);
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
            .find(|id| world.locations[id].is_open(world.tick.hour()))
            .expect("open destination");
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
                tone: DialogueTone::Friendly,
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
    fn rumors_are_observer_specific_and_name_the_source() {
        let mut world = World::briar_glen(12).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let observer = residents[0];
        let source = residents[1];
        let outsider = residents[2];
        let mut event = match world.execute(source, ProposedAction::Wait) {
            ActionResult::Success(events) => events[0].clone(),
            ActionResult::Rejected(reason) => panic!("wait rejected: {reason:?}"),
        };
        event.kind = EventKind::Worked { agent: source };
        world.agents.get_mut(&source).expect("source").activity = None;
        let claim = event.id;
        world.agents.get_mut(&observer).expect("observer").rumors = vec![Rumor {
            event,
            source,
            depth: 2,
            confidence: 0.6,
            resolved: false,
        }];

        let observation = perceive(&world, observer).expect("observer");
        let rumor = &observation.rumors[0];
        assert!(rumor.report.contains(&world.agents[&source].name));
        assert_eq!(rumor.source, world.agents[&source].name);
        assert_eq!(rumor.subject, Some(source));
        assert_eq!(rumor.depth, 2);
        assert_eq!(rumor.confidence, 0.6);
        assert_eq!(
            observation.action_affordances.confront,
            vec![super::ConfrontationAffordance {
                target: source,
                claim,
            }]
        );
        assert!(
            perceive(&world, outsider)
                .expect("outsider")
                .rumors
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
