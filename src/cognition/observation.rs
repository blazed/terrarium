use crate::sim::{
    Activity, AgentId, Belief, Business, Event, EventId, EventKind, Goal, HealthCondition,
    Intention, Inventory, LifeState, LocationId, Needs, ObservationTarget, Occupation, Offering,
    OpeningHours, Personality, Relationship, RoutingStats, Tick, TownEventKind, World,
    event_evidence,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub tick: Tick,
    pub local_time: LocalTime,
    pub town_event: Option<TownEventObservation>,
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
pub struct TownEventObservation {
    pub kind: TownEventKind,
    pub remaining_ticks: u64,
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
    pub can_purchase: bool,
    pub can_consume_meal: bool,
    pub can_use_supplies: bool,
    pub can_use_repair_kit: bool,
    pub can_use_medicine: bool,
    pub can_seek_treatment: bool,
    pub can_rest: bool,
    pub can_work: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteHint {
    pub destination: LocationId,
    pub next_hop: LocationId,
    pub distance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteHints {
    pub home: Option<RouteHint>,
    pub workplace: Option<RouteHint>,
    pub purchase: Option<RouteHint>,
    pub treatment: Option<RouteHint>,
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
    pub balance: u64,
    pub inventory: Inventory,
    #[serde(skip)]
    pub routing: RoutingStats,
    pub activity: Option<Activity>,
    pub intention: Option<Intention>,
    pub mood: f32,
    pub health: f32,
    pub injury: bool,
    pub conditions: Vec<HealthCondition>,
    pub life: LifeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationDescription {
    pub id: LocationId,
    pub name: String,
    pub business: Option<Business>,
    pub opening_hours: Option<OpeningHours>,
    pub is_open: bool,
    pub connected: Vec<LocationSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationSummary {
    pub id: LocationId,
    pub name: String,
    pub business: Option<Business>,
    pub opening_hours: Option<OpeningHours>,
    pub is_open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleAgent {
    pub id: AgentId,
    pub name: String,
    pub occupation: Occupation,
    pub activity: Option<Activity>,
    pub conditions: Vec<HealthCondition>,
    pub relationship: Relationship,
    pub last_conversation: Option<Tick>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObservationError {
    #[error("unknown observer {0}")]
    UnknownAgent(AgentId),
    #[error("observer is at unknown location {0}")]
    UnknownLocation(LocationId),
    #[error("observer {0} is dead")]
    AgentDead(AgentId),
    #[error("location contains unknown agent {0}")]
    InvalidVisibleAgent(AgentId),
}

pub fn perceive(world: &World, observer: AgentId) -> Result<AgentObservation, ObservationError> {
    let agent = world
        .agents
        .get(&observer)
        .ok_or(ObservationError::UnknownAgent(observer))?;
    if !agent.is_alive() {
        return Err(ObservationError::AgentDead(observer));
    }
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
            business: location.business,
            opening_hours: location.opening_hours,
            is_open: world.is_location_open(location.id),
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
                conditions: visible.health_conditions(),
                relationship: agent
                    .relationships
                    .get(id)
                    .copied()
                    .unwrap_or(Relationship::NEUTRAL),
                last_conversation: agent.memories.iter().rev().find_map(|event| {
                    matches!(
                        &event.kind,
                        EventKind::Spoke {
                            speaker,
                            listener,
                            ..
                        } if (*speaker == observer && *listener == *id)
                            || (*speaker == *id && *listener == observer)
                    )
                    .then_some(event.tick)
                }),
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
        can_purchase: location.business.is_some_and(|business| {
            world.is_location_open(location.id)
                && business.stock > 0
                && agent.balance >= business.price
                && business
                    .offering
                    .item()
                    .is_none_or(|item| agent.inventory.has_capacity(item))
        }),
        can_consume_meal: agent.inventory.meals > 0,
        can_use_supplies: agent.inventory.supplies > 0,
        can_use_repair_kit: agent.inventory.repair_kits > 0,
        can_use_medicine: agent.inventory.medicine > 0
            && (agent.injury || agent.disease.is_symptomatic()),
        can_seek_treatment: location.business.is_some_and(|business| {
            business.offering == Offering::Medicine
                && world.is_location_open(location.id)
                && business.stock > 0
                && agent.balance >= business.price
                && (agent.injury || agent.disease.is_symptomatic())
        }),
        can_rest: location.id == agent.home,
        can_work: agent.workplace == Some(location.id)
            && agent.health >= 0.2
            && !agent.injury
            && world.is_location_open(location.id)
            && location.business.is_some_and(Business::solvent),
    };
    let shortage = world
        .active_town_event
        .is_some_and(|event| event.kind == TownEventKind::Shortage);
    let desired_offering =
        Offering::desired(&agent.needs).or_else(|| agent.inventory.reserve_offering(shortage));
    let route_hints = RouteHints {
        home: next_hop(world, location.id, BTreeSet::from([agent.home])),
        workplace: agent.workplace.and_then(|workplace| {
            world.locations[&workplace]
                .business
                .filter(|business| business.solvent())
                .and_then(|_| next_hop(world, location.id, BTreeSet::from([workplace])))
        }),
        purchase: next_hop(
            world,
            location.id,
            world
                .locations
                .values()
                .filter(|candidate| {
                    candidate.business.is_some_and(|business| {
                        desired_offering.is_none_or(|offering| business.offering == offering)
                            && world.is_location_open(candidate.id)
                            && business.stock > 0
                            && agent.balance >= business.price
                            && business
                                .offering
                                .item()
                                .is_none_or(|item| agent.inventory.has_capacity(item))
                    })
                })
                .map(|candidate| candidate.id)
                .collect(),
        ),
        treatment: world.clinic_location().and_then(|clinic| {
            let business = world.locations[&clinic].business?;
            ((agent.injury || agent.disease.is_symptomatic())
                && world.is_location_open(clinic)
                && business.stock > 0
                && agent.balance >= business.price)
                .then(|| next_hop(world, location.id, BTreeSet::from([clinic])))
                .flatten()
        }),
    };

    Ok(AgentObservation {
        tick: world.tick,
        local_time: LocalTime {
            day: world.tick.day(),
            hour: world.tick.hour(),
            minute: world.tick.minute(),
        },
        town_event: world.active_town_event.map(|event| TownEventObservation {
            kind: event.kind,
            remaining_ticks: event.ends_at.0 - world.tick.0,
        }),
        self_description: SelfDescription {
            id: agent.id,
            name: agent.name.clone(),
            age: agent.age,
            occupation: agent.occupation.clone(),
            home,
            workplace,
            personality: agent.personality.clone(),
            needs: agent.needs.clone(),
            balance: agent.balance,
            inventory: agent.inventory,
            routing: agent.routing,
            activity: agent.activity,
            intention: agent.intention.clone(),
            mood: agent.mood,
            health: agent.health,
            injury: agent.injury,
            conditions: agent.health_conditions(),
            life: agent.life,
        },
        current_location: LocationDescription {
            id: location.id,
            name: location.name.clone(),
            business: location.business,
            opening_hours: location.opening_hours,
            is_open: world.is_location_open(location.id),
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

fn next_hop(world: &World, start: LocationId, targets: BTreeSet<LocationId>) -> Option<RouteHint> {
    world
        .shortest_open_route(start, &targets)
        .map(|(destination, next_hop, distance)| RouteHint {
            destination,
            next_hop,
            distance,
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
        EventKind::TownEventStarted { kind, ends_at } => {
            format!("The {kind} began and will last until {ends_at}.")
        }
        EventKind::TownEventEnded { kind } => format!("The {kind} ended."),
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
        EventKind::Purchased {
            agent,
            offering,
            cost,
        } if *agent == observer => format!("You bought {offering} for {cost} coins."),
        EventKind::Purchased {
            agent,
            offering,
            cost,
        } => format!("{} bought {offering} for {cost} coins.", agent_name(*agent)),
        EventKind::ItemUsed { agent, item } if *agent == observer => {
            format!("You used a {item}.")
        }
        EventKind::ItemUsed { agent, item } => {
            format!("{} used a {item}.", agent_name(*agent))
        }
        EventKind::Treated { agent, cost } if *agent == observer => {
            format!("You received treatment for {cost} coins.")
        }
        EventKind::Treated { agent, cost } => {
            format!(
                "{} received treatment for {cost} coins.",
                agent_name(*agent)
            )
        }
        EventKind::Rested { agent } if *agent == observer => "You rested.".into(),
        EventKind::Rested { agent } => format!("{} rested.", agent_name(*agent)),
        EventKind::Worked {
            agent,
            wage,
            stock_produced,
        } if *agent == observer => format!(
            "You worked and earned {wage} coins{}.",
            produced_stock(*stock_produced)
        ),
        EventKind::Worked {
            agent,
            wage,
            stock_produced,
        } => format!(
            "{} worked and earned {wage} coins{}.",
            agent_name(*agent),
            produced_stock(*stock_produced)
        ),
        EventKind::GoalCompleted { agent, goal } if *agent == observer => {
            format!("You completed your goal: {goal}.")
        }
        EventKind::GoalCompleted { agent, goal } => {
            format!("{} completed their goal: {goal}.", agent_name(*agent))
        }
        EventKind::Waited { agent } if *agent == observer => "You waited.".into(),
        EventKind::Waited { agent } => format!("{} waited.", agent_name(*agent)),
        EventKind::Died { agent, cause } => {
            format!("{} died from {cause}.", agent_name(*agent))
        }
        EventKind::DiseaseInfected { .. } => "Someone became infected.".into(),
        EventKind::DiseaseSymptoms { agent } if *agent == observer => {
            "You developed Briar fever symptoms.".into()
        }
        EventKind::DiseaseSymptoms { agent } => {
            format!("{} developed Briar fever symptoms.", agent_name(*agent))
        }
        EventKind::DiseaseRecovered { agent } if *agent == observer => {
            "You started recovering from Briar fever.".into()
        }
        EventKind::DiseaseRecovered { agent } => {
            format!(
                "{} started recovering from Briar fever.",
                agent_name(*agent)
            )
        }
        EventKind::DiseaseImmunityExpired { agent } if *agent == observer => {
            "Your Briar fever immunity expired.".into()
        }
        EventKind::DiseaseImmunityExpired { agent } => {
            format!("{}'s Briar fever immunity expired.", agent_name(*agent))
        }
        EventKind::ActionRejected { agent, .. } if *agent == observer => {
            "Your attempted action was rejected.".into()
        }
        EventKind::ActionRejected { agent, .. } => {
            format!("{} had an action rejected.", agent_name(*agent))
        }
    };
    format!("{}: {description}", event.tick)
}

fn produced_stock(amount: u32) -> String {
    if amount == 0 {
        String::new()
    } else {
        format!(" and produced {amount} stock")
    }
}

#[cfg(test)]
mod tests {
    use super::{ObservationError, next_hop, perceive};
    use crate::sim::{
        ActionResult, Activity, ActivityKind, AgentId, Belief, DialogueTone, DiseaseState,
        EventKind, HealthCondition, Intention, IntentionGoal, ObservationTarget, OpeningHours,
        ProposedAction, Relationship, Rumor, Tick, TownEventKind, World,
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
        assert!((1..=3).contains(&observation.goals.len()));
        assert!(
            observation
                .goals
                .iter()
                .all(|goal| goal.progress < goal.required)
        );
        assert!(
            observation
                .current_location
                .connected
                .iter()
                .all(|location| {
                    let source = &world.locations[&location.id];
                    location.business == source.business
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
        assert!(!observation.action_affordances.can_purchase);
        assert_eq!(
            observation.action_affordances.can_rest,
            observation.current_location.id == observation.self_description.home.id
        );
        assert!(!observation.action_affordances.can_work);
        assert!(
            [
                observation.route_hints.home,
                observation.route_hints.workplace,
                observation.route_hints.purchase,
            ]
            .into_iter()
            .flatten()
            .all(|hint| observation
                .action_affordances
                .move_to
                .contains(&hint.next_hop))
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
        world.agents.get_mut(&observer).expect("observer").intention = Some(Intention {
            goal: IntentionGoal::Rest,
            expires_at: Tick(world.tick.0 + 10),
        });
        world.agents.get_mut(&visible).expect("visible").intention = Some(Intention {
            goal: IntentionGoal::Work,
            expires_at: Tick(world.tick.0 + 10),
        });

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
        assert_eq!(
            observation
                .self_description
                .intention
                .expect("own intention")
                .goal,
            IntentionGoal::Rest
        );
        assert!(
            serde_json::to_value(&observation.visible_agents[0])
                .expect("visible agent")
                .get("intention")
                .is_none()
        );
    }

    #[test]
    fn visible_agents_show_only_direct_conversation_recency() {
        let mut world = World::briar_glen(13).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[1];
        let witness = residents[2];
        assert!(matches!(
            world.execute(
                speaker,
                ProposedAction::Talk {
                    target: listener,
                    tone: DialogueTone::Friendly,
                    message: "Morning.".into(),
                },
            ),
            ActionResult::Success(_)
        ));

        let speaker_view = perceive(&world, speaker).expect("speaker observation");
        assert_eq!(
            speaker_view
                .visible_agents
                .iter()
                .find(|agent| agent.id == listener)
                .expect("listener")
                .last_conversation,
            Some(world.tick)
        );
        let witness_view = perceive(&world, witness).expect("witness observation");
        assert_eq!(
            witness_view
                .visible_agents
                .iter()
                .find(|agent| agent.id == speaker)
                .expect("speaker")
                .last_conversation,
            None
        );
    }

    #[test]
    fn observation_exposes_only_the_actors_inventory_and_item_affordances() {
        let mut world = World::briar_glen(19).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let inventory = &mut world.agents.get_mut(&actor).expect("resident").inventory;
        inventory.meals = 1;
        inventory.repair_kits = 1;
        let expected = *inventory;

        let observation = perceive(&world, actor).expect("observation");
        assert_eq!(observation.self_description.inventory, expected);
        assert!(observation.action_affordances.can_consume_meal);
        assert!(!observation.action_affordances.can_use_supplies);
        assert!(observation.action_affordances.can_use_repair_kit);
        assert!(observation.visible_agents.iter().all(|agent| {
            serde_json::to_value(agent)
                .expect("visible agent")
                .get("inventory")
                .is_none()
        }));
    }

    #[test]
    fn marketplace_visibility_and_routes_respect_balance_and_stock() {
        let mut world = World::briar_glen(19).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let business = world
            .locations
            .values()
            .find(|location| location.business.is_some() && location.is_open(world.tick.hour()))
            .expect("open business")
            .id;
        world.relocate(actor, business);

        let observation = perceive(&world, actor).expect("observation");
        assert_eq!(observation.self_description.balance, 20);
        assert_eq!(
            observation
                .current_location
                .business
                .map(|business| business.price),
            Some(5)
        );
        assert_eq!(
            observation.current_location.business,
            world.locations[&business].business
        );
        let ledger = observation
            .current_location
            .business
            .expect("visible ledger");
        assert_eq!(ledger.cash, crate::sim::BUSINESS_STARTING_CASH);
        assert_eq!(ledger.wages_paid, 0);
        assert!(observation.action_affordances.can_purchase);

        let home = world.agents[&actor].home;
        world.relocate(actor, home);
        for location in world.locations.values_mut() {
            if let Some(ledger) = location.business.as_mut() {
                ledger.stock = 0;
            }
        }
        assert_eq!(
            perceive(&world, actor)
                .expect("sold out")
                .route_hints
                .purchase,
            None
        );
        world
            .locations
            .get_mut(&business)
            .expect("business")
            .business
            .as_mut()
            .expect("ledger")
            .stock = 1;
        assert!(
            perceive(&world, actor)
                .expect("stocked")
                .route_hints
                .purchase
                .is_some()
        );

        world.agents.get_mut(&actor).expect("resident").balance = 0;
        assert_eq!(
            perceive(&world, actor).expect("broke").route_hints.purchase,
            None
        );

        world.agents.get_mut(&actor).expect("resident").balance = 20;
        world
            .locations
            .get_mut(&business)
            .expect("business")
            .business
            .as_mut()
            .expect("ledger")
            .cash = 0;
        assert_eq!(
            perceive(&world, actor)
                .expect("insolvent")
                .route_hints
                .workplace,
            None
        );
        world.relocate(actor, business);
        assert!(
            !perceive(&world, actor)
                .expect("insolvent workplace")
                .action_affordances
                .can_work
        );
    }

    #[test]
    fn marketplace_routes_follow_the_need_each_offering_satisfies() {
        let mut world = World::briar_glen(23).expect("town");
        world.advance_to(Tick(8 * 12)).expect("business hours");
        let actor = *world.agents.keys().next().expect("resident");
        let home = world.agents[&actor].home;
        world.relocate(actor, home);
        world.agents.get_mut(&actor).expect("resident").balance = 100;

        for (offering, food, safety, status) in [
            (crate::sim::Offering::Meal, 0.1, 1.0, 1.0),
            (crate::sim::Offering::Repairs, 1.0, 0.1, 1.0),
            (crate::sim::Offering::Supplies, 1.0, 0.3, 1.0),
            (crate::sim::Offering::CivicServices, 1.0, 1.0, 0.1),
        ] {
            let needs = &mut world.agents.get_mut(&actor).expect("resident").needs;
            needs.food = food;
            needs.safety = safety;
            needs.status = status;
            let route = perceive(&world, actor)
                .expect("marketplace route")
                .route_hints
                .purchase
                .expect("matching business");
            assert!(route.distance > 0);
            assert_eq!(
                world.locations[&route.destination]
                    .business
                    .expect("business")
                    .offering,
                offering
            );
        }
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
        assert_eq!(
            next_hop(&world, mill, target.clone()),
            Some(super::RouteHint {
                destination: store,
                next_hop: houses,
                distance: 2,
            })
        );

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
        event.kind = EventKind::Worked {
            agent: source,
            wage: crate::sim::WORK_WAGE,
            stock_produced: 0,
        };
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
    fn observations_expose_active_town_events_and_storm_closures() {
        let mut world = World::briar_glen(0).expect("town");
        world
            .advance_to(Tick(8 * 60 / Tick::MINUTES))
            .expect("storm");
        let observer = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, observer).expect("observation");

        let event = observation.town_event.expect("town event");
        assert_eq!(event.kind, TownEventKind::Storm);
        assert_eq!(event.remaining_ticks, 72);
        assert!(
            observation
                .current_location
                .connected
                .iter()
                .all(|location| location.is_open == world.is_location_open(location.id))
        );
    }

    #[test]
    fn observations_expose_only_visible_health_conditions() {
        let mut world = World::briar_glen(13).expect("town");
        let observer = *world.agents.keys().next().expect("resident");
        let location = world.agents[&observer].location;
        let visible = world.locations[&location]
            .agents
            .iter()
            .find(|id| **id != observer)
            .copied()
            .expect("visible resident");
        let observer_agent = world.agents.get_mut(&observer).expect("observer");
        observer_agent.needs.food = 1.0;
        observer_agent.needs.energy = 0.0;
        observer_agent.injury = false;
        let visible_agent = world.agents.get_mut(&visible).expect("visible resident");
        visible_agent.needs.food = 0.0;
        visible_agent.needs.energy = 1.0;
        visible_agent.injury = true;
        visible_agent.disease = DiseaseState::Incubating {
            until: Tick(world.tick.0 + 10),
        };

        let observation = perceive(&world, observer).expect("observation");
        assert_eq!(
            observation.self_description.conditions,
            vec![HealthCondition::Exhausted]
        );
        let visible_agent = observation
            .visible_agents
            .iter()
            .find(|agent| agent.id == visible)
            .expect("visible resident");
        assert_eq!(
            visible_agent.conditions,
            vec![HealthCondition::Hungry, HealthCondition::Injured]
        );
        world
            .agents
            .get_mut(&visible)
            .expect("visible resident")
            .disease = DiseaseState::Symptomatic {
            until: Tick(world.tick.0 + 10),
        };
        let observation = perceive(&world, observer).expect("symptom observation");
        let visible_agent = observation
            .visible_agents
            .iter()
            .find(|agent| agent.id == visible)
            .expect("visible resident");
        assert!(visible_agent.conditions.contains(&HealthCondition::Sick));
        assert!(
            serde_json::to_value(visible_agent)
                .expect("visible JSON")
                .get("health")
                .is_none()
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
