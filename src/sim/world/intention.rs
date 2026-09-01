use super::*;

impl World {
    pub fn is_location_open(&self, location: LocationId) -> bool {
        if self
            .active_town_event
            .is_some_and(|event| event.kind == TownEventKind::Storm)
        {
            self.locations[&location].kind == LocationKind::Home
        } else {
            self.locations[&location].is_open(self.tick.hour())
        }
    }

    pub(super) fn start_intention(
        &mut self,
        actor: AgentId,
        goal: IntentionGoal,
        llm_intention: bool,
    ) -> ActionResult {
        let expires_at = Tick(self.tick.0.saturating_add(INTENTION_DURATION_TICKS));
        let intention = Intention { goal, expires_at };
        if let Err(rejection) = self.intention_action(actor, &intention) {
            let location = self.agents.get(&actor).map(|agent| agent.location);
            return self.reject(actor, location, rejection);
        }
        let agent = self
            .agents
            .get_mut(&actor)
            .expect("validated intention actor");
        agent.intention = Some(intention);
        agent.llm_intention = llm_intention;
        if llm_intention {
            agent.routing.llm_intentions_started =
                agent.routing.llm_intentions_started.saturating_add(1);
        }
        self.continue_intention_inner(actor, false)
            .unwrap_or_else(|| self.execute(actor, ProposedAction::Wait))
    }

    pub fn continue_intention(&mut self, actor: AgentId) -> Option<ActionResult> {
        self.continue_intention_inner(actor, true)
    }

    fn continue_intention_inner(
        &mut self,
        actor: AgentId,
        follow_up: bool,
    ) -> Option<ActionResult> {
        if !self.agents.get(&actor).is_some_and(Agent::is_alive) {
            return None;
        }
        let intention = self.agents.get(&actor)?.intention.clone()?;
        let agent = self.agents.get_mut(&actor)?;
        if follow_up && agent.llm_intention {
            agent.routing.llm_intention_steps = agent.routing.llm_intention_steps.saturating_add(1);
        }
        let needs = &agent.needs;
        let medical_need = agent.health < 0.5 || agent.injury || agent.disease.is_symptomatic();
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
                ))
            || (medical_need
                && !matches!(
                    &intention.goal,
                    IntentionGoal::SeekTreatment | IntentionGoal::Rest
                ));
        if intention.expires_at <= self.tick || interrupted {
            self.clear_intention(
                actor,
                !interrupted && matches!(intention.goal, IntentionGoal::Work),
            );
            return None;
        }
        let action = match self.intention_action(actor, &intention) {
            Ok(Some(action)) => action,
            Ok(None) => {
                self.clear_intention(actor, true);
                return None;
            }
            Err(rejection) => {
                let location = self.agents.get(&actor).map(|agent| agent.location);
                self.clear_intention(actor, false);
                return Some(self.reject(actor, location, rejection));
            }
        };
        let terminal = !matches!(action, ProposedAction::Move { .. });
        let result = self.execute(actor, action);
        let rejected = matches!(result, ActionResult::Rejected(_));
        let durable = self
            .agents
            .get(&actor)
            .is_some_and(|agent| agent.llm_intention);
        let completed = rejected
            || self.intention_complete(actor, &intention.goal)
            || (terminal
                && (!durable
                    || !matches!(intention.goal, IntentionGoal::Rest | IntentionGoal::Work)))
            || (durable
                && matches!(intention.goal, IntentionGoal::Rest)
                && self.agents[&actor].needs.energy >= 0.8);
        if completed {
            self.clear_intention(actor, !rejected);
        }
        Some(result)
    }

    fn clear_intention(&mut self, actor: AgentId, completed: bool) {
        if let Some(agent) = self.agents.get_mut(&actor) {
            if agent.llm_intention {
                if completed {
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
    }

    pub(crate) fn intention_action(
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
            IntentionGoal::Give { target, item } => Ok(Some(ProposedAction::Give {
                target: *target,
                item: *item,
            })),
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
            IntentionGoal::Observe { target } => Ok(Some(ProposedAction::Observe {
                target: target.clone(),
            })),
            IntentionGoal::Confront { target, claim } => Ok(Some(ProposedAction::Confront {
                target: *target,
                claim: *claim,
            })),
            IntentionGoal::StealFrom { target, loot } => Ok(Some(ProposedAction::Steal {
                target: *target,
                loot: *loot,
            })),
            IntentionGoal::Attack { target } => {
                Ok(Some(ProposedAction::Attack { target: *target }))
            }
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

    pub(super) fn next_route_step(
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
}
