use super::*;

impl World {
    pub(super) fn refresh_goals(&mut self, actor: AgentId) {
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
                !visited.contains(destination)
                    && self.is_location_open(*destination)
                    && self.locations[destination].kind != LocationKind::Home
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

    pub(super) fn goal_target_is_valid(&self, actor: AgentId, target: &GoalTarget) -> bool {
        let agent = &self.agents[&actor];
        match target {
            GoalTarget::Work { workplace } => agent.workplace == Some(*workplace),
            GoalTarget::Talk { resident } => {
                *resident != actor && self.agents.get(resident).is_some_and(Agent::is_alive)
            }
            GoalTarget::Visit { destination } => {
                *destination != agent.location
                    && self
                        .locations
                        .get(destination)
                        .is_some_and(|location| location.kind != LocationKind::Home)
            }
            GoalTarget::Purchase { location } => self
                .locations
                .get(location)
                .is_some_and(|location| location.business.is_some()),
            GoalTarget::Rest { home } => *home == agent.home,
        }
    }

    pub(super) fn advance_goal(&mut self, actor: AgentId, event: &EventKind) -> Option<String> {
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
}
