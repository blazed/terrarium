use super::*;

impl World {
    pub(super) fn apply_action_effects(&mut self, actor: AgentId, kind: &EventKind) {
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
            EventKind::ItemGiven { receiver, item, .. } => {
                self.agents
                    .get_mut(&actor)
                    .expect("validated giver")
                    .inventory
                    .remove(*item);
                self.agents
                    .get_mut(receiver)
                    .expect("validated receiver")
                    .inventory
                    .add(*item);
                self.apply_dialogue_relationship(actor, *receiver, DialogueTone::Supportive, 0.5);
                self.apply_dialogue_relationship(*receiver, actor, DialogueTone::Supportive, 1.5);
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

    pub(super) fn apply_mood_effects(&mut self, kind: &EventKind) {
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
            EventKind::ItemGiven {
                giver, receiver, ..
            } => {
                adjust(*giver, 0.05);
                adjust(*receiver, 0.08);
            }
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

    pub(super) fn remember(&mut self, event: &Event) {
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
            EventKind::ItemGiven {
                giver, receiver, ..
            } => {
                witnesses.extend([*giver, *receiver]);
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
}
