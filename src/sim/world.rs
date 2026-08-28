use super::{
    ActionRejection, ActionResult, Activity, Agent, AgentId, BUSINESS_STARTING_CASH, Business,
    ConfrontationOutcome, DeathCause, DialogueTone, DiseaseState, Event, EventId, EventKind, Goal,
    GoalKind, GoalTarget, Intention, IntentionGoal, Inventory, Item, LifeState, Location,
    LocationId, MAX_TALK_MESSAGE_CHARS, NEW_WORLD_START_HOUR, Needs, ObservationTarget, Occupation,
    Offering, OpeningHours, Personality, ProposedAction, Relationship, RoutingStats, Rumor,
    STARTING_STOCK, STOCK_PER_SHIFT, Tick, TownEvent, TownEventKind, WORK_WAGE, seeded_uuid,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

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

pub(crate) fn event_evidence(kind: &EventKind) -> Option<(AgentId, f32, f32, f32)> {
    match kind {
        EventKind::Spoke { speaker, tone, .. } => Some(match tone {
            DialogueTone::Friendly => (*speaker, 0.08, 0.0, -0.03),
            DialogueTone::Supportive => (*speaker, 0.06, 0.06, -0.03),
            DialogueTone::Neutral => (*speaker, 0.04, 0.0, 0.0),
            DialogueTone::Tense => (*speaker, 0.02, -0.03, 0.12),
        }),
        EventKind::Worked { agent, .. } => Some((*agent, 0.0, 0.08, 0.0)),
        _ => None,
    }
}

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
            "Briar Glen Clinic",
        ];
        let location_ids: Vec<_> = (0..location_names.len())
            .map(|index| LocationId(seeded_uuid(2, seed, index as u32)))
            .collect();
        let mut locations: BTreeMap<_, _> = location_names
            .into_iter()
            .zip(location_ids.iter().copied())
            .map(|(name, id)| {
                let offering = match name {
                    "The Crooked Lantern" | "Mara's Bakery" => Some((Offering::Meal, 5)),
                    "General Store" | "Abandoned Mill" => Some((Offering::Supplies, 6)),
                    "Carpenter's Workshop" => Some((Offering::Repairs, 8)),
                    "Briar Glen Clinic" => Some((Offering::Medicine, CLINIC_PRICE)),
                    "Town Hall" => Some((Offering::CivicServices, 4)),
                    _ => None,
                };
                (
                    id,
                    Location {
                        id,
                        name: name.into(),
                        business: offering.map(|(offering, price)| Business {
                            offering,
                            price,
                            cash: BUSINESS_STARTING_CASH,
                            stock: STARTING_STOCK,
                            revenue: 0,
                            wages_paid: 0,
                        }),
                        opening_hours: match name {
                            "The Crooked Lantern" => Some(OpeningHours {
                                opens_at_hour: 12,
                                closes_at_hour: 23,
                            }),
                            "Mara's Bakery" => Some(OpeningHours {
                                opens_at_hour: 6,
                                closes_at_hour: 14,
                            }),
                            "Old Chapel" => Some(OpeningHours {
                                opens_at_hour: 6,
                                closes_at_hour: 20,
                            }),
                            "Briar Glen Clinic" => Some(OpeningHours {
                                opens_at_hour: 8,
                                closes_at_hour: 20,
                            }),
                            "Riverside Houses" => None,
                            _ => Some(OpeningHours {
                                opens_at_hour: 8,
                                closes_at_hour: 18,
                            }),
                        },
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
            (2, 8),
            (5, 8),
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
            ("Iris Bell", 27, Occupation::Doctor, 8),
        ];
        let mut agents = BTreeMap::new();
        let mut rng = StdRng::seed_from_u64(seed);
        for (index, (name, age, occupation, workplace)) in residents.into_iter().enumerate() {
            let id = AgentId(seeded_uuid(1, seed, index as u32));
            let home = location_ids[5];
            let personality_offset = index as f32 * 0.03;
            let mut vary = |base: f32| (base + rng.random_range(-0.15..=0.15)).clamp(0.0, 1.0);
            let personality = Personality {
                openness: vary(0.45 + personality_offset),
                agreeableness: vary(0.7 - personality_offset),
                neuroticism: vary(0.25 + personality_offset),
                honesty: vary(0.75 - personality_offset / 2.0),
                ambition: vary(0.4 + personality_offset),
                impulsiveness: vary(0.5 - personality_offset),
            };
            let mut vary_need = |base: f32| (base + rng.random_range(-0.08..=0.08)).clamp(0.0, 1.0);
            let agent = Agent {
                id,
                name: name.into(),
                age,
                occupation,
                home,
                workplace: Some(location_ids[workplace]),
                location: home,
                personality,
                needs: Needs {
                    money: vary_need(0.5),
                    food: vary_need(0.2),
                    companionship: vary_need(0.3),
                    safety: vary_need(0.15),
                    status: vary_need(0.35),
                    energy: vary_need(0.8),
                },
                health: 1.0,
                injury: false,
                disease: DiseaseState::Susceptible,
                life: LifeState::Alive,
                balance: 20,
                routing: RoutingStats::default(),
                inventory: Inventory::default(),
                activity: None,
                intention: None,
                mood: 0.0,
                relationships: BTreeMap::new(),
                beliefs: BTreeMap::new(),
                goals: Vec::new(),
                memories: Vec::new(),
                rumors: Vec::new(),
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

        let mut world = Self {
            name: "Briar Glen".into(),
            seed,
            tick: Tick(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES),
            agents,
            locations,
            active_town_event: TownEvent::scheduled(
                seed,
                Tick(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES),
            ),
            events: Vec::new(),
        };
        for agent in agent_ids {
            world.refresh_goals(agent);
        }
        world.validate()?;
        Ok(world)
    }

    fn advance_disease_tick(&mut self) -> Vec<AgentId> {
        let mut emitted = Vec::new();
        if self.tick.0 == PATIENT_ZERO_TICK
            && !self
                .agents
                .values()
                .any(|agent| agent.is_alive() && agent.disease.is_infected())
        {
            let alive = self
                .agents
                .values()
                .filter(|agent| agent.is_alive())
                .map(|agent| agent.id)
                .collect::<Vec<_>>();
            if !alive.is_empty() {
                let patient_zero = alive[(self.seed as usize) % alive.len()];
                let location = self.agents[&patient_zero].location;
                self.agents
                    .get_mut(&patient_zero)
                    .expect("patient zero")
                    .disease = DiseaseState::Incubating {
                    until: Tick(self.tick.0 + INCUBATION_TICKS),
                };
                emitted.push((
                    Some(location),
                    EventKind::DiseaseInfected {
                        agent: patient_zero,
                        source: None,
                    },
                ));
            }
        }

        let transitions = self
            .agents
            .values()
            .filter(|agent| agent.is_alive())
            .filter_map(|agent| {
                let next = match agent.disease {
                    DiseaseState::Incubating { until } if until <= self.tick => Some((
                        DiseaseState::Symptomatic {
                            until: Tick(self.tick.0 + SYMPTOMATIC_TICKS),
                        },
                        Some(EventKind::DiseaseSymptoms { agent: agent.id }),
                    )),
                    DiseaseState::Symptomatic { until } if until <= self.tick => Some((
                        DiseaseState::Recovering {
                            until: Tick(self.tick.0 + RECOVERY_TICKS),
                        },
                        Some(EventKind::DiseaseRecovered { agent: agent.id }),
                    )),
                    DiseaseState::Recovering { until } if until <= self.tick => Some((
                        DiseaseState::Immune {
                            until: Tick(self.tick.0 + IMMUNITY_TICKS),
                        },
                        None,
                    )),
                    DiseaseState::Immune { until } if until <= self.tick => Some((
                        DiseaseState::Susceptible,
                        Some(EventKind::DiseaseImmunityExpired { agent: agent.id }),
                    )),
                    _ => None,
                }?;
                Some((agent.id, agent.location, next.0, next.1))
            })
            .collect::<Vec<_>>();
        for (agent, location, disease, event) in transitions {
            self.agents.get_mut(&agent).expect("disease owner").disease = disease;
            if let Some(event) = event {
                emitted.push((Some(location), event));
            }
        }

        if self.tick.0.is_multiple_of(60 / Tick::MINUTES) {
            let infectious = self
                .agents
                .values()
                .filter(|agent| agent.is_alive() && agent.disease.is_symptomatic())
                .map(|agent| (agent.id, agent.location))
                .collect::<Vec<_>>();
            let susceptible = self
                .agents
                .values()
                .filter(|agent| {
                    agent.is_alive() && matches!(agent.disease, DiseaseState::Susceptible)
                })
                .map(|agent| (agent.id, agent.location))
                .collect::<Vec<_>>();
            for (source, source_location) in infectious {
                for (target, target_location) in susceptible.iter().copied() {
                    if source_location != target_location
                        || !matches!(self.agents[&target].disease, DiseaseState::Susceptible)
                        || !self.disease_exposure_succeeds(source, target)
                    {
                        continue;
                    }
                    self.agents
                        .get_mut(&target)
                        .expect("infection target")
                        .disease = DiseaseState::Incubating {
                        until: Tick(self.tick.0 + INCUBATION_TICKS),
                    };
                    emitted.push((
                        Some(target_location),
                        EventKind::DiseaseInfected {
                            agent: target,
                            source: Some(source),
                        },
                    ));
                }
            }
        }

        for (location, event) in emitted {
            self.append_event(location, event);
        }
        self.agents
            .values()
            .filter(|agent| agent.is_alive() && agent.disease.is_symptomatic())
            .map(|agent| agent.id)
            .collect()
    }

    fn disease_exposure_succeeds(&self, source: AgentId, target: AgentId) -> bool {
        let source = source.0.as_u128();
        let target = target.0.as_u128();
        let mut value = self.seed
            ^ self.tick.0.wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ source as u64
            ^ (source >> 64) as u64
            ^ target as u64
            ^ (target >> 64) as u64;
        value ^= value >> 30;
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        value % 100 < 35
    }

    pub fn advance_to(&mut self, proposed: Tick) -> Result<(), WorldError> {
        if proposed <= self.tick {
            return Err(WorldError::NonMonotonicTick {
                current: self.tick,
                proposed,
            });
        }
        let elapsed = proposed.0 - self.tick.0;
        let mut storm_ticks = 0;
        let mut disease_ticks = BTreeMap::<AgentId, u64>::new();
        while self.tick < proposed {
            self.tick.0 += 1;
            self.update_town_event();
            for agent in self.advance_disease_tick() {
                *disease_ticks.entry(agent).or_default() += 1;
            }
            storm_ticks += u64::from(
                self.active_town_event
                    .is_some_and(|event| event.kind == TownEventKind::Storm),
            );
        }
        let mut deaths = Vec::new();
        for agent in self.agents.values_mut() {
            if !agent.is_alive() {
                continue;
            }
            agent.needs.decay(elapsed);
            agent.needs.safety = (agent.needs.safety - 0.0004 * storm_ticks as f32).max(0.0);
            agent.decay_mood(elapsed);
            agent.decay_beliefs(elapsed);
            let urgent =
                agent.needs.food < 0.1 || agent.needs.energy < 0.1 || agent.needs.safety < 0.1;
            if agent
                .activity
                .is_some_and(|activity| activity.until <= proposed || urgent)
            {
                agent.activity = None;
            }
            if agent
                .intention
                .as_ref()
                .is_some_and(|intention| intention.expires_at <= proposed)
            {
                agent.intention = None;
            }
            agent.goals.retain(|goal| goal.expires_at > proposed);
            let critical_needs = [agent.needs.food, agent.needs.energy, agent.needs.safety]
                .into_iter()
                .filter(|need| *need < 0.05)
                .count();
            let damage = (critical_needs as f32 * 0.00001
                + if agent.injury { 0.000005 } else { 0.0 })
                * elapsed as f32
                + disease_ticks.get(&agent.id).copied().unwrap_or_default() as f32
                    * DISEASE_DAMAGE_PER_TICK;
            agent.health = (agent.health - damage).max(0.0);
            agent.mood = (agent.mood - damage * 10.0).max(-1.0);
            if agent.needs.safety < 0.05 {
                agent.injury = true;
            }
            if agent.health <= 0.0 {
                let cause = if agent.disease.is_symptomatic() {
                    DeathCause::Disease
                } else if agent.needs.food <= agent.needs.energy
                    && agent.needs.food <= agent.needs.safety
                {
                    DeathCause::Starvation
                } else if agent.needs.energy <= agent.needs.safety {
                    DeathCause::Exhaustion
                } else {
                    DeathCause::Injury
                };
                agent.life = LifeState::Dead {
                    tick: proposed,
                    cause,
                };
                agent.disease = DiseaseState::Susceptible;
                agent.activity = None;
                agent.intention = None;
                agent.goals.clear();
                deaths.push((agent.id, agent.location, cause));
            }
        }
        let deceased = deaths
            .iter()
            .map(|(agent, _, _)| *agent)
            .collect::<BTreeSet<_>>();
        for (agent, location, _) in &deaths {
            if let Some(location) = self.locations.get_mut(location) {
                location.agents.remove(agent);
            }
        }
        for (agent, location, cause) in deaths {
            self.append_event(Some(location), EventKind::Died { agent, cause });
        }
        for agent in self.agents.values_mut().filter(|agent| agent.is_alive()) {
            if agent.intention.as_ref().is_some_and(|intention| {
                matches!(
                    intention.goal,
                    IntentionGoal::Talk { target, .. } if deceased.contains(&target)
                )
            }) {
                agent.intention = None;
            }
        }
        let agents = self
            .agents
            .values()
            .filter(|agent| agent.is_alive())
            .map(|agent| agent.id)
            .collect::<Vec<_>>();
        for agent in agents {
            self.refresh_goals(agent);
        }
        Ok(())
    }

    fn update_town_event(&mut self) {
        if self
            .active_town_event
            .is_some_and(|event| event.ends_at == self.tick)
        {
            let event = self.active_town_event.take().expect("active town event");
            self.append_event(None, EventKind::TownEventEnded { kind: event.kind });
        }
        if self.active_town_event.is_none()
            && let Some(event) = TownEvent::scheduled(self.seed, self.tick)
            && event.starts_at == self.tick
        {
            self.active_town_event = Some(event);
            if event.kind == TownEventKind::Storm {
                for agent in self.agents.values_mut() {
                    agent.intention = None;
                }
            }
            self.append_event(
                None,
                EventKind::TownEventStarted {
                    kind: event.kind,
                    ends_at: event.ends_at,
                },
            );
        }
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
            let invalid_disease_event = match &event.kind {
                EventKind::DiseaseInfected { agent, source } => {
                    !self.agents.contains_key(agent)
                        || source.is_some_and(|source| {
                            source == *agent || !self.agents.contains_key(&source)
                        })
                }
                EventKind::DiseaseSymptoms { agent }
                | EventKind::DiseaseRecovered { agent }
                | EventKind::DiseaseImmunityExpired { agent } => !self.agents.contains_key(agent),
                _ => false,
            };
            if invalid_disease_event {
                return Err(WorldError::InvalidState(format!(
                    "event {} references an invalid disease agent",
                    event.id
                )));
            }
            let invalid_transaction = match &event.kind {
                EventKind::Purchased { offering, cost, .. } => event
                    .location
                    .and_then(|location| self.locations.get(&location))
                    .and_then(|location| location.business)
                    .is_none_or(|business| {
                        *offering != business.offering || *cost != business.price
                    }),
                EventKind::Treated { cost, .. } => event
                    .location
                    .and_then(|location| self.locations.get(&location))
                    .and_then(|location| location.business)
                    .is_none_or(|business| {
                        business.offering != Offering::Medicine || *cost != business.price
                    }),
                EventKind::Worked {
                    wage,
                    stock_produced,
                    ..
                } => {
                    *wage != WORK_WAGE
                        || *stock_produced != self.stock_per_shift(event.tick)
                        || event
                            .location
                            .and_then(|location| self.locations.get(&location))
                            .is_none_or(|location| location.business.is_none())
                }
                _ => false,
            };
            if invalid_transaction {
                return Err(WorldError::InvalidState(format!(
                    "event {} has an invalid transaction amount",
                    event.id
                )));
            }
            previous_tick = event.tick;
        }
        for agent in self.agents.values() {
            for event in agent
                .memories
                .iter()
                .chain(agent.rumors.iter().map(|rumor| &rumor.event))
            {
                if matches!(&event.kind, EventKind::DiseaseInfected { .. }) {
                    return Err(WorldError::InvalidState(format!(
                        "agent {} remembers a hidden infection",
                        agent.id
                    )));
                }
                if history.get(&event.id) != Some(&event) {
                    return Err(WorldError::InvalidState(format!(
                        "agent {} knows an event absent from history",
                        agent.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn is_location_open(&self, location: LocationId) -> bool {
        if self
            .active_town_event
            .is_some_and(|event| event.kind == TownEventKind::Storm)
        {
            self.agents.values().any(|agent| agent.home == location)
        } else {
            self.locations[&location].is_open(self.tick.hour())
        }
    }

    pub fn execute(&mut self, actor: AgentId, action: ProposedAction) -> ActionResult {
        let Some(agent) = self.agents.get(&actor) else {
            return self.reject(actor, None, ActionRejection::UnknownActor(actor));
        };
        if !agent.is_alive() {
            return self.reject(
                actor,
                Some(agent.location),
                ActionRejection::AgentDead(actor),
            );
        }
        let current = agent.location;

        let kind = match action {
            ProposedAction::Pursue { intention } => {
                return self.start_intention(actor, intention);
            }
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
                if !self.is_location_open(destination) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::LocationClosed(destination),
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
            ProposedAction::Talk {
                target,
                tone,
                message,
            } => {
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
                if !target_agent.is_alive() {
                    return self.reject(actor, Some(current), ActionRejection::AgentDead(target));
                }
                if target_agent.location != current {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::NotCoLocated { actor, target },
                    );
                }
                let message = message.trim();
                if message.is_empty() {
                    return self.reject(actor, Some(current), ActionRejection::EmptyMessage);
                }
                if message.chars().any(char::is_control) {
                    return self.reject(actor, Some(current), ActionRejection::InvalidMessage);
                }
                if message.chars().count() > MAX_TALK_MESSAGE_CHARS {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::MessageTooLong {
                            max: MAX_TALK_MESSAGE_CHARS,
                        },
                    );
                }
                EventKind::Spoke {
                    speaker: actor,
                    listener: target,
                    tone,
                    message: message.into(),
                }
            }
            ProposedAction::Confront { target, claim } => {
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
                if !target_agent.is_alive() {
                    return self.reject(actor, Some(current), ActionRejection::AgentDead(target));
                }
                if target_agent.location != current {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::NotCoLocated { actor, target },
                    );
                }
                let Some(rumor) = self.agents[&actor]
                    .rumors
                    .iter()
                    .find(|rumor| rumor.event.id == claim && !rumor.resolved)
                    .cloned()
                else {
                    return self.reject(actor, Some(current), ActionRejection::UnknownClaim(claim));
                };
                if self.events.iter().find(|event| event.id == claim) != Some(&rumor.event)
                    || !matches!(event_evidence(&rumor.event.kind), Some((subject, ..)) if subject == target)
                {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ClaimNotAboutTarget { claim, target },
                    );
                }
                let outcome = self.resolve_confrontation(actor, target, &rumor);
                EventKind::Confronted {
                    accuser: actor,
                    target,
                    claim,
                    outcome,
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
            ProposedAction::Purchase => {
                let location = &self.locations[&current];
                if location.business.is_none() {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotPurchaseHere(current),
                    );
                }
                if !self.is_location_open(current) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::LocationClosed(current),
                    );
                }
                let business = location.business.expect("validated business");
                if business.stock == 0 {
                    return self.reject(actor, Some(current), ActionRejection::SoldOut(current));
                }
                if agent.balance < business.price {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::InsufficientFunds {
                            cost: business.price,
                            available: agent.balance,
                        },
                    );
                }
                if let Some(item) = business.offering.item()
                    && !agent.inventory.has_capacity(item)
                {
                    return self.reject(actor, Some(current), ActionRejection::InventoryFull(item));
                }
                if business.revenue.checked_add(business.price).is_none()
                    || business.cash.checked_add(business.price).is_none()
                {
                    return self.reject(actor, Some(current), ActionRejection::EconomyOverflow);
                }
                EventKind::Purchased {
                    agent: actor,
                    offering: business.offering,
                    cost: business.price,
                }
            }
            ProposedAction::ConsumeMeal => {
                if agent.inventory.meals == 0 {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemUnavailable(Item::Meal),
                    );
                }
                EventKind::ItemUsed {
                    agent: actor,
                    item: Item::Meal,
                }
            }
            ProposedAction::UseSupplies => {
                if agent.inventory.supplies == 0 {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemUnavailable(Item::Supplies),
                    );
                }
                EventKind::ItemUsed {
                    agent: actor,
                    item: Item::Supplies,
                }
            }
            ProposedAction::UseRepairKit => {
                if agent.inventory.repair_kits == 0 {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemUnavailable(Item::RepairKit),
                    );
                }
                EventKind::ItemUsed {
                    agent: actor,
                    item: Item::RepairKit,
                }
            }
            ProposedAction::UseMedicine => {
                if agent.inventory.medicine == 0 {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemUnavailable(Item::Medicine),
                    );
                }
                if !agent.injury && !agent.disease.is_symptomatic() {
                    return self.reject(actor, Some(current), ActionRejection::NoMedicalNeed);
                }
                EventKind::ItemUsed {
                    agent: actor,
                    item: Item::Medicine,
                }
            }
            ProposedAction::SeekTreatment => {
                let Some(business) = self.locations[&current].business else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotSeekTreatmentHere(current),
                    );
                };
                if business.offering != Offering::Medicine {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotSeekTreatmentHere(current),
                    );
                }
                if !agent.injury && !agent.disease.is_symptomatic() {
                    return self.reject(actor, Some(current), ActionRejection::NoMedicalNeed);
                }
                if !self.is_location_open(current) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::LocationClosed(current),
                    );
                }
                if business.stock == 0 {
                    return self.reject(actor, Some(current), ActionRejection::SoldOut(current));
                }
                if agent.balance < business.price {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::InsufficientFunds {
                            cost: business.price,
                            available: agent.balance,
                        },
                    );
                }
                if business.cash.checked_add(business.price).is_none()
                    || business.revenue.checked_add(business.price).is_none()
                {
                    return self.reject(actor, Some(current), ActionRejection::EconomyOverflow);
                }
                EventKind::Treated {
                    agent: actor,
                    cost: business.price,
                }
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
                if agent.health < 0.2 || agent.injury {
                    return self.reject(actor, Some(current), ActionRejection::TooUnwell);
                }
                if !self.is_location_open(current) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::LocationClosed(current),
                    );
                }
                let Some(business) = self.locations[&current].business else {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::CannotWorkHere(current),
                    );
                };
                if business.cash < WORK_WAGE {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::InsolventEmployer {
                            location: current,
                            wage: WORK_WAGE,
                            available: business.cash,
                        },
                    );
                }
                let stock_produced = self.stock_per_shift(self.tick);
                if agent.balance.checked_add(WORK_WAGE).is_none()
                    || business.wages_paid.checked_add(WORK_WAGE).is_none()
                    || business.stock.checked_add(stock_produced).is_none()
                {
                    return self.reject(actor, Some(current), ActionRejection::EconomyOverflow);
                }
                EventKind::Worked {
                    agent: actor,
                    wage: WORK_WAGE,
                    stock_produced,
                }
            }
            ProposedAction::Wait => EventKind::Waited { agent: actor },
        };

        let starts_recovery = self.agents[&actor].disease.is_symptomatic()
            && matches!(
                &kind,
                EventKind::ItemUsed {
                    item: Item::Medicine,
                    ..
                } | EventKind::Treated { .. }
            );
        self.apply_action_effects(actor, &kind);
        if let EventKind::Spoke { listener, .. } = &kind {
            self.share_rumor(actor, *listener);
        }
        if let Some(mut activity) = Activity::from_event(&kind, self.tick) {
            if self.agents[&actor].health < 0.5 {
                let duration = activity.until.0.saturating_sub(self.tick.0);
                activity.until = Tick(self.tick.0.saturating_add(duration.saturating_mul(2)));
            }
            self.agents.get_mut(&actor).expect("known actor").activity = Some(activity);
            let other = match &kind {
                EventKind::Spoke { listener, .. } => Some(*listener),
                EventKind::Confronted { target, .. } => Some(*target),
                _ => None,
            };
            if let Some(other) = other {
                self.agents
                    .get_mut(&other)
                    .expect("validated resident")
                    .activity = Some(activity);
            }
        }
        let completed_goal = self.advance_goal(actor, &kind);
        let mut events = vec![self.append_event(Some(current), kind)];
        if starts_recovery {
            events.push(
                self.append_event(Some(current), EventKind::DiseaseRecovered { agent: actor }),
            );
        }
        if let Some(goal) = completed_goal {
            events.push(self.append_event(
                Some(current),
                EventKind::GoalCompleted { agent: actor, goal },
            ));
            self.refresh_goals(actor);
        }
        ActionResult::Success(events)
    }

    fn start_intention(&mut self, actor: AgentId, goal: IntentionGoal) -> ActionResult {
        let expires_at = Tick(self.tick.0.saturating_add(INTENTION_DURATION_TICKS));
        let intention = Intention { goal, expires_at };
        if let Err(rejection) = self.intention_action(actor, &intention) {
            let location = self.agents.get(&actor).map(|agent| agent.location);
            return self.reject(actor, location, rejection);
        }
        self.agents
            .get_mut(&actor)
            .expect("validated intention actor")
            .intention = Some(intention);
        self.continue_intention(actor)
            .unwrap_or_else(|| self.execute(actor, ProposedAction::Wait))
    }

    pub fn continue_intention(&mut self, actor: AgentId) -> Option<ActionResult> {
        if !self.agents.get(&actor).is_some_and(Agent::is_alive) {
            return None;
        }
        let intention = self.agents.get(&actor)?.intention.clone()?;
        let needs = &self.agents.get(&actor)?.needs;
        let purchase_offering = match intention.goal {
            IntentionGoal::Purchase { destination } => self
                .locations
                .get(&destination)
                .and_then(|location| location.business)
                .map(|business| business.offering),
            _ => None,
        };
        let interrupted = (needs.food < 0.1 && purchase_offering != Some(Offering::Meal))
            || (needs.energy < 0.1 && !matches!(&intention.goal, IntentionGoal::Rest))
            || (needs.safety < 0.1
                && !matches!(
                    purchase_offering,
                    Some(Offering::Supplies | Offering::Repairs)
                ));
        if intention.expires_at <= self.tick || interrupted {
            self.agents.get_mut(&actor)?.intention = None;
            return None;
        }
        let action = match self.intention_action(actor, &intention) {
            Ok(Some(action)) => action,
            Ok(None) => {
                self.agents.get_mut(&actor)?.intention = None;
                return None;
            }
            Err(rejection) => {
                let location = self.agents.get(&actor).map(|agent| agent.location);
                self.agents.get_mut(&actor)?.intention = None;
                return Some(self.reject(actor, location, rejection));
            }
        };
        let terminal = !matches!(action, ProposedAction::Move { .. });
        let result = self.execute(actor, action);
        let completed = terminal
            || matches!(result, ActionResult::Rejected(_))
            || self.intention_complete(actor, &intention.goal);
        if completed && let Some(agent) = self.agents.get_mut(&actor) {
            agent.intention = None;
        }
        Some(result)
    }

    fn intention_action(
        &self,
        actor: AgentId,
        intention: &Intention,
    ) -> Result<Option<ProposedAction>, ActionRejection> {
        let agent = self
            .agents
            .get(&actor)
            .ok_or(ActionRejection::UnknownActor(actor))?;
        let travel = |destination| {
            self.next_route_step(agent.location, destination)
                .map(|step| step.map(|destination| ProposedAction::Move { destination }))
        };
        match &intention.goal {
            IntentionGoal::Visit { destination } => travel(*destination),
            IntentionGoal::Purchase { destination } => {
                let location = self
                    .locations
                    .get(destination)
                    .ok_or(ActionRejection::UnknownLocation(*destination))?;
                if location.business.is_none() {
                    return Err(ActionRejection::CannotPurchaseHere(*destination));
                }
                if agent.location == *destination {
                    Ok(Some(ProposedAction::Purchase))
                } else {
                    travel(*destination)
                }
            }
            IntentionGoal::Rest => {
                if agent.location == agent.home {
                    Ok(Some(ProposedAction::Rest))
                } else {
                    travel(agent.home)
                }
            }
            IntentionGoal::SeekTreatment => {
                let clinic = self
                    .clinic_location()
                    .ok_or(ActionRejection::CannotSeekTreatmentHere(agent.location))?;
                if agent.location == clinic {
                    Ok(Some(ProposedAction::SeekTreatment))
                } else {
                    travel(clinic)
                }
            }
            IntentionGoal::Work => {
                let workplace = agent
                    .workplace
                    .ok_or(ActionRejection::CannotWorkHere(agent.location))?;
                if agent.location == workplace {
                    Ok(Some(ProposedAction::Work))
                } else {
                    travel(workplace)
                }
            }
            IntentionGoal::Talk {
                target,
                tone,
                message,
            } => Ok(Some(ProposedAction::Talk {
                target: *target,
                tone: *tone,
                message: message.clone(),
            })),
        }
    }

    pub(crate) fn clinic_location(&self) -> Option<LocationId> {
        self.locations.iter().find_map(|(id, location)| {
            location
                .business
                .is_some_and(|business| business.offering == Offering::Medicine)
                .then_some(*id)
        })
    }

    pub(crate) fn shortest_open_route(
        &self,
        from: LocationId,
        targets: &BTreeSet<LocationId>,
    ) -> Option<(LocationId, LocationId, u32)> {
        if targets.contains(&from) || !self.locations.contains_key(&from) {
            return None;
        }
        let mut queue = VecDeque::from([(from, from, 0)]);
        let mut visited = BTreeSet::from([from]);
        while let Some((location, first_step, distance)) = queue.pop_front() {
            for next in &self.locations[&location].connected {
                if !visited.insert(*next) || !self.is_location_open(*next) {
                    continue;
                }
                let first_step = if location == from { *next } else { first_step };
                if targets.contains(next) {
                    return Some((*next, first_step, distance + 1));
                }
                queue.push_back((*next, first_step, distance + 1));
            }
        }
        None
    }

    fn next_route_step(
        &self,
        from: LocationId,
        destination: LocationId,
    ) -> Result<Option<LocationId>, ActionRejection> {
        if !self.locations.contains_key(&destination) {
            return Err(ActionRejection::UnknownLocation(destination));
        }
        if from == destination {
            return Ok(None);
        }
        self.shortest_open_route(from, &BTreeSet::from([destination]))
            .map(|(_, next, _)| Some(next))
            .ok_or(ActionRejection::NoRoute { from, destination })
    }

    fn intention_complete(&self, actor: AgentId, goal: &IntentionGoal) -> bool {
        let Some(agent) = self.agents.get(&actor) else {
            return true;
        };
        matches!(goal, IntentionGoal::Visit { destination } if agent.location == *destination)
    }

    fn share_rumor(&mut self, speaker: AgentId, listener: AgentId) {
        let listener_state = &self.agents[&listener];
        let known = listener_state
            .memories
            .iter()
            .map(|event| event.id)
            .chain(listener_state.rumors.iter().map(|rumor| rumor.event.id))
            .collect::<BTreeSet<_>>();
        let Some((event, depth, base_confidence)) = self.agents.get(&speaker).and_then(|agent| {
            agent
                .memories
                .iter()
                .rev()
                .find(|event| !known.contains(&event.id))
                .map(|event| (event.clone(), 1, 0.9))
                .or_else(|| {
                    agent
                        .rumors
                        .iter()
                        .rev()
                        .find(|rumor| !known.contains(&rumor.event.id))
                        .and_then(|rumor| {
                            rumor
                                .depth
                                .checked_add(1)
                                .map(|depth| (rumor.event.clone(), depth, rumor.confidence * 0.7))
                        })
                })
        }) else {
            return;
        };

        let honesty = self.agents[&speaker].personality.honesty;
        let relationship = self.agents[&listener]
            .relationships
            .get(&speaker)
            .copied()
            .unwrap_or(Relationship::NEUTRAL);
        let perceived_trust =
            ((relationship.trust - relationship.suspicion + 2.0) / 4.0).clamp(0.0, 1.0);
        let confidence = base_confidence * (0.5 + 0.5 * honesty) * (0.5 + 0.5 * perceived_trust);
        if confidence < 0.15 {
            return;
        }

        let agent = self.agents.get_mut(&listener).expect("known listener");
        if let Some((subject, sociability, reliability, hostility)) = event_evidence(&event.kind)
            && subject != listener
        {
            agent.learn_about_weighted(subject, sociability, reliability, hostility, confidence);
        }
        agent.rumors.push(Rumor {
            event,
            source: speaker,
            depth,
            confidence,
            resolved: false,
        });
        let excess = agent.rumors.len().saturating_sub(RUMOR_LIMIT);
        agent.rumors.drain(..excess);
    }

    fn resolve_confrontation(
        &mut self,
        accuser: AgentId,
        target: AgentId,
        rumor: &Rumor,
    ) -> ConfrontationOutcome {
        let response = &self.agents[&target];
        let toward_accuser = response
            .relationships
            .get(&accuser)
            .copied()
            .unwrap_or(Relationship::NEUTRAL);
        let source_credibility = self.agents[&accuser]
            .relationships
            .get(&rumor.source)
            .map_or(0.5, |relationship| {
                ((relationship.trust - relationship.suspicion + 2.0) / 4.0).clamp(0.0, 1.0)
            });
        let candor = response.personality.honesty
            + 0.15 * (toward_accuser.trust - toward_accuser.suspicion)
            + 0.1 * source_credibility
            + 0.1 * response.mood;
        let outcome = if candor >= 0.65 {
            ConfrontationOutcome::Confirmed
        } else if candor <= 0.4 {
            ConfrontationOutcome::Denied
        } else {
            ConfrontationOutcome::Challenged
        };

        let accuser_state = self.agents.get_mut(&accuser).expect("known accuser");
        if let Some(known) = accuser_state
            .rumors
            .iter_mut()
            .find(|known| known.event.id == rumor.event.id)
        {
            known.confidence = match outcome {
                ConfrontationOutcome::Confirmed => known.confidence.max(0.9),
                ConfrontationOutcome::Denied => known.confidence * 0.5,
                ConfrontationOutcome::Challenged => known.confidence * 0.75,
            };
            known.resolved = true;
        }
        if let Some((subject, sociability, reliability, hostility)) =
            event_evidence(&rumor.event.kind)
        {
            match outcome {
                ConfrontationOutcome::Confirmed => accuser_state.learn_about_weighted(
                    subject,
                    sociability,
                    reliability,
                    hostility,
                    1.0,
                ),
                ConfrontationOutcome::Denied | ConfrontationOutcome::Challenged => {
                    if let Some(belief) = accuser_state.beliefs.get_mut(&subject) {
                        belief.confidence *= if outcome == ConfrontationOutcome::Denied {
                            0.7
                        } else {
                            0.85
                        };
                    }
                }
            }
        }

        let weak_accusation = rumor.confidence < 0.5;
        let adjust = |relationship: &mut Relationship, trust: f32, suspicion: f32| {
            relationship.trust = (relationship.trust + trust).clamp(-1.0, 1.0);
            relationship.suspicion = (relationship.suspicion + suspicion).clamp(-1.0, 1.0);
        };
        let accuser_relationship = self
            .agents
            .get_mut(&accuser)
            .expect("known accuser")
            .relationships
            .entry(target)
            .or_insert(Relationship::NEUTRAL);
        match outcome {
            ConfrontationOutcome::Confirmed => adjust(accuser_relationship, 0.04, -0.03),
            ConfrontationOutcome::Denied => adjust(accuser_relationship, -0.05, 0.06),
            ConfrontationOutcome::Challenged => adjust(accuser_relationship, -0.01, 0.03),
        }
        let target_relationship = self
            .agents
            .get_mut(&target)
            .expect("known target")
            .relationships
            .entry(accuser)
            .or_insert(Relationship::NEUTRAL);
        let (trust, suspicion) = match outcome {
            ConfrontationOutcome::Confirmed if !weak_accusation => (0.01, 0.0),
            ConfrontationOutcome::Confirmed => (-0.02, 0.03),
            ConfrontationOutcome::Denied => (-0.05, 0.08),
            ConfrontationOutcome::Challenged => (-0.03, 0.05),
        };
        adjust(target_relationship, trust, suspicion);
        outcome
    }

    fn refresh_goals(&mut self, actor: AgentId) {
        let Some(agent) = self.agents.get(&actor) else {
            return;
        };
        if !agent.is_alive() {
            return;
        }
        let mut goals = agent.goals.clone();
        goals.retain(|goal| {
            goal.expires_at > self.tick
                && self.goal_target_is_valid(actor, &goal.target)
                && self.goal_target_is_available(&goal.target)
        });
        let mut targets = goals
            .iter()
            .map(|goal| goal.target)
            .collect::<BTreeSet<_>>();
        for (_, goal) in self.goal_candidates(actor) {
            if goals.len() == GOAL_LIMIT {
                break;
            }
            if targets.insert(goal.target) {
                goals.push(goal);
            }
        }
        self.agents.get_mut(&actor).expect("known goal owner").goals = goals;
    }

    fn goal_candidates(&self, actor: AgentId) -> Vec<(f32, Goal)> {
        let agent = &self.agents[&actor];
        let expires_at = Tick(self.tick.0.saturating_add(GOAL_DURATION_TICKS));
        let mut candidates = Vec::new();
        if let Some(workplace) = agent.workplace
            && self.locations[&workplace]
                .business
                .is_some_and(Business::solvent)
        {
            candidates.push((
                agent.personality.ambition + (1.0 - agent.needs.money),
                Goal::new(
                    format!("Complete two shifts at {}", self.locations[&workplace].name),
                    GoalKind::Livelihood,
                    GoalTarget::Work { workplace },
                    2,
                    expires_at,
                ),
            ));
        }

        let mut residents = self
            .agents
            .values()
            .filter(|resident| resident.is_alive() && resident.id != actor)
            .map(|resident| resident.id)
            .collect::<Vec<_>>();
        residents.sort_by(|left, right| {
            let score = |resident| {
                let relationship = agent
                    .relationships
                    .get(resident)
                    .copied()
                    .unwrap_or(Relationship::NEUTRAL);
                relationship.score()
            };
            score(right)
                .total_cmp(&score(left))
                .then_with(|| left.cmp(right))
        });
        if !residents.is_empty() {
            let resident = residents[self.goal_choice(actor, 1, residents.len())];
            candidates.push((
                agent.personality.agreeableness + (1.0 - agent.needs.companionship),
                Goal::new(
                    format!("Catch up with {}", self.agents[&resident].name),
                    GoalKind::Community,
                    GoalTarget::Talk { resident },
                    1,
                    expires_at,
                ),
            ));
        }

        let visited = agent
            .memories
            .iter()
            .filter_map(|event| match event.kind {
                EventKind::Moved {
                    agent: mover, to, ..
                } if mover == actor => Some(to),
                _ => None,
            })
            .chain([agent.location])
            .collect::<BTreeSet<_>>();
        let destinations = self
            .locations
            .keys()
            .copied()
            .filter(|destination| {
                !visited.contains(destination) && self.is_location_open(*destination)
            })
            .collect::<Vec<_>>();
        if !destinations.is_empty() {
            let destination = destinations[self.goal_choice(actor, 2, destinations.len())];
            candidates.push((
                (agent.personality.openness + agent.personality.impulsiveness) / 2.0,
                Goal::new(
                    format!("Visit {}", self.locations[&destination].name),
                    GoalKind::Exploration,
                    GoalTarget::Visit { destination },
                    1,
                    expires_at,
                ),
            ));
        }

        let marketplace =
            self.locations
                .values()
                .filter(|location| {
                    location.business.is_some_and(|business| {
                        business.stock > 0 && agent.balance >= business.price
                    }) && self.is_location_open(location.id)
                        && self.next_route_step(agent.location, location.id).is_ok()
                })
                .collect::<Vec<_>>();
        for location in marketplace {
            let business = location.business.expect("marketplace");
            let need = match business.offering {
                Offering::Meal => 1.0 - agent.needs.food,
                Offering::Supplies => (1.0 - agent.needs.safety) * 0.8,
                Offering::Repairs => 1.0 - agent.needs.safety,
                Offering::Medicine => {
                    if agent.injury || agent.disease.is_symptomatic() {
                        1.0 - agent.health
                    } else {
                        -1.0
                    }
                }
                Offering::CivicServices => 1.0 - agent.needs.status,
            };
            candidates.push((
                need - business.price as f32 / 100.0,
                Goal::new(
                    format!("Buy {} at {}", business.offering, location.name),
                    GoalKind::Wellbeing,
                    GoalTarget::Purchase {
                        location: location.id,
                    },
                    1,
                    expires_at,
                ),
            ));
        }
        candidates.push((
            1.0 - agent.needs.energy + agent.personality.neuroticism * 0.2,
            Goal::new(
                "Get some rest at home",
                GoalKind::Wellbeing,
                GoalTarget::Rest { home: agent.home },
                1,
                expires_at,
            ),
        ));
        candidates.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left.description.cmp(&right.description))
        });
        candidates
    }

    fn goal_choice(&self, actor: AgentId, salt: u128, len: usize) -> usize {
        let changing = u128::from(self.tick.0 / Tick::PER_DAY)
            .wrapping_add(self.agents[&actor].memories.len() as u128)
            .wrapping_add(salt);
        (actor.0.as_u128().wrapping_add(changing) % len as u128) as usize
    }

    fn goal_target_is_available(&self, target: &GoalTarget) -> bool {
        match target {
            GoalTarget::Visit { destination } => self.is_location_open(*destination),
            GoalTarget::Purchase { location } => {
                self.locations[location]
                    .business
                    .is_some_and(|business| business.stock > 0)
                    && self.is_location_open(*location)
            }
            GoalTarget::Work { workplace } => {
                self.locations[workplace]
                    .business
                    .is_some_and(Business::solvent)
                    && self.is_location_open(*workplace)
            }
            _ => true,
        }
    }

    fn goal_target_is_valid(&self, actor: AgentId, target: &GoalTarget) -> bool {
        let agent = &self.agents[&actor];
        match target {
            GoalTarget::Work { workplace } => agent.workplace == Some(*workplace),
            GoalTarget::Talk { resident } => {
                *resident != actor && self.agents.get(resident).is_some_and(Agent::is_alive)
            }
            GoalTarget::Visit { destination } => {
                *destination != agent.location && self.locations.contains_key(destination)
            }
            GoalTarget::Purchase { location } => self
                .locations
                .get(location)
                .is_some_and(|location| location.business.is_some()),
            GoalTarget::Rest { home } => *home == agent.home,
        }
    }

    fn advance_goal(&mut self, actor: AgentId, event: &EventKind) -> Option<String> {
        let location = self.agents.get(&actor)?.location;
        let position =
            self.agents
                .get(&actor)?
                .goals
                .iter()
                .position(|goal| match (&goal.target, event) {
                    (GoalTarget::Work { workplace }, EventKind::Worked { agent, .. }) => {
                        *agent == actor && *workplace == location
                    }
                    (
                        GoalTarget::Talk { resident },
                        EventKind::Spoke {
                            speaker, listener, ..
                        },
                    ) => *speaker == actor && *resident == *listener,
                    (GoalTarget::Visit { destination }, EventKind::Moved { agent, to, .. }) => {
                        *agent == actor && *destination == *to
                    }
                    (
                        GoalTarget::Purchase { location: target },
                        EventKind::Purchased { agent, .. },
                    ) => *agent == actor && *target == location,
                    (GoalTarget::Rest { home }, EventKind::Rested { agent }) => {
                        *agent == actor && *home == location
                    }
                    _ => false,
                })?;
        let goal = &mut self.agents.get_mut(&actor)?.goals[position];
        goal.progress = goal.progress.saturating_add(1).min(goal.required);
        if goal.progress < goal.required {
            return None;
        }
        Some(
            self.agents
                .get_mut(&actor)?
                .goals
                .remove(position)
                .description,
        )
    }

    fn apply_action_effects(&mut self, actor: AgentId, kind: &EventKind) {
        let festival_bonus = if self
            .active_town_event
            .is_some_and(|event| event.kind == TownEventKind::Festival)
        {
            1.5
        } else {
            1.0
        };
        match kind {
            EventKind::Moved { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.needs.energy = (agent.needs.energy - 0.01).max(0.0);
                }
            }
            EventKind::Spoke { listener, tone, .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.companionship, 0.12 * festival_bonus);
                    Needs::restore(&mut agent.needs.status, 0.01 * festival_bonus);
                }
                if let Some(agent) = self.agents.get_mut(listener) {
                    Needs::restore(&mut agent.needs.companionship, 0.08 * festival_bonus);
                }
                self.apply_dialogue_relationship(actor, *listener, *tone, 1.0);
                self.apply_dialogue_relationship(*listener, actor, *tone, 0.75);
            }
            EventKind::Observed { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.safety, 0.03);
                }
            }
            EventKind::Purchased { offering, cost, .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.balance -= *cost;
                    if let Some(item) = offering.item() {
                        agent.inventory.add(item);
                    } else {
                        Needs::restore(&mut agent.needs.status, 0.15);
                        Needs::restore(&mut agent.needs.companionship, 0.03);
                    }
                }
                let business = self
                    .locations
                    .get_mut(&self.agents[&actor].location)
                    .and_then(|location| location.business.as_mut())
                    .expect("validated business");
                business.cash += *cost;
                business.stock -= 1;
                business.revenue += *cost;
            }
            EventKind::ItemUsed { item, .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.inventory.remove(*item);
                    match item {
                        Item::Meal => {
                            Needs::restore(&mut agent.needs.food, 0.35);
                            Needs::restore(&mut agent.needs.energy, 0.02);
                            agent.health = (agent.health + 0.01).min(1.0);
                        }
                        Item::Supplies => {
                            Needs::restore(&mut agent.needs.safety, 0.2);
                            agent.health = (agent.health + 0.03).min(1.0);
                        }
                        Item::RepairKit => {
                            Needs::restore(&mut agent.needs.safety, 0.35);
                            agent.health = (agent.health + 0.15).min(1.0);
                            agent.injury = false;
                        }
                        Item::Medicine => {
                            agent.health = (agent.health + 0.25).min(1.0);
                            agent.injury = false;
                            if agent.disease.is_symptomatic() {
                                agent.disease = DiseaseState::Recovering {
                                    until: Tick(self.tick.0.saturating_add(RECOVERY_TICKS)),
                                };
                            }
                        }
                    }
                }
            }
            EventKind::Treated { cost, .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.balance -= *cost;
                    agent.health = (agent.health + 0.55).min(1.0);
                    agent.injury = false;
                    if agent.disease.is_symptomatic() {
                        agent.disease = DiseaseState::Recovering {
                            until: Tick(self.tick.0.saturating_add(RECOVERY_TICKS)),
                        };
                    }
                }
                let business = self
                    .locations
                    .get_mut(&self.agents[&actor].location)
                    .and_then(|location| location.business.as_mut())
                    .expect("validated clinic");
                business.cash += *cost;
                business.stock -= 1;
                business.revenue += *cost;
            }
            EventKind::Rested { .. } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    Needs::restore(&mut agent.needs.energy, 0.25);
                    Needs::restore(&mut agent.needs.safety, 0.05);
                    agent.health = (agent.health + 0.03).min(1.0);
                    if agent.health > 0.5 {
                        agent.injury = false;
                    }
                }
            }
            EventKind::Worked {
                wage,
                stock_produced,
                ..
            } => {
                if let Some(agent) = self.agents.get_mut(&actor) {
                    agent.balance += *wage;
                    Needs::restore(&mut agent.needs.money, 0.12);
                    Needs::restore(&mut agent.needs.status, 0.05);
                    agent.needs.energy = (agent.needs.energy - 0.03).max(0.0);
                    agent.needs.food = (agent.needs.food - 0.02).max(0.0);
                }
                let business = self
                    .locations
                    .get_mut(&self.agents[&actor].location)
                    .and_then(|location| location.business.as_mut())
                    .expect("validated employer");
                business.cash -= *wage;
                business.stock += *stock_produced;
                business.wages_paid += *wage;
            }
            EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::Confronted { .. }
            | EventKind::GoalCompleted { .. }
            | EventKind::Waited { .. }
            | EventKind::Died { .. }
            | EventKind::DiseaseInfected { .. }
            | EventKind::DiseaseSymptoms { .. }
            | EventKind::DiseaseRecovered { .. }
            | EventKind::DiseaseImmunityExpired { .. }
            | EventKind::ActionRejected { .. } => {}
        }
    }

    fn apply_dialogue_relationship(
        &mut self,
        source: AgentId,
        target: AgentId,
        tone: DialogueTone,
        amount: f32,
    ) {
        if let Some(agent) = self.agents.get_mut(&source) {
            let warmth = 0.75 + 0.5 * agent.personality.agreeableness;
            let credibility = 0.75 + 0.5 * agent.personality.honesty;
            let (affection, trust, respect, suspicion) = match tone {
                DialogueTone::Friendly => (0.03, 0.012, 0.006, -0.01),
                DialogueTone::Supportive => (0.02, 0.025, 0.012, -0.02),
                DialogueTone::Neutral => (0.02, 0.015, 0.005, -0.01),
                DialogueTone::Tense => (-0.025, -0.02, -0.01, 0.025),
            };
            let relationship = agent
                .relationships
                .entry(target)
                .or_insert(Relationship::NEUTRAL);
            relationship.affection =
                (relationship.affection + affection * amount * warmth).clamp(-1.0, 1.0);
            relationship.trust =
                (relationship.trust + trust * amount * credibility).clamp(-1.0, 1.0);
            relationship.respect = (relationship.respect + respect * amount).clamp(-1.0, 1.0);
            relationship.suspicion =
                (relationship.suspicion + suspicion * amount * credibility).clamp(-1.0, 1.0);
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
        self.apply_mood_effects(&event.kind);
        self.events.push(event.clone());
        self.remember(&event);
        event
    }

    fn apply_mood_effects(&mut self, kind: &EventKind) {
        let mut adjust = |id: AgentId, amount: f32| {
            if let Some(agent) = self.agents.get_mut(&id) {
                agent.mood = (agent.mood + amount).clamp(-1.0, 1.0);
            }
        };
        match kind {
            EventKind::Spoke {
                speaker,
                listener,
                tone,
                ..
            } => {
                let (speaker_change, listener_change) = match tone {
                    DialogueTone::Friendly => (0.05, 0.04),
                    DialogueTone::Supportive => (0.06, 0.08),
                    DialogueTone::Neutral => (0.01, 0.01),
                    DialogueTone::Tense => (-0.08, -0.06),
                };
                adjust(*speaker, speaker_change);
                adjust(*listener, listener_change);
            }
            EventKind::Purchased { agent, .. } => adjust(*agent, 0.06),
            EventKind::ItemUsed { agent, .. } => adjust(*agent, 0.04),
            EventKind::Treated { agent, .. } => adjust(*agent, 0.1),
            EventKind::Rested { agent } => adjust(*agent, 0.08),
            EventKind::Worked { agent, .. } => adjust(*agent, 0.03),
            EventKind::Confronted {
                accuser,
                target,
                outcome,
                ..
            } => {
                let (accuser_change, target_change) = match outcome {
                    ConfrontationOutcome::Confirmed => (0.04, 0.01),
                    ConfrontationOutcome::Denied => (-0.06, -0.05),
                    ConfrontationOutcome::Challenged => (-0.03, -0.04),
                };
                adjust(*accuser, accuser_change);
                adjust(*target, target_change);
            }
            EventKind::Observed { observer, .. } => adjust(*observer, 0.02),
            EventKind::GoalCompleted { agent, .. } => adjust(*agent, 0.15),
            EventKind::ActionRejected { agent, .. } => adjust(*agent, -0.06),
            EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::Moved { .. }
            | EventKind::Died { .. }
            | EventKind::DiseaseInfected { .. }
            | EventKind::DiseaseSymptoms { .. }
            | EventKind::DiseaseRecovered { .. }
            | EventKind::DiseaseImmunityExpired { .. }
            | EventKind::Waited { .. } => {}
        }
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
            EventKind::Confronted {
                accuser, target, ..
            } => {
                witnesses.extend([*accuser, *target]);
            }
            EventKind::Observed { observer, target } => {
                witnesses.insert(*observer);
                if let ObservationTarget::Agent(agent) = target {
                    witnesses.insert(*agent);
                }
            }
            EventKind::Purchased { agent, .. }
            | EventKind::ItemUsed { agent, .. }
            | EventKind::Treated { agent, .. }
            | EventKind::Rested { agent }
            | EventKind::Worked { agent, .. }
            | EventKind::GoalCompleted { agent, .. }
            | EventKind::Died { agent, .. }
            | EventKind::DiseaseSymptoms { agent }
            | EventKind::DiseaseRecovered { agent }
            | EventKind::DiseaseImmunityExpired { agent } => {
                witnesses.insert(*agent);
            }
            EventKind::DiseaseInfected { .. }
            | EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::Waited { .. }
            | EventKind::ActionRejected { .. } => return,
        }
        if let Some(location) = event.location.and_then(|id| self.locations.get(&id)) {
            witnesses.extend(location.agents.iter().copied());
        }

        let evidence = event_evidence(&event.kind);
        for witness in witnesses {
            if let Some(agent) = self.agents.get_mut(&witness) {
                agent.memories.push(event.clone());
                let excess = agent.memories.len().saturating_sub(MEMORY_LIMIT);
                agent.memories.drain(..excess);
                if let Some((subject, sociability, reliability, hostility)) = evidence
                    && witness != subject
                {
                    agent.learn_about_weighted(subject, sociability, reliability, hostility, 1.0);
                }
            }
        }
    }

    fn stock_per_shift(&self, tick: Tick) -> u32 {
        match TownEvent::scheduled(self.seed, tick).map(|event| event.kind) {
            Some(TownEventKind::Shortage) => STOCK_PER_SHIFT / 2,
            Some(TownEventKind::MarketDay) => STOCK_PER_SHIFT * 2,
            _ => STOCK_PER_SHIFT,
        }
    }

    pub fn validate(&self) -> Result<(), WorldError> {
        if self.active_town_event != TownEvent::scheduled(self.seed, self.tick) {
            return Err(WorldError::InvalidState(
                "active town event does not match the deterministic schedule".into(),
            ));
        }
        for (id, location) in &self.locations {
            if location
                .business
                .is_some_and(|business| business.price == 0)
            {
                return Err(WorldError::InvalidState(format!(
                    "business at {id} has a zero price"
                )));
            }
            if location
                .opening_hours
                .is_some_and(|hours| !hours.is_valid())
            {
                return Err(WorldError::InvalidState(format!(
                    "location {id} has invalid opening hours"
                )));
            }
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
            match agent.life {
                LifeState::Alive if agent.health > 0.0 => {}
                LifeState::Dead { tick, .. } if agent.health == 0.0 && tick <= self.tick => {}
                _ => {
                    return Err(WorldError::InvalidState(format!(
                        "agent {id} has an invalid life state"
                    )));
                }
            }
            if agent.is_alive() && !self.locations.contains_key(&agent.location) {
                return Err(WorldError::UnknownLocation(agent.location));
            }
            if !self.locations.contains_key(&agent.home) {
                return Err(WorldError::UnknownLocation(agent.home));
            }
            if agent.workplace.is_some_and(|workplace| {
                self.locations
                    .get(&workplace)
                    .is_none_or(|location| location.business.is_none())
            }) {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has a workplace without a business ledger"
                )));
            }
            let expected_memberships = usize::from(agent.is_alive());
            if memberships.get(id).copied().unwrap_or_default() != expected_memberships {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} belongs to {} locations",
                    memberships.get(id).copied().unwrap_or_default()
                )));
            }
            let disease_has_valid_until = match agent.disease {
                DiseaseState::Susceptible => true,
                DiseaseState::Incubating { until }
                | DiseaseState::Symptomatic { until }
                | DiseaseState::Recovering { until }
                | DiseaseState::Immune { until } => until > self.tick,
            };
            if !agent.is_alive() && !matches!(agent.disease, DiseaseState::Susceptible) {
                return Err(WorldError::InvalidState(format!(
                    "dead agent {id} retains disease state"
                )));
            }
            if !disease_has_valid_until {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has an expired disease stage"
                )));
            }
            if !agent.personality.is_normalized()
                || !agent.needs.is_normalized()
                || !agent.inventory.is_valid()
                || !agent.health.is_finite()
                || !(0.0..=1.0).contains(&agent.health)
                || !(-1.0..=1.0).contains(&agent.mood)
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has non-normalized traits"
                )));
            }
            if agent.routing.budget_day > self.tick.day()
                || agent
                    .routing
                    .last_llm_attempt
                    .is_some_and(|tick| tick > self.tick || tick.day() != agent.routing.budget_day)
                || (agent.routing.llm_calls_today > 0 && agent.routing.last_llm_attempt.is_none())
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has invalid LLM routing state"
                )));
            }
            if !agent.is_alive() && (agent.activity.is_some() || agent.intention.is_some()) {
                return Err(WorldError::InvalidState(format!(
                    "dead agent {id} retains an activity or intention"
                )));
            }
            if agent
                .activity
                .is_some_and(|activity| activity.until <= self.tick)
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has an expired activity"
                )));
            }
            if let Some(intention) = &agent.intention {
                let target_is_valid = match &intention.goal {
                    IntentionGoal::Visit { destination }
                    | IntentionGoal::Purchase { destination } => {
                        self.locations.contains_key(destination)
                    }
                    IntentionGoal::Rest => self.locations.contains_key(&agent.home),
                    IntentionGoal::SeekTreatment => self.clinic_location().is_some(),
                    IntentionGoal::Work => agent
                        .workplace
                        .is_some_and(|workplace| self.locations.contains_key(&workplace)),
                    IntentionGoal::Talk {
                        target, message, ..
                    } => {
                        target != id
                            && self.agents.get(target).is_some_and(Agent::is_alive)
                            && !message.trim().is_empty()
                            && !message.chars().any(char::is_control)
                            && message.chars().count() <= MAX_TALK_MESSAGE_CHARS
                    }
                };
                if intention.expires_at <= self.tick || !target_is_valid {
                    return Err(WorldError::InvalidState(format!(
                        "agent {id} has an invalid intention"
                    )));
                }
            }
            if agent.relationships.iter().any(|(target, relationship)| {
                target == id || !self.agents.contains_key(target) || !relationship.is_normalized()
            }) {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has an invalid relationship"
                )));
            }
            if agent.beliefs.iter().any(|(subject, belief)| {
                subject == id || !self.agents.contains_key(subject) || !belief.is_normalized()
            }) {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has an invalid belief"
                )));
            }
            let mut goal_targets = BTreeSet::new();
            if agent.goals.len() > GOAL_LIMIT
                || agent.goals.iter().any(|goal| {
                    goal.description.trim().is_empty()
                        || goal.required == 0
                        || goal.progress >= goal.required
                        || goal.expires_at <= self.tick
                        || !goal_targets.insert(goal.target)
                        || !self.goal_target_is_valid(*id, &goal.target)
                })
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has invalid goals"
                )));
            }
            if agent.memories.len() > MEMORY_LIMIT {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has too many memories"
                )));
            }
            let mut known_ids = agent
                .memories
                .iter()
                .map(|event| event.id)
                .collect::<BTreeSet<_>>();
            if agent.rumors.len() > RUMOR_LIMIT
                || agent.rumors.iter().any(|rumor| {
                    !known_ids.insert(rumor.event.id)
                        || rumor.source == *id
                        || !self.agents.contains_key(&rumor.source)
                        || rumor.depth == 0
                        || !(0.0..=1.0).contains(&rumor.confidence)
                })
            {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} has invalid rumors"
                )));
            }
        }
        Ok(())
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
mod tests {
    use super::{
        ActionRejection, ActionResult, Activity, AgentId, Business, ConfrontationOutcome,
        DeathCause, DialogueTone, DiseaseState, EventId, EventKind, GOAL_LIMIT, Goal, GoalKind,
        GoalTarget, IMMUNITY_TICKS, INCUBATION_TICKS, Intention, IntentionGoal, Item, LifeState,
        MAX_TALK_MESSAGE_CHARS, ObservationTarget, Offering, PATIENT_ZERO_TICK, ProposedAction,
        RECOVERY_TICKS, Relationship, SYMPTOMATIC_TICKS, Tick, TownEvent, TownEventKind, World,
        WorldError,
    };
    use crate::sim::{
        ActivityKind, BUSINESS_STARTING_CASH, MAX_ITEMS_PER_KIND, STOCK_PER_SHIFT, Scheduler,
        WORK_WAGE,
    };
    use std::collections::BTreeSet;
    use uuid::Uuid;

    #[test]
    fn briar_glen_has_consistent_residents() {
        let world = World::briar_glen(814_921).expect("town should construct");
        assert_eq!(world.agents.len(), 8);
        assert_eq!(world.locations.len(), 9);
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
    fn town_events_start_end_and_change_conditions() {
        let mut storm = World::briar_glen(0).expect("town");
        let home = storm.agents.values().next().expect("resident").home;
        let non_home = storm
            .locations
            .keys()
            .copied()
            .find(|location| !storm.agents.values().any(|agent| agent.home == *location))
            .expect("non-home location");
        let safety = storm.agents.values().next().expect("resident").needs.safety;
        storm
            .advance_to(Tick(8 * 60 / Tick::MINUTES))
            .expect("storm starts");
        assert_eq!(
            storm.active_town_event.expect("active").kind,
            TownEventKind::Storm
        );
        assert!(storm.is_location_open(home));
        assert!(!storm.is_location_open(non_home));
        assert!(safety - storm.agents.values().next().expect("resident").needs.safety > 0.001);
        assert!(matches!(
            storm.events().last().expect("start").kind,
            EventKind::TownEventStarted {
                kind: TownEventKind::Storm,
                ..
            }
        ));

        let ends_at = storm.active_town_event.expect("active").ends_at;
        storm.advance_to(ends_at).expect("storm ends");
        assert_eq!(storm.active_town_event, None);
        assert!(matches!(
            storm.events().last().expect("end").kind,
            EventKind::TownEventEnded {
                kind: TownEventKind::Storm
            }
        ));

        storm.active_town_event = Some(TownEvent {
            kind: TownEventKind::Festival,
            starts_at: storm.tick,
            ends_at: Tick(storm.tick.0 + 1),
        });
        assert!(matches!(storm.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn festivals_and_market_conditions_modify_existing_actions() {
        let mut festival = World::briar_glen(1).expect("town");
        let residents = festival.agents.keys().copied().take(2).collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[1];
        let home = festival.agents[&speaker].home;
        festival.relocate(speaker, home);
        festival.relocate(listener, home);
        festival
            .advance_to(Tick(9 * 60 / Tick::MINUTES))
            .expect("festival starts");
        festival
            .agents
            .get_mut(&speaker)
            .expect("speaker")
            .needs
            .companionship = 0.0;
        festival
            .agents
            .get_mut(&speaker)
            .expect("speaker")
            .needs
            .status = 0.0;
        festival.execute(
            speaker,
            ProposedAction::Talk {
                target: listener,
                tone: DialogueTone::Neutral,
                message: "Enjoy the festival!".into(),
            },
        );
        assert!(festival.agents[&speaker].needs.companionship > 0.17);
        assert!(festival.agents[&speaker].needs.status >= 0.015);

        for (seed, expected_kind, expected_stock) in [
            (2, TownEventKind::Shortage, STOCK_PER_SHIFT / 2),
            (3, TownEventKind::MarketDay, STOCK_PER_SHIFT * 2),
        ] {
            let mut world = World::briar_glen(seed).expect("town");
            let (worker, workplace) = world
                .agents
                .iter()
                .find_map(|(id, agent)| agent.workplace.map(|workplace| (*id, workplace)))
                .expect("worker");
            world.relocate(worker, workplace);
            world
                .advance_to(Tick((8 + seed) * 60 / Tick::MINUTES))
                .expect("event starts");
            assert_eq!(world.active_town_event.expect("active").kind, expected_kind);
            assert!(matches!(
                world.execute(worker, ProposedAction::Work),
                ActionResult::Success(ref events)
                    if matches!(events[0].kind, EventKind::Worked { stock_produced, .. } if stock_produced == expected_stock)
            ));
        }
    }

    #[test]
    fn goals_are_contextual_and_seeded() {
        let first = World::briar_glen(11).expect("town");
        let repeated = World::briar_glen(11).expect("same town");
        let different = World::briar_glen(12).expect("different town");
        let goals = |world: &World| {
            world
                .agents
                .values()
                .map(|agent| agent.goals.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(goals(&first), goals(&repeated));
        assert_ne!(goals(&first), goals(&different));
        assert!(first.agents.values().all(|agent| {
            !agent.goals.is_empty()
                && agent.goals.len() <= GOAL_LIMIT
                && agent
                    .goals
                    .iter()
                    .all(|goal| goal.expires_at == Tick(first.tick.0 + Tick::PER_DAY))
        }));
        let descriptions = first
            .agents
            .values()
            .flat_map(|agent| agent.goals.iter().map(|goal| goal.description.as_str()))
            .collect::<BTreeSet<_>>();
        assert!(descriptions.len() > GOAL_LIMIT);
    }

    #[test]
    fn tick_only_moves_forward() {
        let mut world = World::briar_glen(1).expect("town should construct");
        let start = world.tick;
        world.advance_to(Tick(start.0 + 2)).expect("forward tick");
        assert_eq!(
            world.advance_to(Tick(start.0 + 1)),
            Err(WorldError::NonMonotonicTick {
                current: Tick(start.0 + 2),
                proposed: Tick(start.0 + 1),
            })
        );
    }

    #[test]
    fn activities_last_until_completion_and_urgent_needs_interrupt_them() {
        let mut world = World::briar_glen(2).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let start = world.tick;
        let food_business = world
            .locations
            .values()
            .find(|location| location.business.is_some() && location.is_open(world.tick.hour()))
            .expect("open food business")
            .id;
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 1.0;
        agent.needs.safety = 1.0;

        assert!(matches!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Rejected(ActionRejection::CannotWorkHere(_))
        ));
        assert_eq!(world.agents[&actor].activity, None);

        world.relocate(actor, food_business);
        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(_)
        ));
        assert_eq!(
            world.agents[&actor].activity,
            Some(Activity {
                kind: ActivityKind::Shopping,
                until: Tick(start.0 + 3),
            })
        );
        world.advance_to(Tick(start.0 + 2)).expect("activity time");
        assert!(world.agents[&actor].activity.is_some());
        world.advance_to(Tick(start.0 + 3)).expect("completion");
        assert_eq!(world.agents[&actor].activity, None);

        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.activity = Some(Activity {
            kind: ActivityKind::Working,
            until: Tick(start.0 + 15),
        });
        agent.needs.food = 0.05;
        world.advance_tick().expect("interruption");
        assert_eq!(world.agents[&actor].activity, None);
    }

    #[test]
    fn time_and_successful_actions_update_needs() {
        let mut world = World::briar_glen(2).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let actor = residents[0];
        let listener = residents[1];
        let food_business = world
            .locations
            .values()
            .find(|location| location.business.is_some() && location.is_open(world.tick.hour()))
            .expect("open food business")
            .id;
        world.relocate(actor, food_business);
        world.relocate(listener, food_business);
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
                tone: DialogueTone::Neutral,
                message: "Hello.".into(),
            },
        );
        assert!(world.agents[&actor].needs.companionship > companionship);

        let needs = world.agents[&actor].needs.clone();
        world.execute(actor, ProposedAction::Purchase);
        assert_eq!(world.agents[&actor].needs, needs);
        assert_eq!(world.agents[&actor].inventory.meals, 1);
        world.execute(actor, ProposedAction::ConsumeMeal);
        assert!(world.agents[&actor].needs.food > needs.food);
        assert_eq!(world.agents[&actor].inventory.meals, 0);
        let energy = world.agents[&actor].needs.energy;
        let safety = world.agents[&actor].needs.safety;
        let home = world.agents[&actor].home;
        world.relocate(actor, home);
        world.execute(actor, ProposedAction::Rest);
        assert!(world.agents[&actor].needs.energy > energy);
        assert!(world.agents[&actor].needs.safety > safety);
        assert!(world.agents[&listener].memories.iter().any(
            |event| matches!(event.kind, EventKind::Purchased { agent, .. } if agent == actor)
        ));
        assert!(world.agents[&listener].memories.iter().any(
            |event| matches!(event.kind, EventKind::ItemUsed { agent, item: Item::Meal } if agent == actor)
        ));
        world.validate().expect("normalized needs");
    }

    #[test]
    fn work_and_purchases_transfer_coins_and_reject_atomically() {
        let mut world = World::briar_glen(12).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let business = world.agents[&actor].workplace.expect("workplace");
        assert!(world.locations[&business].business.is_some());
        world.relocate(actor, business);

        let balance = world.agents[&actor].balance;
        let initial_stock = world.locations[&business].business.expect("business").stock;
        assert!(matches!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Worked {
                    wage: WORK_WAGE,
                    stock_produced: STOCK_PER_SHIFT,
                    ..
                })
        ));
        assert_eq!(world.agents[&actor].balance, balance + WORK_WAGE);
        assert_eq!(
            world.locations[&business].business.expect("business").stock,
            initial_stock + STOCK_PER_SHIFT
        );

        let stock = world.locations[&business].business.expect("business").stock;
        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Purchased { cost: 5, .. })
        ));
        assert_eq!(world.agents[&actor].balance, balance + WORK_WAGE - 5);
        assert_eq!(
            world.locations[&business].business.expect("business"),
            Business {
                offering: Offering::Meal,
                price: 5,
                cash: BUSINESS_STARTING_CASH - WORK_WAGE + 5,
                stock: stock - 1,
                revenue: 5,
                wages_paid: WORK_WAGE,
            }
        );

        world.agents.get_mut(&actor).expect("resident").balance = 0;
        let before = world.clone();
        assert_eq!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Rejected(ActionRejection::InsufficientFunds {
                cost: 5,
                available: 0,
            })
        );
        assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
        assert_eq!(world.locations[&business], before.locations[&business]);

        world.agents.get_mut(&actor).expect("resident").balance = 5;
        world
            .locations
            .get_mut(&business)
            .expect("business")
            .business
            .as_mut()
            .expect("ledger")
            .stock = 0;
        let before = world.clone();
        assert_eq!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Rejected(ActionRejection::SoldOut(business))
        );
        assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
        assert_eq!(world.locations[&business], before.locations[&business]);

        let ledger = world
            .locations
            .get_mut(&business)
            .expect("business")
            .business
            .as_mut()
            .expect("ledger");
        ledger.cash = WORK_WAGE - 1;
        let before = world.clone();
        assert_eq!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Rejected(ActionRejection::InsolventEmployer {
                location: business,
                wage: WORK_WAGE,
                available: WORK_WAGE - 1,
            })
        );
        assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
        assert_eq!(world.locations[&business], before.locations[&business]);

        let ledger = world
            .locations
            .get_mut(&business)
            .expect("business")
            .business
            .as_mut()
            .expect("ledger");
        ledger.cash = u64::MAX;
        ledger.stock = 1;
        let before = world.clone();
        assert_eq!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Rejected(ActionRejection::EconomyOverflow)
        );
        assert_eq!(world.agents[&actor].balance, before.agents[&actor].balance);
        assert_eq!(world.locations[&business], before.locations[&business]);
    }

    #[test]
    fn inventory_capacity_and_missing_items_reject_atomically() {
        let mut world = World::briar_glen(22).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let meal_business = world
            .locations
            .values()
            .find(|location| {
                location
                    .business
                    .is_some_and(|business| business.offering == Offering::Meal)
                    && location.is_open(world.tick.hour())
            })
            .expect("open meal business")
            .id;
        world.relocate(actor, meal_business);

        assert_eq!(
            world.execute(actor, ProposedAction::ConsumeMeal),
            ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Meal))
        );
        assert_eq!(
            world.execute(actor, ProposedAction::UseSupplies),
            ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Supplies))
        );
        assert_eq!(
            world.execute(actor, ProposedAction::UseRepairKit),
            ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::RepairKit))
        );

        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .inventory
            .meals = MAX_ITEMS_PER_KIND;
        let before_agent = world.agents[&actor].clone();
        let before_location = world.locations[&meal_business].clone();
        assert_eq!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Rejected(ActionRejection::InventoryFull(Item::Meal))
        );
        assert_eq!(world.agents[&actor].balance, before_agent.balance);
        assert_eq!(world.agents[&actor].inventory, before_agent.inventory);
        assert_eq!(world.locations[&meal_business], before_location);

        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .inventory
            .meals = MAX_ITEMS_PER_KIND + 1;
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn clinic_sells_medicine_and_provides_paid_treatment() {
        let mut world = World::briar_glen(31).expect("town");
        world.advance_to(Tick(8 * 12)).expect("clinic opening");
        let clinic = world.clinic_location().expect("one clinic");
        let actor = *world.agents.keys().next().expect("resident");
        world.relocate(actor, clinic);
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.balance = 100;
        agent.health = 0.3;
        agent.injury = true;

        let business_before = world.locations[&clinic].business.expect("clinic business");
        assert_eq!(business_before.offering, Offering::Medicine);
        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(_)
        ));
        assert_eq!(world.agents[&actor].inventory.medicine, 1);
        assert!(matches!(
            world.execute(actor, ProposedAction::UseMedicine),
            ActionResult::Success(_)
        ));
        assert_eq!(world.agents[&actor].inventory.medicine, 0);
        assert!(world.agents[&actor].health > 0.3);
        assert!(!world.agents[&actor].injury);

        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(_)
        ));
        let recovery_until = Tick(world.tick.0 + RECOVERY_TICKS);
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.health = 0.2;
        agent.disease = DiseaseState::Symptomatic {
            until: Tick(world.tick.0 + SYMPTOMATIC_TICKS),
        };
        assert!(matches!(
            world.execute(actor, ProposedAction::UseMedicine),
            ActionResult::Success(ref events)
                if events.iter().any(|event| matches!(
                    event.kind,
                    EventKind::DiseaseRecovered { agent } if agent == actor
                ))
        ));
        assert_eq!(
            world.agents[&actor].disease,
            DiseaseState::Recovering {
                until: recovery_until
            }
        );

        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.health = 0.2;
        agent.injury = true;
        let balance_before = agent.balance;
        let stock_before = world.locations[&clinic].business.expect("clinic").stock;
        assert!(matches!(
            world.execute(actor, ProposedAction::SeekTreatment),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Treated { cost, .. } if cost == business_before.price)
        ));
        assert_eq!(
            world.agents[&actor].balance,
            balance_before - business_before.price
        );
        assert_eq!(
            world.locations[&clinic].business.expect("clinic").stock,
            stock_before - 1
        );
        assert!(world.agents[&actor].health > 0.2);
        assert!(!world.agents[&actor].injury);
        world.validate().expect("valid treated world");
    }

    #[test]
    fn treatment_requires_the_clinic_and_a_medical_need() {
        let mut world = World::briar_glen(32).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        world.agents.get_mut(&actor).expect("resident").balance = 100;
        assert_eq!(
            world.execute(actor, ProposedAction::SeekTreatment),
            ActionResult::Rejected(ActionRejection::CannotSeekTreatmentHere(
                world.agents[&actor].location
            ))
        );
        let clinic = world.clinic_location().expect("clinic");
        world.relocate(actor, clinic);
        assert_eq!(
            world.execute(actor, ProposedAction::SeekTreatment),
            ActionResult::Rejected(ActionRejection::NoMedicalNeed)
        );
        assert_eq!(
            world.execute(actor, ProposedAction::UseMedicine),
            ActionResult::Rejected(ActionRejection::ItemUnavailable(Item::Medicine))
        );
    }

    #[test]
    fn every_market_offering_transfers_value_and_work_restocks_it() {
        for offering in [
            Offering::Meal,
            Offering::Supplies,
            Offering::Repairs,
            Offering::Medicine,
            Offering::CivicServices,
        ] {
            let mut world = World::briar_glen(21).expect("town");
            world.advance_to(Tick(8 * 12)).expect("business hours");
            let location = world
                .locations
                .values()
                .find(|location| {
                    location.is_open(world.tick.hour())
                        && location
                            .business
                            .is_some_and(|business| business.offering == offering)
                })
                .map(|location| location.id)
                .expect("open offering");
            let actor = world
                .agents
                .values()
                .find(|agent| agent.workplace == Some(location))
                .map(|agent| agent.id)
                .expect("worker");
            world.relocate(actor, location);
            let agent = world.agents.get_mut(&actor).expect("resident");
            agent.balance = 100;
            agent.needs.food = 0.1;
            agent.needs.safety = 0.1;
            agent.needs.status = 0.1;
            agent.needs.companionship = 0.1;
            if offering == Offering::Medicine {
                agent.health = 0.4;
                agent.injury = true;
            }
            let before_health = agent.health;
            let before_needs = agent.needs.clone();
            let before = world.locations[&location].business.expect("business");

            assert!(matches!(
                world.execute(actor, ProposedAction::Purchase),
                ActionResult::Success(ref events)
                    if matches!(events[0].kind, EventKind::Purchased {
                        offering: actual,
                        cost,
                        ..
                    } if actual == offering && cost == before.price)
            ));
            let after_purchase = world.locations[&location].business.expect("business");
            assert_eq!(world.agents[&actor].balance, 100 - before.price);
            assert_eq!(after_purchase.cash, before.cash + before.price);
            assert_eq!(after_purchase.revenue, before.revenue + before.price);
            assert_eq!(after_purchase.stock, before.stock - 1);
            match offering {
                Offering::Meal => {
                    assert_eq!(world.agents[&actor].inventory.meals, 1);
                    world.execute(actor, ProposedAction::ConsumeMeal);
                    assert!(world.agents[&actor].needs.food > before_needs.food);
                }
                Offering::Supplies => {
                    assert_eq!(world.agents[&actor].inventory.supplies, 1);
                    world.execute(actor, ProposedAction::UseSupplies);
                    assert!(world.agents[&actor].needs.safety > before_needs.safety);
                }
                Offering::Repairs => {
                    assert_eq!(world.agents[&actor].inventory.repair_kits, 1);
                    world.execute(actor, ProposedAction::UseRepairKit);
                    assert!(world.agents[&actor].needs.safety > before_needs.safety);
                }
                Offering::Medicine => {
                    assert_eq!(world.agents[&actor].inventory.medicine, 1);
                    assert!(matches!(
                        world.execute(actor, ProposedAction::UseMedicine),
                        ActionResult::Success(_)
                    ));
                    assert!(world.agents[&actor].health > before_health);
                    assert!(!world.agents[&actor].injury);
                }
                Offering::CivicServices => {
                    assert!(world.agents[&actor].needs.status > before_needs.status);
                    assert!(world.agents[&actor].needs.companionship > before_needs.companionship);
                }
            }

            assert!(matches!(
                world.execute(actor, ProposedAction::Work),
                ActionResult::Success(ref events)
                    if matches!(events[0].kind, EventKind::Worked {
                        stock_produced: STOCK_PER_SHIFT,
                        ..
                    })
            ));
            assert_eq!(
                world.locations[&location]
                    .business
                    .expect("restocked")
                    .stock,
                after_purchase.stock + STOCK_PER_SHIFT
            );
        }
    }

    #[test]
    fn events_change_mood_and_time_returns_it_toward_neutral() {
        let mut world = World::briar_glen(4).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let location = world.agents[&actor].location;

        for _ in 0..4 {
            world.execute(
                actor,
                ProposedAction::Observe {
                    target: ObservationTarget::Location(location),
                },
            );
        }
        assert!((world.agents[&actor].mood - 0.08).abs() < f32::EPSILON * 4.0);

        world.execute(
            actor,
            ProposedAction::Talk {
                target: actor,
                tone: DialogueTone::Neutral,
                message: "Hello, me.".into(),
            },
        );
        assert!((world.agents[&actor].mood - 0.02).abs() < f32::EPSILON * 4.0);

        world
            .advance_to(Tick(world.tick.0 + 10))
            .expect("time advances");
        assert_eq!(world.agents[&actor].mood, 0.0);
        world.validate().expect("bounded mood");
        world.agents.get_mut(&actor).expect("actor").mood = 1.1;
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn contextual_goals_match_exact_targets_and_refresh() {
        let mut world = World::briar_glen(4).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let actor = residents[0];
        let listener = residents[1];
        let other = residents[2];
        let expires_at = Tick(world.tick.0 + Tick::PER_DAY);
        world.agents.get_mut(&actor).expect("actor").goals = vec![Goal::new(
            "Speak twice with the intended resident",
            GoalKind::Community,
            GoalTarget::Talk { resident: listener },
            2,
            expires_at,
        )];

        world.execute(
            actor,
            ProposedAction::Talk {
                target: other,
                tone: DialogueTone::Neutral,
                message: "Hello.".into(),
            },
        );
        assert_eq!(world.agents[&actor].goals[0].progress, 0);

        for _ in 0..2 {
            world.execute(
                actor,
                ProposedAction::Talk {
                    target: listener,
                    tone: DialogueTone::Neutral,
                    message: "Hello.".into(),
                },
            );
        }
        assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::GoalCompleted { agent, .. } if agent == actor))
                .count(),
            1
        );
        assert_eq!(world.agents[&actor].goals.len(), GOAL_LIMIT);
        assert!(
            world.agents[&actor]
                .goals
                .iter()
                .all(|goal| goal.description != "Speak twice with the intended resident")
        );
        assert!(world.agents[&listener].memories.iter().any(
            |event| matches!(event.kind, EventKind::GoalCompleted { agent, .. } if agent == actor)
        ));

        let home = world.agents[&actor].home;
        world.agents.get_mut(&actor).expect("actor").goals = vec![Goal::new(
            "Expiring goal",
            GoalKind::Exploration,
            GoalTarget::Visit { destination: home },
            1,
            Tick(world.tick.0 + 1),
        )];
        world
            .advance_to(Tick(world.tick.0 + 1))
            .expect("goal expiry");
        assert!(
            world.agents[&actor]
                .goals
                .iter()
                .all(|goal| goal.description != "Expiring goal")
        );

        world.agents.get_mut(&actor).expect("actor").goals[0].progress = 1;
        world.agents.get_mut(&actor).expect("actor").goals[0].required = 1;
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn multi_hop_intentions_continue_and_clear() {
        let mut world = World::briar_glen(3).expect("town");
        world.advance_to(Tick(12 * 12)).expect("noon");
        let actor = *world.agents.keys().next().expect("resident");
        let destination = world
            .locations
            .values()
            .find(|location| location.name == "Town Hall")
            .expect("town hall")
            .id;
        assert!(
            !world.locations[&world.agents[&actor].location]
                .connected
                .contains(&destination)
        );

        assert!(matches!(
            world.execute(
                actor,
                ProposedAction::Pursue {
                    intention: IntentionGoal::Visit { destination },
                },
            ),
            ActionResult::Success(_)
        ));
        assert!(world.agents[&actor].intention.is_some());

        while world.agents[&actor].intention.is_some() {
            let until = world.agents[&actor]
                .activity
                .expect("travel activity")
                .until;
            world.advance_to(until).expect("finish route step");
            world.continue_intention(actor);
        }

        assert_eq!(world.agents[&actor].location, destination);
        assert_eq!(
            world
                .events()
                .iter()
                .filter(
                    |event| matches!(event.kind, EventKind::Moved { agent, .. } if agent == actor)
                )
                .count(),
            2
        );
        world.validate().expect("valid completed intention");
    }

    #[test]
    fn invalid_and_expired_intentions_clear_safely() {
        let mut world = World::briar_glen(3).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let unknown = crate::sim::LocationId(Uuid::nil());
        assert!(matches!(
            world.execute(
                actor,
                ProposedAction::Pursue {
                    intention: IntentionGoal::Visit {
                        destination: unknown,
                    },
                },
            ),
            ActionResult::Rejected(ActionRejection::UnknownLocation(id)) if id == unknown
        ));
        assert_eq!(world.agents[&actor].intention, None);

        world.agents.get_mut(&actor).expect("resident").intention = Some(Intention {
            goal: IntentionGoal::Rest,
            expires_at: world.tick,
        });
        assert_eq!(world.continue_intention(actor), None);
        assert_eq!(world.agents[&actor].intention, None);
    }

    #[test]
    fn work_and_activity_locations_are_authoritative() {
        let mut world = World::briar_glen(3).expect("town");
        let actor = *world
            .agents
            .values()
            .find(|agent| agent.workplace.is_some())
            .map(|agent| &agent.id)
            .expect("worker");
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
        assert!(matches!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Success(_)
        ));
        assert_eq!(
            world.execute(actor, ProposedAction::Rest),
            ActionResult::Rejected(ActionRejection::CannotRestHere(workplace))
        );

        world.advance_to(Tick(18 * 12)).expect("evening");
        assert_eq!(
            world.execute(actor, ProposedAction::Work),
            ActionResult::Rejected(ActionRejection::LocationClosed(workplace))
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
    fn closed_locations_reject_entry_and_activity_but_allow_departure() {
        let mut world = World::briar_glen(5).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let home = world.agents[&actor].home;
        let tavern = world
            .locations
            .values()
            .find(|location| location.name == "The Crooked Lantern")
            .map(|location| location.id)
            .expect("tavern");

        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Move {
                    destination: tavern,
                }
            ),
            ActionResult::Rejected(ActionRejection::LocationClosed(tavern))
        );

        world.advance_to(Tick(12 * 12)).expect("opening time");
        assert!(matches!(
            world.execute(
                actor,
                ProposedAction::Move {
                    destination: tavern,
                }
            ),
            ActionResult::Success(_)
        ));
        world.advance_to(Tick(23 * 12)).expect("closing time");
        assert_eq!(
            world.execute(actor, ProposedAction::Purchase),
            ActionResult::Rejected(ActionRejection::LocationClosed(tavern))
        );
        assert!(matches!(
            world.execute(actor, ProposedAction::Move { destination: home }),
            ActionResult::Success(_)
        ));
    }

    #[test]
    fn movement_updates_both_sides_and_records_an_event() {
        let mut world = World::briar_glen(4).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let from = world.agents[&actor].location;
        let destination = *world.locations[&from]
            .connected
            .iter()
            .find(|id| world.locations[id].is_open(world.tick.hour()))
            .expect("open connected location");

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
            .find(|id| world.locations[id].is_open(world.tick.hour()))
            .expect("open destination");
        world.execute(remote, ProposedAction::Move { destination });
        for agent in world.agents.values_mut() {
            agent.memories.clear();
        }

        world.execute(
            speaker,
            ProposedAction::Talk {
                target: listener,
                tone: DialogueTone::Neutral,
                message: "Did you hear the bells?".into(),
            },
        );

        assert!(world.agents[&remote].memories.is_empty());
        assert!(world.agents[&remote].beliefs.is_empty());
        assert!(!world.agents[&speaker].beliefs.contains_key(&speaker));
        let listener_belief = world.agents[&listener].beliefs[&speaker];
        assert!(listener_belief.sociability > 0.5);
        assert_eq!(listener_belief.confidence, 0.15);
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

        world.append_event(
            Some(home),
            EventKind::Worked {
                agent: speaker,
                wage: WORK_WAGE,
                stock_produced: 0,
            },
        );
        let belief = world.agents[&listener].beliefs[&speaker];
        assert!(belief.reliability > 0.5);
        assert_eq!(belief.confidence, 0.3);
        world
            .advance_to(Tick(world.tick.0 + 10))
            .expect("time advances");
        assert!((world.agents[&listener].beliefs[&speaker].confidence - 0.29).abs() < f32::EPSILON);
        world.validate().expect("valid beliefs");

        world
            .agents
            .get_mut(&listener)
            .expect("listener")
            .beliefs
            .get_mut(&speaker)
            .expect("belief")
            .confidence = 1.1;
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
    }

    #[test]
    fn conversations_propagate_bounded_degrading_rumors() {
        let mut world = World::briar_glen(42).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let subject = residents[0];
        let first_listener = residents[2];
        let second_listener = residents[3];
        let fact = world.append_event(
            None,
            EventKind::Worked {
                agent: subject,
                wage: WORK_WAGE,
                stock_produced: 0,
            },
        );

        world.execute(
            subject,
            ProposedAction::Talk {
                target: first_listener,
                tone: DialogueTone::Neutral,
                message: "Work went well today.".into(),
            },
        );
        let rumor = &world.agents[&first_listener].rumors[0];
        assert_eq!(rumor.event, fact);
        assert_eq!(rumor.source, subject);
        assert_eq!(rumor.depth, 1);
        assert!(rumor.confidence > 0.0 && rumor.confidence <= 1.0);
        assert!(world.agents[&first_listener].beliefs[&subject].reliability > 0.5);

        let first_confidence = rumor.confidence;
        world.execute(
            subject,
            ProposedAction::Talk {
                target: first_listener,
                tone: DialogueTone::Neutral,
                message: "As I was saying.".into(),
            },
        );
        assert_eq!(world.agents[&first_listener].rumors.len(), 1);

        world.execute(
            first_listener,
            ProposedAction::Talk {
                target: second_listener,
                tone: DialogueTone::Neutral,
                message: "I heard work went well.".into(),
            },
        );
        let retelling = world.agents[&second_listener]
            .rumors
            .iter()
            .find(|rumor| rumor.event.id == fact.id)
            .expect("retold rumor");
        assert_eq!(retelling.source, first_listener);
        assert_eq!(retelling.depth, 2);
        assert!(retelling.confidence < first_confidence);
        let retelling_confidence = retelling.confidence;
        world.validate().expect("valid rumors");

        world
            .agents
            .get_mut(&second_listener)
            .expect("listener")
            .rumors[0]
            .confidence = 1.1;
        assert!(matches!(world.validate(), Err(WorldError::InvalidState(_))));
        world
            .agents
            .get_mut(&second_listener)
            .expect("listener")
            .rumors[0]
            .confidence = retelling_confidence;
        world
            .agents
            .get_mut(&second_listener)
            .expect("listener")
            .rumors[0]
            .event
            .id = crate::sim::EventId(Uuid::nil());
        assert!(matches!(
            world.validate_history(),
            Err(WorldError::InvalidState(_))
        ));
    }

    #[test]
    fn confrontations_confirm_deny_and_reject_invalid_claims() {
        fn world_with_rumor(honesty: f32) -> (World, AgentId, AgentId, EventId) {
            let mut world = World::briar_glen(43).expect("town");
            let residents = world.agents.keys().copied().collect::<Vec<_>>();
            let target = residents[0];
            let accuser = residents[2];
            world
                .agents
                .get_mut(&target)
                .expect("target")
                .personality
                .honesty = honesty;
            let fact = world.append_event(
                None,
                EventKind::Worked {
                    agent: target,
                    wage: WORK_WAGE,
                    stock_produced: 0,
                },
            );
            world.execute(
                target,
                ProposedAction::Talk {
                    target: accuser,
                    tone: DialogueTone::Neutral,
                    message: "Work went well.".into(),
                },
            );
            (world, accuser, target, fact.id)
        }

        let (mut world, accuser, target, claim) = world_with_rumor(1.0);
        let old_confidence = world.agents[&accuser].rumors[0].confidence;
        let result = world.execute(accuser, ProposedAction::Confront { target, claim });
        assert!(matches!(
            result,
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Confronted {
                    outcome: ConfrontationOutcome::Confirmed,
                    ..
                })
        ));
        assert!(world.agents[&accuser].rumors[0].confidence > old_confidence);

        let (mut world, accuser, target, claim) = world_with_rumor(0.0);
        let old_trust = world.agents[&target]
            .relationships
            .get(&accuser)
            .expect("conversation relationship")
            .trust;
        assert!(matches!(
            world.execute(accuser, ProposedAction::Confront { target, claim }),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Confronted {
                    outcome: ConfrontationOutcome::Denied,
                    ..
                })
        ));
        assert!(world.agents[&target].relationships[&accuser].trust < old_trust);
        assert!(matches!(
            world.execute(
                accuser,
                ProposedAction::Confront {
                    target,
                    claim: EventId(Uuid::nil()),
                },
            ),
            ActionResult::Rejected(ActionRejection::UnknownClaim(_))
        ));

        world.agents.get_mut(&accuser).expect("accuser").rumors[0].resolved = false;
        let wrong_target = world
            .agents
            .keys()
            .copied()
            .find(|id| *id != accuser && *id != target)
            .expect("third resident");
        assert!(matches!(
            world.execute(
                accuser,
                ProposedAction::Confront {
                    target: wrong_target,
                    claim,
                },
            ),
            ActionResult::Rejected(ActionRejection::ClaimNotAboutTarget { .. })
        ));

        let (mut world, accuser, target, claim) = world_with_rumor(0.5);
        assert!(matches!(
            world.execute(accuser, ProposedAction::Confront { target, claim }),
            ActionResult::Success(ref events)
                if matches!(events[0].kind, EventKind::Confronted {
                    outcome: ConfrontationOutcome::Challenged,
                    ..
                })
        ));
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
                        tone: DialogueTone::Neutral,
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
    fn agreeable_honest_speakers_build_relationships_faster() {
        let mut warm = World::briar_glen(42).expect("town");
        let residents = warm.agents.keys().copied().collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[2];
        let mut cold = warm.clone();
        warm.agents
            .get_mut(&speaker)
            .expect("speaker")
            .personality
            .agreeableness = 1.0;
        warm.agents
            .get_mut(&speaker)
            .expect("speaker")
            .personality
            .honesty = 1.0;
        cold.agents
            .get_mut(&speaker)
            .expect("speaker")
            .personality
            .agreeableness = 0.0;
        cold.agents
            .get_mut(&speaker)
            .expect("speaker")
            .personality
            .honesty = 0.0;
        let talk = ProposedAction::Talk {
            target: listener,
            tone: DialogueTone::Neutral,
            message: "Good to see you.".into(),
        };

        warm.execute(speaker, talk.clone());
        cold.execute(speaker, talk);

        let warm_view = warm.agents[&speaker].relationships[&listener];
        let cold_view = cold.agents[&speaker].relationships[&listener];
        assert!(warm_view.affection > cold_view.affection);
        assert!(warm_view.trust > cold_view.trust);
        assert!(warm_view.suspicion < cold_view.suspicion);
    }

    #[test]
    fn dialogue_tone_changes_relationship_effects() {
        let neutral = World::briar_glen(42).expect("town");
        let residents = neutral.agents.keys().copied().collect::<Vec<_>>();
        let speaker = residents[0];
        let listener = residents[2];
        let mut friendly = neutral.clone();
        let mut supportive = neutral.clone();
        let mut tense = neutral;

        for (world, tone) in [
            (&mut friendly, DialogueTone::Friendly),
            (&mut supportive, DialogueTone::Supportive),
            (&mut tense, DialogueTone::Tense),
        ] {
            world.execute(
                speaker,
                ProposedAction::Talk {
                    target: listener,
                    tone,
                    message: "Hello.".into(),
                },
            );
        }

        assert!(friendly.agents[&speaker].mood > 0.0);
        assert!(supportive.agents[&listener].mood > supportive.agents[&speaker].mood);
        assert!(tense.agents[&speaker].mood < 0.0);
        let friendly = friendly.agents[&speaker].relationships[&listener];
        let supportive = supportive.agents[&speaker].relationships[&listener];
        let tense = tense.agents[&speaker].relationships[&listener];
        assert!(friendly.affection > supportive.affection);
        assert!(supportive.trust > friendly.trust);
        assert!(tense.affection < 0.0 && tense.trust < 0.0 && tense.suspicion > 0.0);
    }

    #[test]
    fn dialogue_is_trimmed_and_bounded_to_one_printable_line() {
        let mut world = World::briar_glen(43).expect("town");
        let residents = world.agents.keys().copied().collect::<Vec<_>>();
        let actor = residents[0];
        let listener = residents[1];

        for (message, rejection) in [
            ("   ".into(), ActionRejection::EmptyMessage),
            ("hello\nthere".into(), ActionRejection::InvalidMessage),
            (
                "x".repeat(MAX_TALK_MESSAGE_CHARS + 1),
                ActionRejection::MessageTooLong {
                    max: MAX_TALK_MESSAGE_CHARS,
                },
            ),
        ] {
            assert_eq!(
                world.execute(
                    actor,
                    ProposedAction::Talk {
                        target: listener,
                        tone: DialogueTone::Neutral,
                        message,
                    },
                ),
                ActionResult::Rejected(rejection)
            );
        }

        let result = world.execute(
            actor,
            ProposedAction::Talk {
                target: listener,
                tone: DialogueTone::Friendly,
                message: "  A concise greeting.  ".into(),
            },
        );
        assert!(matches!(
            result,
            ActionResult::Success(events)
                if matches!(&events[0].kind, EventKind::Spoke { message, .. } if message == "A concise greeting.")
        ));
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
                    tone: DialogueTone::Neutral,
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
        assert_eq!(
            world.agents[&actor].memories,
            world.events()[world.events().len() - 20..]
        );
    }

    #[test]
    fn rejected_action_changes_only_history_and_actor_mood() {
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
                    tone: DialogueTone::Neutral,
                    message: "Hello".into(),
                },
            ),
            ActionResult::Rejected(ActionRejection::UnknownAgent(absent))
        );
        assert_eq!(world.agents[&actor].mood, -0.06);
        world.agents.get_mut(&actor).expect("actor").mood = 0.0;
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
        assert_eq!(world.agents[&actor].mood, -0.06);
        world.agents.get_mut(&actor).expect("actor").mood = 0.0;
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
            .find(|id| world.locations[id].is_open(world.tick.hour()))
            .expect("open connected location");
        assert!(matches!(
            world.execute(target, ProposedAction::Move { destination }),
            ActionResult::Success(_)
        ));
        assert_eq!(
            world.execute(
                actor,
                ProposedAction::Talk {
                    target,
                    tone: DialogueTone::Neutral,
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
        let start = world.tick.0;
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
            vec![Tick(start + 1), Tick(start + 2)]
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

    #[test]
    fn critical_needs_damage_health_and_repair_recovers_injury() {
        let mut world = World::briar_glen(19).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.health = 0.5;
        agent.needs.food = 0.0;
        agent.needs.energy = 1.0;
        agent.needs.safety = 0.0;
        agent.inventory.repair_kits = 1;
        world.advance_to(Tick(world.tick.0 + 100)).expect("advance");
        assert!(world.agents[&actor].health < 0.5);
        assert!(world.agents[&actor].injury);
        world.execute(actor, ProposedAction::UseRepairKit);
        assert!(world.agents[&actor].health > 0.5);
        assert!(!world.agents[&actor].injury);
    }

    #[test]
    fn health_zero_emits_one_death_and_removes_membership() {
        let mut world = World::briar_glen(20).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let other = *world.agents.keys().find(|id| **id != actor).expect("other");
        let location = world.agents[&actor].location;
        world.agents.get_mut(&other).expect("other").intention = Some(Intention {
            goal: IntentionGoal::Talk {
                target: actor,
                tone: DialogueTone::Supportive,
                message: "How are you?".into(),
            },
            expires_at: Tick(world.tick.0 + 10),
        });
        world.agents.get_mut(&actor).expect("resident").health = 0.000001;
        world.agents.get_mut(&actor).expect("resident").needs.food = 0.0;
        world.advance_tick().expect("advance");
        assert!(matches!(
            world.agents[&actor].life,
            LifeState::Dead {
                cause: DeathCause::Starvation,
                ..
            }
        ));
        assert!(!world.locations[&location].agents.contains(&actor));
        assert!(world.agents[&other].intention.is_none());
        assert_eq!(
            world
                .events()
                .iter()
                .filter(
                    |event| matches!(event.kind, EventKind::Died { agent, .. } if agent == actor)
                )
                .count(),
            1
        );
        assert_eq!(
            world.execute(actor, ProposedAction::Wait),
            ActionResult::Rejected(ActionRejection::AgentDead(actor))
        );
        world.validate().expect("valid dead resident history");
    }

    #[test]
    fn briar_fever_is_deterministic_and_infection_is_hidden_from_memories() {
        let mut world = World::briar_glen(1).expect("town");
        world
            .advance_to(Tick(PATIENT_ZERO_TICK))
            .expect("patient zero");
        let patient_zero = world
            .events()
            .iter()
            .find_map(|event| match event.kind {
                EventKind::DiseaseInfected {
                    agent,
                    source: None,
                } => Some(agent),
                _ => None,
            })
            .expect("patient zero event");
        assert!(matches!(
            world.agents[&patient_zero].disease,
            DiseaseState::Incubating { .. }
        ));
        assert!(world.agents.values().all(|agent| {
            agent
                .memories
                .iter()
                .all(|event| !matches!(event.kind, EventKind::DiseaseInfected { .. }))
        }));

        world
            .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
            .expect("symptoms");
        assert!(matches!(
            world.agents[&patient_zero].disease,
            DiseaseState::Symptomatic { .. }
        ));
        assert!(world.events().iter().any(|event| matches!(
            event.kind,
            EventKind::DiseaseSymptoms { agent } if agent == patient_zero
        )));
        assert!(world.events().iter().any(|event| matches!(
            event.kind,
            EventKind::DiseaseInfected {
                source: Some(source),
                ..
            } if source == patient_zero
        )));
    }

    #[test]
    fn briar_fever_recovers_and_immunity_expires() {
        let mut world = World::briar_glen(1).expect("town");
        world
            .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
            .expect("symptoms");
        let patient_zero = world
            .events()
            .iter()
            .find_map(|event| match event.kind {
                EventKind::DiseaseInfected {
                    agent,
                    source: None,
                } => Some(agent),
                _ => None,
            })
            .expect("patient zero");
        world
            .advance_to(Tick(
                PATIENT_ZERO_TICK + INCUBATION_TICKS + SYMPTOMATIC_TICKS,
            ))
            .expect("recovery");
        assert!(matches!(
            world.agents[&patient_zero].disease,
            DiseaseState::Recovering { .. }
        ));
        world
            .advance_to(Tick(
                PATIENT_ZERO_TICK + INCUBATION_TICKS + SYMPTOMATIC_TICKS + RECOVERY_TICKS,
            ))
            .expect("immunity");
        assert!(matches!(
            world.agents[&patient_zero].disease,
            DiseaseState::Immune { .. }
        ));
        world
            .advance_to(Tick(
                PATIENT_ZERO_TICK
                    + INCUBATION_TICKS
                    + SYMPTOMATIC_TICKS
                    + RECOVERY_TICKS
                    + IMMUNITY_TICKS,
            ))
            .expect("immunity expiry");
        assert_eq!(
            world.agents[&patient_zero].disease,
            DiseaseState::Susceptible
        );
        assert!(world.events().iter().any(|event| matches!(
            event.kind,
            EventKind::DiseaseImmunityExpired { agent } if agent == patient_zero
        )));
    }

    #[test]
    fn briar_fever_only_spreads_between_co_located_residents() {
        let mut world = World::briar_glen(1).expect("town");
        world
            .advance_to(Tick(PATIENT_ZERO_TICK))
            .expect("infection");
        let patient_zero = world
            .events()
            .iter()
            .find_map(|event| match event.kind {
                EventKind::DiseaseInfected {
                    agent,
                    source: None,
                } => Some(agent),
                _ => None,
            })
            .expect("patient zero");
        world
            .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS))
            .expect("symptoms");
        let target = world
            .agents
            .values()
            .find(|agent| {
                agent.id != patient_zero && matches!(agent.disease, DiseaseState::Susceptible)
            })
            .expect("susceptible target")
            .id;
        let from = world.agents[&target].location;
        let to = *world.locations[&from]
            .connected
            .iter()
            .next()
            .expect("neighbor");
        world
            .locations
            .get_mut(&from)
            .expect("from")
            .agents
            .remove(&target);
        world
            .locations
            .get_mut(&to)
            .expect("to")
            .agents
            .insert(target);
        world.agents.get_mut(&target).expect("target").location = to;
        world.validate().expect("separated residents");
        let infection_count = world
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::DiseaseInfected { agent, source: Some(_ ) } if agent == target))
            .count();
        world
            .advance_to(Tick(PATIENT_ZERO_TICK + INCUBATION_TICKS + 12))
            .expect("transmission window");
        assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::DiseaseInfected { agent, source: Some(_) } if agent == target))
                .count(),
            infection_count
        );
    }

    #[test]
    fn symptomatic_disease_can_cause_death_once() {
        let mut world = World::briar_glen(22).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let location = world.agents[&actor].location;
        let now = world.tick;
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.health = 0.0005;
        agent.needs.food = 1.0;
        agent.needs.energy = 1.0;
        agent.needs.safety = 1.0;
        agent.disease = DiseaseState::Symptomatic {
            until: Tick(now.0 + 10),
        };
        world.advance_tick().expect("disease damage");
        assert!(matches!(
            world.agents[&actor].life,
            LifeState::Dead {
                cause: DeathCause::Disease,
                ..
            }
        ));
        assert_eq!(world.agents[&actor].disease, DiseaseState::Susceptible);
        assert!(!world.locations[&location].agents.contains(&actor));
        assert_eq!(
            world
                .events()
                .iter()
                .filter(|event| matches!(event.kind, EventKind::Died { agent, cause: DeathCause::Disease } if agent == actor))
                .count(),
            1
        );
        world.validate().expect("dead resident history");
    }

    #[test]
    fn dead_residents_are_excluded_from_observation_and_scheduling() {
        let mut world = World::briar_glen(21).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let location = world.agents[&actor].location;
        world.agents.get_mut(&actor).expect("resident").life = LifeState::Dead {
            tick: world.tick,
            cause: DeathCause::Injury,
        };
        world.agents.get_mut(&actor).expect("resident").health = 0.0;
        for agent in world.agents.values_mut() {
            agent.goals.clear();
        }
        world
            .locations
            .get_mut(&location)
            .expect("location")
            .agents
            .remove(&actor);
        assert!(matches!(
            crate::cognition::perceive(&world, actor),
            Err(crate::cognition::ObservationError::AgentDead(id)) if id == actor
        ));
        assert!(!Scheduler.agents_to_act(&world).contains(&actor));
        world.validate().expect("valid dead resident history");
    }
}
