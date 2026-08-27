use super::{
    ActionRejection, ActionResult, Agent, AgentId, Event, EventId, EventKind, Goal, Location,
    LocationId, Needs, ObservationTarget, Occupation, Personality, ProposedAction, Relationship,
    Tick, seeded_uuid,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

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
    events: Vec<Event>,
}

impl World {
    pub fn briar_glen(seed: u64) -> Result<Self, WorldError> {
        let location_names = [
            "The Crooked Lantern",
            "Mara's Bakery",
            "Town Hall",
            "General Store",
            "Old Chapel",
            "Riverside Houses",
            "Abandoned Mill",
            "Carpenter's Workshop",
        ];
        let location_ids: Vec<_> = (0..location_names.len())
            .map(|index| LocationId(seeded_uuid(2, seed, index as u32)))
            .collect();
        let mut locations: BTreeMap<_, _> = location_names
            .into_iter()
            .zip(location_ids.iter().copied())
            .map(|(name, id)| {
                (
                    id,
                    Location {
                        id,
                        name: name.into(),
                        connected: BTreeSet::new(),
                        agents: BTreeSet::new(),
                    },
                )
            })
            .collect();

        for (left, right) in [
            (0, 1),
            (0, 2),
            (0, 5),
            (1, 3),
            (1, 5),
            (2, 3),
            (2, 4),
            (3, 5),
            (4, 5),
            (4, 6),
            (5, 6),
            (5, 7),
            (6, 7),
        ] {
            locations
                .get_mut(&location_ids[left])
                .ok_or(WorldError::UnknownLocation(location_ids[left]))?
                .connected
                .insert(location_ids[right]);
            locations
                .get_mut(&location_ids[right])
                .ok_or(WorldError::UnknownLocation(location_ids[right]))?
                .connected
                .insert(location_ids[left]);
        }

        let residents = [
            ("Mara Quinn", 41, Occupation::Baker, 1),
            ("Elias Ward", 46, Occupation::Carpenter, 7),
            ("Alice Vale", 35, Occupation::Shopkeeper, 3),
            ("Bob Mercer", 29, Occupation::Laborer, 6),
            ("Clara Voss", 38, Occupation::Teacher, 2),
            ("Sheriff Hale", 52, Occupation::Sheriff, 2),
            ("Jonas Reed", 44, Occupation::Publican, 0),
            ("Iris Bell", 27, Occupation::Clerk, 2),
        ];
        let mut agents = BTreeMap::new();
        for (index, (name, age, occupation, workplace)) in residents.into_iter().enumerate() {
            let id = AgentId(seeded_uuid(1, seed, index as u32));
            let home = location_ids[5];
            let personality_offset = index as f32 * 0.03;
            let agent = Agent {
                id,
                name: name.into(),
                age,
                occupation,
                home,
                workplace: Some(location_ids[workplace]),
                location: home,
                personality: Personality {
                    openness: 0.45 + personality_offset,
                    agreeableness: 0.7 - personality_offset,
                    neuroticism: 0.25 + personality_offset,
                    honesty: 0.75 - personality_offset / 2.0,
                    ambition: 0.4 + personality_offset,
                    impulsiveness: 0.5 - personality_offset,
                },
                needs: Needs {
                    money: 0.5,
                    food: 0.2,
                    companionship: 0.3,
                    safety: 0.15,
                    status: 0.35,
                    energy: 0.8,
                },
                relationships: BTreeMap::new(),
                goals: vec![Goal(format!("Succeed as {name}"))],
            };
            locations
                .get_mut(&home)
                .ok_or(WorldError::UnknownLocation(home))?
                .agents
                .insert(id);
            agents.insert(id, agent);
        }

        let agent_ids: Vec<_> = agents.keys().copied().collect();
        for pair in agent_ids.windows(2) {
            agents
                .get_mut(&pair[0])
                .ok_or(WorldError::UnknownAgent(pair[0]))?
                .relationships
                .insert(
                    pair[1],
                    Relationship {
                        affection: 0.2,
                        trust: 0.1,
                        respect: 0.1,
                        ..Relationship::NEUTRAL
                    },
                );
        }

        let world = Self {
            name: "Briar Glen".into(),
            seed,
            tick: Tick(0),
            agents,
            locations,
            events: Vec::new(),
        };
        world.validate()?;
        Ok(world)
    }

    pub fn advance_to(&mut self, proposed: Tick) -> Result<(), WorldError> {
        if proposed <= self.tick {
            return Err(WorldError::NonMonotonicTick {
                current: self.tick,
                proposed,
            });
        }
        self.tick = proposed;
        Ok(())
    }

    pub fn advance_tick(&mut self) -> Result<(), WorldError> {
        let next = self.tick.0.checked_add(1).ok_or(WorldError::TickOverflow)?;
        self.tick = Tick(next);
        Ok(())
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn execute(&mut self, actor: AgentId, action: ProposedAction) -> ActionResult {
        let Some(agent) = self.agents.get(&actor) else {
            return self.reject(actor, None, ActionRejection::UnknownActor(actor));
        };
        let current = agent.location;

        let kind = match action {
            ProposedAction::Move { destination } => {
                let Some(destination_location) = self.locations.get(&destination) else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::UnknownLocation(destination),
                    );
                };
                let Some(source) = self.locations.get(&current) else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::UnknownLocation(current),
                    );
                };
                if !source.agents.contains(&actor) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::InvalidMembership(actor),
                    );
                }
                if !source.connected.contains(&destination) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::Disconnected {
                            from: current,
                            destination,
                        },
                    );
                }
                debug_assert!(!destination_location.agents.contains(&actor));
                if let Some(source) = self.locations.get_mut(&current) {
                    source.agents.remove(&actor);
                } else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::UnknownLocation(current),
                    );
                }
                if let Some(destination_state) = self.locations.get_mut(&destination) {
                    destination_state.agents.insert(actor);
                } else {
                    if let Some(source) = self.locations.get_mut(&current) {
                        source.agents.insert(actor);
                    }
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::UnknownLocation(destination),
                    );
                }
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.location = destination;
                } else {
                    if let Some(source) = self.locations.get_mut(&current) {
                        source.agents.insert(actor);
                    }
                    if let Some(destination_state) = self.locations.get_mut(&destination) {
                        destination_state.agents.remove(&actor);
                    }
                    return self.reject(actor, Some(current), ActionRejection::UnknownActor(actor));
                }
                EventKind::Moved {
                    agent: actor,
                    from: current,
                    to: destination,
                }
            }
            ProposedAction::Talk { target, message } => {
                let Some(target_agent) = self.agents.get(&target) else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::UnknownAgent(target),
                    );
                };
                if target_agent.location != current {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::NotCoLocated { actor, target },
                    );
                }
                if message.trim().is_empty() {
                    return self.reject(actor, Some(current), ActionRejection::EmptyMessage);
                }
                EventKind::Spoke {
                    speaker: actor,
                    listener: target,
                    message,
                }
            }
            ProposedAction::Observe { target } => {
                match &target {
                    ObservationTarget::Agent(target_agent) => {
                        let Some(target_state) = self.agents.get(target_agent) else {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::UnknownAgent(*target_agent),
                            );
                        };
                        if target_state.location != current {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::NotCoLocated {
                                    actor,
                                    target: *target_agent,
                                },
                            );
                        }
                    }
                    ObservationTarget::Location(target_location) => {
                        if !self.locations.contains_key(target_location) {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::UnknownLocation(*target_location),
                            );
                        }
                        if *target_location != current {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::LocationNotVisible {
                                    current,
                                    target: *target_location,
                                },
                            );
                        }
                    }
                }
                EventKind::Observed {
                    observer: actor,
                    target,
                }
            }
            ProposedAction::Wait => EventKind::Waited { agent: actor },
        };

        let event = self.append_event(Some(current), kind);
        ActionResult::Success(vec![event])
    }

    fn reject(
        &mut self,
        actor: AgentId,
        location: Option<LocationId>,
        reason: ActionRejection,
    ) -> ActionResult {
        self.append_event(
            location,
            EventKind::ActionRejected {
                agent: actor,
                reason: reason.clone(),
            },
        );
        ActionResult::Rejected(reason)
    }

    fn append_event(&mut self, location: Option<LocationId>, kind: EventKind) -> Event {
        let event = Event {
            id: EventId(seeded_uuid(3, self.seed, self.events.len() as u32)),
            tick: self.tick,
            location,
            kind,
        };
        self.events.push(event.clone());
        event
    }

    pub fn validate(&self) -> Result<(), WorldError> {
        for (id, location) in &self.locations {
            for connected in &location.connected {
                let other = self
                    .locations
                    .get(connected)
                    .ok_or(WorldError::UnknownLocation(*connected))?;
                if !other.connected.contains(id) {
                    return Err(WorldError::InvalidState(format!(
                        "connection between {id} and {connected} is not symmetric"
                    )));
                }
            }
        }

        let mut memberships = BTreeMap::<AgentId, usize>::new();
        for (location_id, location) in &self.locations {
            for agent_id in &location.agents {
                let agent = self
                    .agents
                    .get(agent_id)
                    .ok_or(WorldError::UnknownAgent(*agent_id))?;
                if agent.location != *location_id {
                    return Err(WorldError::InvalidState(format!(
                        "agent {agent_id} disagrees with location {location_id}"
                    )));
                }
                *memberships.entry(*agent_id).or_default() += 1;
            }
        }

        for (id, agent) in &self.agents {
            if !self.locations.contains_key(&agent.location) {
                return Err(WorldError::UnknownLocation(agent.location));
            }
            if memberships.get(id) != Some(&1) {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} belongs to {} locations",
                    memberships.get(id).copied().unwrap_or_default()
                )));
            }
            if !agent.personality.is_normalized() || !agent.needs.is_normalized() {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has non-normalized traits"
                )));
            }
            if agent
                .relationships
                .values()
                .any(|relationship| !relationship.is_normalized())
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has a non-normalized relationship"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionRejection, ActionResult, AgentId, EventKind, ProposedAction, Tick, World, WorldError,
    };
    use uuid::Uuid;

    #[test]
    fn briar_glen_has_consistent_residents() {
        let world = World::briar_glen(814_921).expect("town should construct");
        assert_eq!(world.agents.len(), 8);
        assert_eq!(world.locations.len(), 8);
        assert_eq!(
            world
                .locations
                .values()
                .map(|location| location.agents.len())
                .sum::<usize>(),
            8
        );
        world.validate().expect("town should be valid");
    }

    #[test]
    fn tick_only_moves_forward() {
        let mut world = World::briar_glen(1).expect("town should construct");
        world.advance_to(Tick(2)).expect("forward tick");
        assert_eq!(
            world.advance_to(Tick(1)),
            Err(WorldError::NonMonotonicTick {
                current: Tick(2),
                proposed: Tick(1),
            })
        );
    }

    #[test]
    fn town_identity_is_seeded() {
        assert_eq!(
            World::briar_glen(7).expect("town").agents,
            World::briar_glen(7).expect("town").agents
        );
        assert_ne!(
            World::briar_glen(7).expect("town").agents.keys().next(),
            World::briar_glen(8).expect("town").agents.keys().next()
        );
    }

    #[test]
    fn movement_updates_both_sides_and_records_an_event() {
        let mut world = World::briar_glen(4).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let from = world.agents[&actor].location;
        let destination = *world.locations[&from]
            .connected
            .iter()
            .next()
            .expect("connected location");

        assert!(matches!(
            world.execute(actor, ProposedAction::Move { destination }),
            ActionResult::Success(_)
        ));
        assert_eq!(world.agents[&actor].location, destination);
        assert!(!world.locations[&from].agents.contains(&actor));
        assert!(world.locations[&destination].agents.contains(&actor));
        assert!(matches!(world.events[0].kind, EventKind::Moved { .. }));
        world
            .validate()
            .expect("movement should preserve invariants");
    }

    #[test]
    fn rejected_action_changes_only_history() {
        let mut world = World::briar_glen(5).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let agents_before = world.agents.clone();
        let locations_before = world.locations.clone();
        let absent = AgentId(Uuid::nil());

        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Talk {
                    target: absent,
                    message: "Hello".into(),
                },
            ),
            ActionResult::Rejected(ActionRejection::UnknownAgent(absent))
        );
        assert_eq!(world.agents, agents_before);
        assert_eq!(world.locations, locations_before);
        assert!(matches!(
            world.events[0].kind,
            EventKind::ActionRejected { .. }
        ));
    }

    #[test]
    fn disconnected_move_is_rejected_without_mutation() {
        let mut world = World::briar_glen(6).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let from = world.agents[&actor].location;
        let destination = *world
            .locations
            .keys()
            .find(|id| **id != from && !world.locations[&from].connected.contains(id))
            .expect("disconnected location");
        let agents_before = world.agents.clone();
        let locations_before = world.locations.clone();

        assert!(matches!(
            world.execute(actor, ProposedAction::Move { destination }),
            ActionResult::Rejected(ActionRejection::Disconnected { .. })
        ));
        assert_eq!(world.agents, agents_before);
        assert_eq!(world.locations, locations_before);
    }

    #[test]
    fn known_but_absent_agent_cannot_be_addressed() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let target = *world
            .agents
            .keys()
            .find(|id| **id != actor)
            .expect("other resident");
        let from = world.agents[&target].location;
        let destination = *world.locations[&from]
            .connected
            .iter()
            .next()
            .expect("connected location");
        assert!(matches!(
            world.execute(target, ProposedAction::Move { destination }),
            ActionResult::Success(_)
        ));
        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Talk {
                    target,
                    message: "Can you hear me?".into(),
                }
            ),
            ActionResult::Rejected(ActionRejection::NotCoLocated { actor, target })
        );
    }

    #[test]
    fn event_log_keeps_insertion_order() {
        let mut world = World::briar_glen(8).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        world.advance_tick().expect("tick");
        world.execute(actor, ProposedAction::Wait);
        world.advance_tick().expect("tick");
        world.execute(actor, ProposedAction::Wait);

        assert_eq!(
            world
                .events()
                .iter()
                .map(|event| event.tick)
                .collect::<Vec<_>>(),
            vec![Tick(1), Tick(2)]
        );
        assert_ne!(world.events()[0].id, world.events()[1].id);
    }

    #[test]
    fn unknown_actor_is_rejected_without_panicking() {
        let mut world = World::briar_glen(7).expect("town");
        let unknown = AgentId(Uuid::nil());
        assert_eq!(
            world.execute(unknown, ProposedAction::Wait),
            ActionResult::Rejected(ActionRejection::UnknownActor(unknown))
        );
    }
}
