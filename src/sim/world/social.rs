use super::*;

pub(crate) fn event_evidence(kind: &EventKind) -> Option<(AgentId, f32, f32, f32)> {
    match kind {
        EventKind::Spoke { speaker, tone, .. } => Some(match tone {
            DialogueTone::Friendly => (*speaker, 0.08, 0.0, -0.03),
            DialogueTone::Supportive => (*speaker, 0.06, 0.06, -0.03),
            DialogueTone::Neutral => (*speaker, 0.04, 0.0, 0.0),
            DialogueTone::Tense => (*speaker, 0.02, -0.03, 0.12),
        }),
        EventKind::Worked { agent, .. } => Some((*agent, 0.0, 0.08, 0.0)),
        EventKind::ItemGiven { giver, .. } => Some((*giver, 0.05, 0.08, -0.02)),
        _ => None,
    }
}

impl World {
    pub(super) fn share_rumor(&mut self, speaker: AgentId, listener: AgentId) {
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

    pub(super) fn resolve_confrontation(
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
}
