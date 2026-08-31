use super::*;

impl World {
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
        self.seeded_roll(source, target, 0.35)
    }

    /// Deterministic seeded roll from (seed, tick, two agent ids). Pure: same inputs
    /// always produce the same outcome, so split and checkpointed runs match.
    /// Order of the ids is irrelevant (XOR mixing).
    pub(super) fn seeded_roll(&self, first: AgentId, second: AgentId, probability: f32) -> bool {
        let first = first.0.as_u128();
        let second = second.0.as_u128();
        let mixed = self.seed
            ^ self.tick.0.wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ first as u64
            ^ (first >> 64) as u64
            ^ second as u64
            ^ (second >> 64) as u64;
        StdRng::seed_from_u64(mixed).random_bool(probability as f64)
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
        let mut releases = Vec::new();
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
            // Only a jail term's expiry frees a prisoner; storms or starvation never do.
            let jailed_expiry = agent.activity.is_some_and(|activity| {
                activity.kind == ActivityKind::Jailed && activity.until <= proposed
            });
            if agent.activity.is_some_and(|activity| {
                activity.until <= proposed || (urgent && activity.kind != ActivityKind::Jailed)
            }) {
                agent.activity = None;
                if jailed_expiry {
                    releases.push(agent.id);
                }
            }
            if agent
                .intention
                .as_ref()
                .is_some_and(|intention| intention.expires_at <= proposed)
            {
                if agent.llm_intention {
                    if agent
                        .intention
                        .as_ref()
                        .is_some_and(|intention| matches!(intention.goal, IntentionGoal::Work))
                    {
                        agent.routing.llm_intentions_completed =
                            agent.routing.llm_intentions_completed.saturating_add(1);
                    } else {
                        agent.routing.llm_intentions_interrupted =
                            agent.routing.llm_intentions_interrupted.saturating_add(1);
                    }
                }
                agent.intention = None;
                agent.llm_intention = false;
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
                if agent.llm_intention {
                    agent.routing.llm_intentions_interrupted =
                        agent.routing.llm_intentions_interrupted.saturating_add(1);
                }
                agent.intention = None;
                agent.llm_intention = false;
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
        for agent in releases {
            let Some(prisoner) = self.agents.get(&agent).filter(|agent| agent.is_alive()) else {
                continue;
            };
            let home = prisoner.home;
            self.relocate(agent, home);
            self.append_event(Some(home), EventKind::Released { agent });
        }
        for agent in self.agents.values_mut().filter(|agent| agent.is_alive()) {
            if agent.intention.as_ref().is_some_and(|intention| {
                matches!(
                    intention.goal,
                    IntentionGoal::Talk { target, .. }
                        | IntentionGoal::Confront { target, .. }
                        | IntentionGoal::Give { target, .. }
                        if deceased.contains(&target)
                )
            }) {
                if agent.llm_intention {
                    agent.routing.llm_intentions_interrupted =
                        agent.routing.llm_intentions_interrupted.saturating_add(1);
                }
                agent.intention = None;
                agent.llm_intention = false;
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
                    if agent.llm_intention {
                        agent.routing.llm_intentions_interrupted =
                            agent.routing.llm_intentions_interrupted.saturating_add(1);
                    }
                    agent.intention = None;
                    agent.llm_intention = false;
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

    pub(super) fn stock_per_shift(&self, tick: Tick) -> u32 {
        match TownEvent::scheduled(self.seed, tick).map(|event| event.kind) {
            Some(TownEventKind::Shortage) => STOCK_PER_SHIFT / 2,
            Some(TownEventKind::MarketDay) => STOCK_PER_SHIFT * 2,
            _ => STOCK_PER_SHIFT,
        }
    }
}
