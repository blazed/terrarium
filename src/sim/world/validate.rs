use super::*;

impl World {
    pub(super) fn validate_history(&self) -> Result<(), WorldError> {
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
            let invalid_aid = matches!(
                &event.kind,
                EventKind::ItemGiven {
                    giver,
                    receiver,
                    ..
                } if giver == receiver
                    || !self.agents.contains_key(giver)
                    || !self.agents.contains_key(receiver)
            );
            if invalid_aid {
                return Err(WorldError::InvalidState(format!(
                    "event {} references invalid mutual aid participants",
                    event.id
                )));
            }
            let invalid_crime = match &event.kind {
                EventKind::Stole { thief, victim, .. }
                | EventKind::TheftFailed { thief, victim, .. } => {
                    thief == victim
                        || !self.agents.contains_key(thief)
                        || !self.agents.contains_key(victim)
                }
                EventKind::Assaulted { attacker, victim } => {
                    attacker == victim
                        || !self.agents.contains_key(attacker)
                        || !self.agents.contains_key(victim)
                }
                EventKind::Arrested {
                    officer,
                    prisoner,
                    claim,
                    ..
                } => {
                    officer == prisoner
                        || !self.agents.contains_key(officer)
                        || !self.agents.contains_key(prisoner)
                        || self.agents[officer].occupation != Occupation::Sheriff
                        || !self.events.iter().any(|event| event.id == *claim)
                }
                EventKind::Released { agent } => !self.agents.contains_key(agent),
                EventKind::Robbed { victim, .. } => !self.agents.contains_key(victim),
                _ => false,
            };
            if invalid_crime {
                return Err(WorldError::InvalidState(format!(
                    "event {} references invalid crime participants",
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
            if location.kind == LocationKind::Home && location.opening_hours.is_some() {
                return Err(WorldError::InvalidState(format!(
                    "home location {id} has opening hours"
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
            if self.locations[&agent.home].kind != LocationKind::Home {
                return Err(WorldError::InvalidState(format!(
                    "agent {id} is assigned to a non-Home location"
                )));
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
                || agent
                    .routing
                    .llm_intentions_completed
                    .saturating_add(agent.routing.llm_intentions_interrupted)
                    > agent.routing.llm_intentions_started
                || (agent.llm_intention && agent.intention.is_none())
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
                    IntentionGoal::Give { target, .. } => {
                        target != id && self.agents.get(target).is_some_and(Agent::is_alive)
                    }
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
                    IntentionGoal::Observe { target } => match target {
                        ObservationTarget::Agent(target) => self.agents.contains_key(target),
                        ObservationTarget::Location(target) => self.locations.contains_key(target),
                    },
                    IntentionGoal::Confront { target, claim } => {
                        target != id
                            && self.agents.get(target).is_some_and(Agent::is_alive)
                            && self.events.iter().any(|event| event.id == *claim)
                    }
                    IntentionGoal::StealFrom { target, loot } => {
                        let target_owned = match loot {
                            Loot::Coins(amount) => {
                                *amount > 0
                                    && self
                                        .agents
                                        .get(target)
                                        .is_some_and(|agent| agent.balance >= *amount)
                            }
                            Loot::Item(item) => self
                                .agents
                                .get(target)
                                .is_some_and(|agent| agent.inventory.count(*item) > 0),
                        };
                        target != id
                            && self.agents.get(target).is_some_and(Agent::is_alive)
                            && target_owned
                    }
                    IntentionGoal::Attack { target } => {
                        target != id && self.agents.get(target).is_some_and(Agent::is_alive)
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
