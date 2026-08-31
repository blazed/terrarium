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
            EventKind::Stole { victim, loot, .. } => {
                let thief = self.agents.get_mut(&actor).expect("validated thief");
                match loot {
                    Loot::Coins(amount) => {
                        thief.balance = thief
                            .balance
                            .checked_add(*amount)
                            .expect("validated theft balance");
                    }
                    Loot::Item(item) => thief.inventory.add(*item),
                }
                let victim_state = self.agents.get_mut(victim).expect("validated victim");
                match loot {
                    Loot::Coins(amount) => victim_state.balance -= *amount,
                    Loot::Item(item) => victim_state.inventory.remove(*item),
                }
                victim_state.needs.safety = (victim_state.needs.safety - 0.15).max(0.0);
            }
            EventKind::TheftFailed { victim, .. } => {
                if let Some(victim_state) = self.agents.get_mut(victim) {
                    victim_state.needs.safety = (victim_state.needs.safety - 0.1).max(0.0);
                }
            }
            EventKind::Assaulted { victim, .. } => {
                {
                    let victim_state = self.agents.get_mut(victim).expect("validated victim");
                    // ponytail: floor at 0.01 instead of 0.0 because death is processed
                    // only in advance_to — an alive agent with health exactly 0.0 fails
                    // validate(). Repeated assaults still push the victim toward real death
                    // via the normal injury/needs damage path.
                    victim_state.health = (victim_state.health - 0.35).max(0.01);
                    victim_state.injury = true;
                    victim_state.needs.safety = (victim_state.needs.safety - 0.2).max(0.0);
                    let relationship = victim_state
                        .relationships
                        .entry(actor)
                        .or_insert(Relationship::NEUTRAL);
                    relationship.affection = (relationship.affection - 0.2).clamp(-1.0, 1.0);
                    relationship.trust = (relationship.trust - 0.25).clamp(-1.0, 1.0);
                    relationship.respect = (relationship.respect - 0.2).clamp(-1.0, 1.0);
                    relationship.suspicion = (relationship.suspicion + 0.3).clamp(-1.0, 1.0);
                }
                if let Some(attacker) = self.agents.get_mut(&actor) {
                    attacker.needs.safety = (attacker.needs.safety - 0.05).max(0.0);
                }
            }
            EventKind::Arrested { prisoner, fine, .. } => {
                // The prisoner is confined to Jail and pays the fine to Town Hall.
                let jail = self
                    .locations
                    .values()
                    .find(|location| location.name == "Jail")
                    .expect("town has a jail")
                    .id;
                self.relocate(*prisoner, jail);
                // Relocation can invalidate goals (e.g. a Visit-Jail goal), so prune.
                self.refresh_goals(*prisoner);
                if *fine > 0 {
                    if let Some(prisoner) = self.agents.get_mut(prisoner) {
                        prisoner.balance -= *fine;
                    }
                    if let Some(town_hall) = self.locations.values_mut().find(|location| {
                        location
                            .business
                            .is_some_and(|business| business.offering == Offering::CivicServices)
                    }) {
                        town_hall
                            .business
                            .as_mut()
                            .expect("town hall business")
                            .cash += *fine;
                    }
                }
            }
            EventKind::Released { .. } => {}
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
            | EventKind::ActionRejected { .. }
            | EventKind::Robbed { .. } => {}
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

    fn adjust_mood(&mut self, id: AgentId, amount: f32) {
        if let Some(agent) = self.agents.get_mut(&id) {
            agent.mood = (agent.mood + amount).clamp(-1.0, 1.0);
        }
    }

    pub(super) fn apply_mood_effects(&mut self, kind: &EventKind) {
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
                self.adjust_mood(*speaker, speaker_change);
                self.adjust_mood(*listener, listener_change);
            }
            EventKind::Purchased { agent, .. } => self.adjust_mood(*agent, 0.06),
            EventKind::ItemGiven {
                giver, receiver, ..
            } => {
                self.adjust_mood(*giver, 0.05);
                self.adjust_mood(*receiver, 0.08);
            }
            EventKind::ItemUsed { agent, .. } => self.adjust_mood(*agent, 0.04),
            EventKind::Treated { agent, .. } => self.adjust_mood(*agent, 0.1),
            EventKind::Rested { agent } => self.adjust_mood(*agent, 0.08),
            EventKind::Worked { agent, .. } => self.adjust_mood(*agent, 0.03),
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
                self.adjust_mood(*accuser, accuser_change);
                self.adjust_mood(*target, target_change);
            }
            EventKind::Observed { observer, .. } => self.adjust_mood(*observer, 0.02),
            EventKind::Stole { thief, victim, .. } => {
                self.adjust_mood(*thief, 0.1);
                self.adjust_mood(*victim, -0.15);
            }
            EventKind::TheftFailed { thief, victim, .. } => {
                self.adjust_mood(*thief, -0.08);
                self.adjust_mood(*victim, -0.08);
            }
            EventKind::Assaulted { attacker, victim } => {
                self.adjust_mood(*victim, -0.25);
                if self.agents[attacker].personality.agreeableness >= 0.5 {
                    self.adjust_mood(*attacker, -0.15);
                }
            }
            EventKind::Arrested { prisoner, .. } => self.adjust_mood(*prisoner, -0.2),
            EventKind::GoalCompleted { agent, .. } => self.adjust_mood(*agent, 0.15),
            EventKind::ActionRejected { agent, .. } => self.adjust_mood(*agent, -0.06),
            EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::Moved { .. }
            | EventKind::Died { .. }
            | EventKind::DiseaseInfected { .. }
            | EventKind::DiseaseSymptoms { .. }
            | EventKind::DiseaseRecovered { .. }
            | EventKind::DiseaseImmunityExpired { .. }
            | EventKind::Waited { .. }
            | EventKind::Robbed { .. }
            | EventKind::Released { .. } => {}
        }
    }

    pub(super) fn remember(&mut self, event: &Event) {
        let mut witnesses = BTreeSet::new();
        let mut blanket_location = true;
        match &event.kind {
            EventKind::Stole { thief, victim, .. } => {
                // Idle onlookers (and the thief) notice a successful theft; the victim
                // only learns from the subject-free Robbed memory.
                witnesses.insert(*thief);
                if let Some(location) = event.location.and_then(|id| self.locations.get(&id)) {
                    witnesses.extend(location.agents.iter().copied().filter(|id| {
                        *id != *victim
                            && self
                                .agents
                                .get(id)
                                .is_some_and(|agent| agent.activity.is_none())
                    }));
                }
                blanket_location = false;
            }
            EventKind::Robbed { victim, .. } => {
                witnesses.insert(*victim);
                blanket_location = false;
            }
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
            EventKind::TheftFailed { thief, victim, .. } => {
                witnesses.extend([*thief, *victim]);
            }
            EventKind::Assaulted { attacker, victim } => {
                witnesses.extend([*attacker, *victim]);
            }
            EventKind::Arrested {
                officer, prisoner, ..
            } => {
                witnesses.extend([*officer, *prisoner]);
            }
            EventKind::Released { agent } => {
                witnesses.insert(*agent);
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
        if blanket_location
            && let Some(location) = event.location.and_then(|id| self.locations.get(&id))
        {
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
