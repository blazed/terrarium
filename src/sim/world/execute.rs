use super::*;

impl World {
    fn validate_colocated_target(
        &self,
        actor: AgentId,
        target: AgentId,
        location: LocationId,
    ) -> Result<(), ActionRejection> {
        if target == actor {
            return Err(ActionRejection::SelfTarget(actor));
        }
        let Some(target_agent) = self.agents.get(&target) else {
            return Err(ActionRejection::UnknownAgent(target));
        };
        if !target_agent.is_alive() {
            return Err(ActionRejection::AgentDead(target));
        }
        if target_agent.location != location {
            return Err(ActionRejection::NotCoLocated { actor, target });
        }
        Ok(())
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
                return self.start_intention(actor, intention, false);
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
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
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
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
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
            ProposedAction::Give { target, item } => {
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
                }
                let receiver = &self.agents[&target];
                if agent.inventory.count(item) == 0 {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemUnavailable(item),
                    );
                }
                if !receiver.inventory.has_capacity(item) {
                    return self.reject(actor, Some(current), ActionRejection::InventoryFull(item));
                }
                if !receiver.needs_item(item) {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ItemNotNeeded { target, item },
                    );
                }
                EventKind::ItemGiven {
                    giver: actor,
                    receiver: target,
                    item,
                }
            }
            ProposedAction::Steal { target, loot } => {
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
                }
                let target_agent = &self.agents[&target];
                match loot {
                    Loot::Coins(amount) => {
                        if amount == 0 || target_agent.balance < amount {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::LootNotOwned { target, loot },
                            );
                        }
                        if agent.balance.checked_add(amount).is_none() {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::EconomyOverflow,
                            );
                        }
                    }
                    Loot::Item(item) => {
                        if target_agent.inventory.count(item) == 0 {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::LootNotOwned { target, loot },
                            );
                        }
                        if !agent.inventory.has_capacity(item) {
                            return self.reject(
                                actor,
                                Some(current),
                                ActionRejection::InventoryFull(item),
                            );
                        }
                    }
                }
                let witnesses = self.locations[&current]
                    .agents
                    .iter()
                    .copied()
                    .filter(|id| *id != actor && *id != target)
                    .filter(|id| self.agents[id].activity.is_none())
                    .count();
                let victim_busy = target_agent.activity.is_some();
                let probability = (0.55 + if victim_busy { 0.2 } else { 0.0 }
                    - 0.1 * agent.personality.honesty
                    - 0.1 * agent.personality.impulsiveness
                    - 0.1 * witnesses as f32)
                    .clamp(0.05, 0.95);
                if self.seeded_roll(actor, target, probability) {
                    EventKind::Stole {
                        thief: actor,
                        victim: target,
                        loot,
                    }
                } else {
                    EventKind::TheftFailed {
                        thief: actor,
                        victim: target,
                        loot,
                    }
                }
            }
            ProposedAction::Attack { target } => {
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
                }
                EventKind::Assaulted {
                    attacker: actor,
                    victim: target,
                }
            }
            ProposedAction::Arrest { target, claim } => {
                if self.agents[&actor].occupation != Occupation::Sheriff {
                    return self.reject(actor, Some(current), ActionRejection::NotSheriff);
                }
                if let Err(reason) = self.validate_colocated_target(actor, target, current) {
                    return self.reject(actor, Some(current), reason);
                }
                let target_agent = &self.agents[&target];
                let Some(event) = self.events.iter().find(|event| event.id == claim) else {
                    return self.reject(actor, Some(current), ActionRejection::UnknownClaim(claim));
                };
                // Arrestable claims are crime events with a subject; Robbed has none.
                if !matches!(event_evidence(&event.kind), Some((subject, ..)) if subject == target)
                {
                    return self.reject(
                        actor,
                        Some(current),
                        ActionRejection::ClaimNotAboutTarget { claim, target },
                    );
                }
                if !agent.has_legal_basis(claim) {
                    return self.reject(actor, Some(current), ActionRejection::NoLegalBasis(claim));
                }
                let fine = if target_agent.balance >= JAIL_FINE {
                    JAIL_FINE
                } else {
                    0
                };
                EventKind::Arrested {
                    officer: actor,
                    prisoner: target,
                    claim,
                    fine,
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
            if matches!(&kind, EventKind::Arrested { .. }) {
                // A sentence is exactly one day and never doubles for poor health.
                activity.until = Tick(self.tick.0.saturating_add(JAIL_TICKS));
            } else if self.agents[&actor].health < 0.5 {
                let duration = activity.until.0.saturating_sub(self.tick.0);
                activity.until = Tick(self.tick.0.saturating_add(duration.saturating_mul(2)));
            }
            match &kind {
                // The prisoner serves the term; the sheriff goes on with the day.
                EventKind::Arrested { prisoner, .. } => {
                    self.agents
                        .get_mut(prisoner)
                        .expect("validated prisoner")
                        .activity = Some(activity);
                }
                _ => {
                    self.agents.get_mut(&actor).expect("known actor").activity = Some(activity);
                    let other = match &kind {
                        EventKind::Spoke { listener, .. } => Some(*listener),
                        EventKind::Confronted { target, .. } => Some(*target),
                        EventKind::Assaulted { victim, .. } => Some(*victim),
                        _ => None,
                    };
                    if let Some(other) = other {
                        self.agents
                            .get_mut(&other)
                            .expect("validated resident")
                            .activity = Some(activity);
                    }
                }
            }
        }
        let completed_goal = self.advance_goal(actor, &kind);
        let robbed = match &kind {
            EventKind::Stole { victim, loot, .. } => Some(EventKind::Robbed {
                victim: *victim,
                loot: *loot,
            }),
            _ => None,
        };
        let mut events = vec![self.append_event(Some(current), kind)];
        if let Some(robbed) = robbed {
            events.push(self.append_event(Some(current), robbed));
        }
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

    pub(super) fn reject(
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

    pub(super) fn append_event(&mut self, location: Option<LocationId>, kind: EventKind) -> Event {
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
}
