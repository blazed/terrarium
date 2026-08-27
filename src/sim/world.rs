use super::{
    ActionRejection, ActionResult, Agent, AgentId, Event, EventId, EventKind, Goal, Location,
    LocationId, Needs, ObservationTarget, Occupation, Personality, ProposedAction, Relationship,
    Tick, seeded_uuid,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const MEMORY_LIMIT: usize = 20;

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
                        serves_food: matches!(
                            name,
                            "The Crooked Lantern" | "Mara's Bakery" | "General Store"
                        ),
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
                memories: Vec::new(),
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
        let elapsed = proposed.0 - self.tick.0;
        for agent in self.agents.values_mut() {
            agent.needs.decay(elapsed);
        }
        self.tick = proposed;
        Ok(())
    }

    pub fn advance_tick(&mut self) -> Result<(), WorldError> {
        let next = self.tick.0.checked_add(1).ok_or(WorldError::TickOverflow)?;
        self.advance_to(Tick(next))
    }

    pub fn from_snapshot(
        name: String,
        seed: u64,
        tick: Tick,
        agents: Vec<Agent>,
        locations: Vec<Location>,
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
            events,
        };
        world.validate()?;
        world.validate_history()?;
        Ok(world)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    fn validate_history(&self) -> Result<(), WorldError> {
        let mut previous_tick = Tick(0);
        let history = self
            .events
            .iter()
            .map(|event| (event.id, event))
            .collect::<BTreeMap<_, _>>();
        if history.len() != self.events.len() {
            return Err(WorldError::InvalidState(
                "checkpoint contains duplicate event IDs".into(),
            ));
        }
        for (index, event) in self.events.iter().enumerate() {
            if event.id != EventId(seeded_uuid(3, self.seed, index as u32)) {
                return Err(WorldError::InvalidState(format!(
                    "event {} is out of sequence",
                    event.id
                )));
            }
            if event.tick < previous_tick || event.tick > self.tick {
                return Err(WorldError::InvalidState(format!(
                    "event {} has an invalid tick",
                    event.id
                )));
            }
            if event
                .location
                .is_some_and(|location| !self.locations.contains_key(&location))
            {
                return Err(WorldError::InvalidState(format!(
                    "event {} references an unknown location",
                    event.id
                )));
            }
            previous_tick = event.tick;
        }
        for agent in self.agents.values() {
            for memory in &agent.memories {
                if history.get(&memory.id) != Some(&memory) {
                    return Err(WorldError::InvalidState(format!(
                        "agent {} remembers an event absent from history",
                        agent.id
                    )));
                }
            }
        }
        Ok(())
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
                if target == actor {
                    return self.reject(actor, Some(current), ActionRejection::SelfTarget(actor));
                }
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
            ProposedAction::Eat => {
                if current != agent.home && !self.locations[&current].serves_food {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotEatHere(current),
                    );
                }
                EventKind::Ate { agent: actor }
            }
            ProposedAction::Rest => {
                if current != agent.home {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotRestHere(current),
                    );
                }
                EventKind::Rested { agent: actor }
            }
            ProposedAction::Work => {
                if agent.workplace != Some(current) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotWorkHere(current),
                    );
                }
                let hour = self.tick.hour();
                if !(8..18).contains(&hour) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::OutsideWorkingHours(hour),
                    );
                }
                EventKind::Worked { agent: actor }
            }
            ProposedAction::Wait => EventKind::Waited { agent: actor },
        };

        self.apply_action_effects(actor, &kind);
        let event = self.append_event(Some(current), kind);
        ActionResult::Success(vec![event])
    }

    fn apply_action_effects(&mut self, actor: AgentId, kind: &EventKind) {
        match kind {
            EventKind::Moved { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.needs.energy = (agent.needs.energy - 0.01).max(0.0);
                }
            }
            EventKind::Spoke { listener, .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.companionship, 0.12);
                    Needs::restore(&mut agent.needs.status, 0.01);
                }
                if let Some(agent) = self.agents.get_mut(listener) {
                    Needs::restore(&mut agent.needs.companionship, 0.08);
                }
                self.strengthen_relationship(actor, *listener, 1.0);
                self.strengthen_relationship(*listener, actor, 0.75);
            }
            EventKind::Observed { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.safety, 0.03);
                }
            }
            EventKind::Ate { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.food, 0.25);
                    Needs::restore(&mut agent.needs.energy, 0.01);
                }
            }
            EventKind::Rested { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.energy, 0.25);
                    Needs::restore(&mut agent.needs.safety, 0.05);
                }
            }
            EventKind::Worked { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.money, 0.12);
                    Needs::restore(&mut agent.needs.status, 0.05);
                    agent.needs.energy = (agent.needs.energy - 0.03).max(0.0);
                    agent.needs.food = (agent.needs.food - 0.02).max(0.0);
                }
            }
            EventKind::Waited { .. } | EventKind::ActionRejected { .. } => {}
        }
    }

    fn strengthen_relationship(&mut self, source: AgentId, target: AgentId, amount: f32) {
        if let Some(agent) = self.agents.get_mut(&source) {
            let relationship = agent
                .relationships
                .entry(target)
                .or_insert(Relationship::NEUTRAL);
            relationship.affection = (relationship.affection + 0.02 * amount).min(1.0);
            relationship.trust = (relationship.trust + 0.015 * amount).min(1.0);
            relationship.respect = (relationship.respect + 0.005 * amount).min(1.0);
            relationship.suspicion = (relationship.suspicion - 0.01 * amount).max(-1.0);
        }
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
        self.remember(&event);
        event
    }

    fn remember(&mut self, event: &Event) {
        let mut witnesses = BTreeSet::new();
        match &event.kind {
            EventKind::Moved { agent, to, .. } => {
                witnesses.insert(*agent);
                if let Some(destination) = self.locations.get(to) {
                    witnesses.extend(destination.agents.iter().copied());
                }
            }
            EventKind::Spoke {
                speaker, listener, ..
            } => {
                witnesses.extend([*speaker, *listener]);
            }
            EventKind::Observed { observer, target } => {
                witnesses.insert(*observer);
                if let ObservationTarget::Agent(agent) = target {
                    witnesses.insert(*agent);
                }
            }
            EventKind::Ate { agent }
            | EventKind::Rested { agent }
            | EventKind::Worked { agent } => {
                witnesses.insert(*agent);
            }
            EventKind::Waited { .. } | EventKind::ActionRejected { .. } => return,
        }
        if let Some(location) = event.location.and_then(|id| self.locations.get(&id)) {
            witnesses.extend(location.agents.iter().copied());
        }

        for witness in witnesses {
            if let Some(agent) = self.agents.get_mut(&witness) {
                agent.memories.push(event.clone());
                let excess = agent.memories.len().saturating_sub(MEMORY_LIMIT);
                agent.memories.drain(..excess);
            }
        }
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
            if agent.relationships.iter().any(|(target, relationship)| {
                target == id || !self.agents.contains_key(target) || !relationship.is_normalized()
            }) {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has an invalid relationship"
                )));
            }
            if agent.memories.len() > MEMORY_LIMIT {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has too many memories"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionRejection, ActionResult, AgentId, EventKind, ObservationTarget, ProposedAction,
        Relationship, Tick, World, WorldError,
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
    fn time_and_successful_actions_update_needs() {
        let mut world = World::briar_glen(2).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let actor = residents[0];
        let listener = residents[1];
        let before = world.agents[&actor].needs.clone();

        world.advance_tick().expect("tick");
        let decayed = &world.agents[&actor].needs;
        assert!(decayed.food < before.food);
        assert!(decayed.energy < before.energy);
        assert!(decayed.companionship < before.companionship);

        let companionship = decayed.companionship;
        world.execute(
            actor,
            ProposedAction::Talk {
                target: listener,
                message: "Hello.".into(),
            },
        );
        assert!(world.agents[&actor].needs.companionship > companionship);

        let needs = world.agents[&actor].needs.clone();
        world.execute(actor, ProposedAction::Eat);
        assert!(world.agents[&actor].needs.food > needs.food);
        let energy = world.agents[&actor].needs.energy;
        let safety = world.agents[&actor].needs.safety;
        world.execute(actor, ProposedAction::Rest);
        assert!(world.agents[&actor].needs.energy > energy);
        assert!(world.agents[&actor].needs.safety > safety);
        assert!(
            world.agents[&listener]
                .memories
                .iter()
                .any(|event| matches!(event.kind, EventKind::Ate { agent } if agent == actor))
        );
        world.validate().expect("normalized needs");
    }

    #[test]
    fn work_and_activity_locations_are_authoritative() {
        let mut world = World::briar_glen(3).expect("town");
        let actor = *world
            .agents
            .values()
            .find(|agent| {
                agent
                    .workplace
                    .is_some_and(|id| !world.locations[&id].serves_food)
            })
            .map(|agent| &agent.id)
            .expect("non-food workplace");
        let workplace = world.agents[&actor].workplace.expect("workplace");
        world.advance_to(Tick(8 * 12)).expect("morning");
        assert!(matches!(
            world.execute(
                actor,
                ProposedAction::Move {
                    destination: workplace
                }
            ),
            ActionResult::Success(_)
        ));

        let needs = world.agents[&actor].needs.clone();
        assert!(matches!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Success(_)
        ));
        assert!(world.agents[&actor].needs.money > needs.money);
        assert_eq!(
            world.execute(actor, ProposedAction::Eat),
            ActionResult::Rejected(ActionRejection::CannotEatHere(workplace))
        );
        assert_eq!(
            world.execute(actor, ProposedAction::Rest),
            ActionResult::Rejected(ActionRejection::CannotRestHere(workplace))
        );

        world.advance_to(Tick(18 * 12)).expect("evening");
        assert_eq!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Rejected(ActionRejection::OutsideWorkingHours(18))
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
    fn only_present_agents_remember_a_conversation() {
        let mut world = World::briar_glen(41).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let remote = residents[0];
        let speaker = residents[1];
        let listener = residents[2];
        let home = world.agents[&remote].location;
        let destination = *world.locations[&home]
            .connected
            .iter()
            .next()
            .expect("destination");
        world.execute(remote, ProposedAction::Move { destination });
        for agent in world.agents.values_mut() {
            agent.memories.clear();
        }

        world.execute(
            speaker,
            ProposedAction::Talk {
                target: listener,
                message: "Did you hear the bells?".into(),
            },
        );

        assert!(world.agents[&remote].memories.is_empty());
        assert!(
            world.agents[&speaker]
                .memories
                .iter()
                .any(|memory| matches!(memory.kind, EventKind::Spoke { .. }))
        );
        assert!(
            world.agents[&residents[3]]
                .memories
                .iter()
                .any(|memory| matches!(memory.kind, EventKind::Spoke { .. }))
        );
    }

    #[test]
    fn conversations_build_bounded_mutual_relationships() {
        let mut world = World::briar_glen(42).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[2];
        assert!(!world.agents[&speaker].relationships.contains_key(&listener));
        assert!(!world.agents[&listener].relationships.contains_key(&speaker));

        for _ in 0..200 {
            assert!(matches!(
                world.execute(
                    speaker,
                    ProposedAction::Talk {
                        target: listener,
                        message: "Good to see you.".into(),
                    }
                ),
                ActionResult::Success(_)
            ));
        }

        let speaker_view = world.agents[&speaker].relationships[&listener];
        let listener_view = world.agents[&listener].relationships[&speaker];
        assert_eq!(speaker_view.affection, 1.0);
        assert_eq!(listener_view.affection, 1.0);
        assert_eq!(speaker_view.suspicion, -1.0);
        assert!(speaker_view.is_normalized() && listener_view.is_normalized());
        world.validate().expect("valid relationships");
    }

    #[test]
    fn invalid_relationship_targets_and_self_talk_are_rejected() {
        let mut world = World::briar_glen(43).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Talk {
                    target: actor,
                    message: "Hello, me.".into(),
                }
            ),
            ActionResult::Rejected(ActionRejection::SelfTarget(actor))
        );

        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .relationships
            .insert(actor, Relationship::NEUTRAL);
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));

        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .relationships
            .remove(&actor);
        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .relationships
            .insert(AgentId(Uuid::nil()), Relationship::NEUTRAL);
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn memories_keep_only_the_latest_twenty_events() {
        let mut world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let location = world.agents[&actor].location;

        for _ in 0..21 {
            world.execute(
                actor,
                ProposedAction::Observe {
                    target: ObservationTarget::Location(location),
                },
            );
        }

        assert_eq!(world.agents[&actor].memories.len(), 20);
        assert_eq!(world.agents[&actor].memories[0], world.events()[1]);
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
