use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::{AgentObservation, RouteHint, VisibleAgent},
    sim::{
        Business, Decision, DialogueTone, GoalKind, GoalTarget, HealthCondition, IntentionGoal,
        ObservationTarget, Offering, ProposedAction, Tick, TownEventKind,
    },
};
use rand::{Rng, SeedableRng, rngs::StdRng};

const RECENT_CONVERSATION_TICKS: u64 = Tick::PER_DAY / 4;

fn recently_spoken(observation: &AgentObservation, agent: &VisibleAgent) -> bool {
    agent
        .last_conversation
        .is_some_and(|last| observation.tick.0.saturating_sub(last.0) < RECENT_CONVERSATION_TICKS)
}

fn preferred_companion(
    observation: &AgentObservation,
    allow_recent: bool,
) -> Option<&VisibleAgent> {
    let relationship_score = |agent: &VisibleAgent| {
        let relationship = agent.relationship;
        let belief = observation
            .beliefs
            .get(&agent.id)
            .map(|belief| {
                0.5 * belief.confidence
                    * (belief.sociability - 0.5 + belief.reliability - 0.5 - belief.hostility)
            })
            .unwrap_or_default();
        relationship.score() + relationship.attraction - relationship.fear + belief
    };
    observation
        .visible_agents
        .iter()
        .filter(|agent| observation.action_affordances.talk_to.contains(&agent.id))
        .filter(|agent| allow_recent || !recently_spoken(observation, agent))
        .max_by(|left, right| {
            (!recently_spoken(observation, left))
                .cmp(&!recently_spoken(observation, right))
                .then_with(|| relationship_score(left).total_cmp(&relationship_score(right)))
        })
}

pub struct RandomDecisionEngine {
    seed: u64,
}

impl RandomDecisionEngine {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl RandomDecisionEngine {
    pub async fn decide(
        &mut self,
        observation: &AgentObservation,
    ) -> Result<ProposedAction, DecisionError> {
        let mut seed = [0; 32];
        seed[..8].copy_from_slice(&self.seed.to_le_bytes());
        seed[8..16].copy_from_slice(&observation.tick.0.to_le_bytes());
        seed[16..].copy_from_slice(observation.self_description.id.0.as_bytes());
        let mut rng = StdRng::from_seed(seed);

        let needs = &observation.self_description.needs;
        let personality = &observation.self_description.personality;
        let mood = observation.self_description.mood;
        let follow_route = |hint: Option<RouteHint>, intention: IntentionGoal| {
            hint.filter(|hint| {
                observation
                    .action_affordances
                    .move_to
                    .contains(&hint.next_hop)
            })
            .map_or(ProposedAction::Wait, |_| ProposedAction::Pursue {
                intention,
            })
        };
        if observation.action_affordances.can_use_medicine {
            return Ok(ProposedAction::UseMedicine);
        }
        let medical_need = observation.self_description.injury
            || observation
                .self_description
                .conditions
                .contains(&HealthCondition::Sick);
        if medical_need {
            if observation.action_affordances.can_seek_treatment {
                return Ok(ProposedAction::SeekTreatment);
            }
            if let Some(route) = observation.route_hints.treatment {
                return Ok(follow_route(Some(route), IntentionGoal::SeekTreatment));
            }
        }
        if observation.self_description.health < 0.25
            && observation.action_affordances.can_use_repair_kit
        {
            return Ok(ProposedAction::UseRepairKit);
        }
        if observation.self_description.health < 0.25 {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, IntentionGoal::Rest)
                },
            );
        }

        if observation
            .town_event
            .as_ref()
            .is_some_and(|event| event.kind == TownEventKind::Storm)
        {
            if needs.safety < 0.3 && observation.action_affordances.can_use_repair_kit {
                return Ok(ProposedAction::UseRepairKit);
            }
            if needs.safety < 0.6 && observation.action_affordances.can_use_supplies {
                return Ok(ProposedAction::UseSupplies);
            }
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, IntentionGoal::Rest)
                },
            );
        }

        let desired_offering = Offering::desired(needs);
        if desired_offering == Some(Offering::Meal) {
            if observation.action_affordances.can_consume_meal {
                return Ok(ProposedAction::ConsumeMeal);
            }
            return Ok(purchase_action(observation, Offering::Meal)
                .or_else(|| work_action(observation))
                .unwrap_or(ProposedAction::Wait));
        }
        if needs.energy < 0.2 + 0.1 * personality.neuroticism {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, IntentionGoal::Rest)
                },
            );
        }
        if needs.companionship < 0.2 + 0.1 * personality.agreeableness
            && let Some(companion) = preferred_companion(observation, true)
        {
            return Ok(talk(observation, companion));
        }
        if desired_offering == Some(Offering::Repairs)
            && observation.action_affordances.can_use_repair_kit
        {
            return Ok(ProposedAction::UseRepairKit);
        }
        if matches!(
            desired_offering,
            Some(Offering::Repairs | Offering::Supplies)
        ) && observation.action_affordances.can_use_supplies
        {
            return Ok(ProposedAction::UseSupplies);
        }
        if let Some(offering @ (Offering::Repairs | Offering::Supplies)) = desired_offering
            && let Some(action) = purchase_action(observation, offering)
        {
            return Ok(action);
        }
        if needs.safety < 0.1 + 0.2 * personality.neuroticism {
            return Ok(ProposedAction::Observe {
                target: ObservationTarget::Location(observation.current_location.id),
            });
        }
        if observation.self_description.balance < 5 * 2
            && let Some(action) = work_action(observation)
        {
            return Ok(action);
        }

        if let Some(event) = &observation.town_event {
            match event.kind {
                TownEventKind::Festival => {
                    if let Some(companion) = preferred_companion(observation, false) {
                        return Ok(talk(observation, companion));
                    }
                }
                TownEventKind::MarketDay => {
                    if let Some(action) = work_action(observation) {
                        return Ok(action);
                    }
                }
                TownEventKind::Storm | TownEventKind::Shortage => {}
            }
        }

        if let Some(confrontation) =
            observation
                .action_affordances
                .confront
                .iter()
                .max_by(|left, right| {
                    let confidence = |claim| {
                        observation
                            .rumors
                            .iter()
                            .find(|rumor| rumor.claim == claim)
                            .map_or(0.0, |rumor| rumor.confidence)
                    };
                    confidence(left.claim).total_cmp(&confidence(right.claim))
                })
        {
            return Ok(ProposedAction::Confront {
                target: confrontation.target,
                claim: confrontation.claim,
            });
        }

        if desired_offering == Some(Offering::CivicServices)
            && let Some(action) = purchase_action(observation, Offering::CivicServices)
        {
            return Ok(action);
        }

        let hour = observation.tick.hour();
        if !(7..21).contains(&hour) {
            return Ok(
                if observation.current_location.id == observation.self_description.home.id {
                    ProposedAction::Rest
                } else {
                    follow_route(observation.route_hints.home, IntentionGoal::Rest)
                },
            );
        }

        let mut goal_priorities = [
            (GoalKind::Livelihood, personality.ambition),
            (GoalKind::Community, personality.agreeableness),
            (
                GoalKind::Exploration,
                (personality.openness + personality.impulsiveness) / 2.0,
            ),
            (GoalKind::Wellbeing, personality.neuroticism),
        ];
        goal_priorities.sort_by(|left, right| right.1.total_cmp(&left.1));
        for (goal_kind, _) in goal_priorities {
            let Some(goal) = observation.goals.iter().find(|goal| goal.kind == goal_kind) else {
                continue;
            };
            match goal.target {
                GoalTarget::Work { workplace }
                    if observation.self_description.workplace.as_ref().is_some_and(
                        |location| {
                            location.id == workplace
                                && location.is_open
                                && location.business.is_some_and(Business::solvent)
                        },
                    ) =>
                {
                    return Ok(if observation.action_affordances.can_work {
                        ProposedAction::Work
                    } else {
                        ProposedAction::Pursue {
                            intention: IntentionGoal::Work,
                        }
                    });
                }
                GoalTarget::Talk { resident } => {
                    if let Some(companion) = observation
                        .visible_agents
                        .iter()
                        .find(|companion| companion.id == resident)
                        && observation.action_affordances.talk_to.contains(&resident)
                    {
                        return Ok(talk(observation, companion));
                    }
                }
                GoalTarget::Visit { destination } => {
                    return Ok(
                        if observation
                            .action_affordances
                            .move_to
                            .contains(&destination)
                        {
                            ProposedAction::Move { destination }
                        } else {
                            ProposedAction::Pursue {
                                intention: IntentionGoal::Visit { destination },
                            }
                        },
                    );
                }
                GoalTarget::Purchase { location } => {
                    if observation.current_location.id == location {
                        if observation.action_affordances.can_purchase {
                            return Ok(ProposedAction::Purchase);
                        }
                        continue;
                    }
                    if observation
                        .route_hints
                        .purchase
                        .is_some_and(|route| route.destination == location)
                    {
                        return Ok(ProposedAction::Pursue {
                            intention: IntentionGoal::Purchase {
                                destination: location,
                            },
                        });
                    }
                    continue;
                }
                GoalTarget::Rest { home } => {
                    return Ok(
                        if observation.current_location.id == home
                            && observation.action_affordances.can_rest
                        {
                            ProposedAction::Rest
                        } else {
                            ProposedAction::Pursue {
                                intention: IntentionGoal::Rest,
                            }
                        },
                    );
                }
                _ => {}
            }
        }

        if (needs.money < 0.75 || needs.status < 0.75)
            && let Some(workplace) = &observation.self_description.workplace
            && workplace.is_open
            && workplace.business.is_some_and(Business::solvent)
        {
            return Ok(if observation.action_affordances.can_work {
                ProposedAction::Work
            } else {
                follow_route(observation.route_hints.workplace, IntentionGoal::Work)
            });
        }

        if !observation
            .town_event
            .as_ref()
            .is_some_and(|event| event.kind == TownEventKind::Shortage)
        {
            let inventory = observation.self_description.inventory;
            let reserve = if inventory.meals < 2 {
                Some(Offering::Meal)
            } else if inventory.supplies == 0 {
                Some(Offering::Supplies)
            } else if inventory.repair_kits == 0 {
                Some(Offering::Repairs)
            } else {
                None
            };
            if let Some(action) =
                reserve.and_then(|offering| purchase_action(observation, offering))
            {
                return Ok(action);
            }
        }

        let weights = [
            0.5 + personality.openness + personality.impulsiveness + mood.max(0.0),
            0.5 + personality.agreeableness + mood.max(0.0),
            0.5 + personality.openness + personality.neuroticism + (-mood).max(0.0),
            1.5 - personality.impulsiveness + (-mood).max(0.0),
        ];
        let mut choice = rng.random::<f32>() * weights.iter().sum::<f32>();
        let action = weights
            .iter()
            .position(|weight| {
                choice -= weight;
                choice <= 0.0
            })
            .unwrap_or(3);

        Ok(match action {
            0 if !observation.action_affordances.move_to.is_empty() => {
                let index = rng.random_range(0..observation.action_affordances.move_to.len());
                ProposedAction::Move {
                    destination: observation.action_affordances.move_to[index],
                }
            }
            1 => preferred_companion(observation, false).map_or_else(
                || ProposedAction::Observe {
                    target: ObservationTarget::Location(observation.current_location.id),
                },
                |companion| talk(observation, companion),
            ),
            2 => {
                let target = if observation.visible_agents.is_empty() || rng.random_bool(0.5) {
                    ObservationTarget::Location(observation.current_location.id)
                } else {
                    let index = rng.random_range(0..observation.visible_agents.len());
                    ObservationTarget::Agent(observation.visible_agents[index].id)
                };
                ProposedAction::Observe { target }
            }
            _ => ProposedAction::Wait,
        })
    }
}

impl DecisionEngine for RandomDecisionEngine {
    async fn decide(&mut self, observation: &AgentObservation) -> Result<Decision, DecisionError> {
        RandomDecisionEngine::decide(self, observation)
            .await
            .map(Decision::local)
    }
}

fn purchase_action(observation: &AgentObservation, offering: Offering) -> Option<ProposedAction> {
    if observation.action_affordances.can_purchase
        && observation
            .current_location
            .business
            .is_some_and(|business| business.offering == offering)
    {
        return Some(ProposedAction::Purchase);
    }
    observation
        .route_hints
        .purchase
        .filter(|hint| {
            observation
                .action_affordances
                .move_to
                .contains(&hint.next_hop)
        })
        .map(|hint| ProposedAction::Pursue {
            intention: IntentionGoal::Purchase {
                destination: hint.destination,
            },
        })
}

fn work_action(observation: &AgentObservation) -> Option<ProposedAction> {
    observation
        .self_description
        .workplace
        .as_ref()
        .filter(|workplace| workplace.is_open && workplace.business.is_some_and(Business::solvent))
        .map(|_| {
            if observation.action_affordances.can_work {
                ProposedAction::Work
            } else {
                ProposedAction::Pursue {
                    intention: IntentionGoal::Work,
                }
            }
        })
}

fn talk(observation: &AgentObservation, companion: &VisibleAgent) -> ProposedAction {
    let tone = dialogue_tone(observation, companion);
    let personality = &observation.self_description.personality;
    let dominant = [
        personality.openness,
        personality.agreeableness,
        personality.neuroticism,
        personality.honesty,
        personality.ambition,
        personality.impulsiveness,
    ]
    .into_iter()
    .enumerate()
    .max_by(|left, right| left.1.total_cmp(&right.1))
    .expect("personality has traits")
    .0;
    let alternate = observation.tick.0.is_multiple_of(2);
    let name = &companion.name;
    let location = &observation.current_location.name;

    let message = match (tone, dominant, alternate) {
        (DialogueTone::Friendly, _, true) => format!("It's good to see you, {name}."),
        (DialogueTone::Friendly, _, false) => format!("How are you doing today, {name}?"),
        (DialogueTone::Supportive, _, true) => format!("How can I help you today, {name}?"),
        (DialogueTone::Supportive, _, false) => {
            format!("I'm here if you need support around {location}, {name}.")
        }
        (DialogueTone::Tense, _, true) => format!("We need to clear something up, {name}."),
        (DialogueTone::Tense, _, false) => format!("I don't trust this situation, {name}."),
        (DialogueTone::Neutral, 0, true) => {
            format!("What have you noticed around {location}, {name}?")
        }
        (DialogueTone::Neutral, 0, false) => {
            format!("What else might be worth exploring, {name}?")
        }
        (DialogueTone::Neutral, 1, true) => format!("It's good to see you, {name}."),
        (DialogueTone::Neutral, 1, false) => format!("How are you doing today, {name}?"),
        (DialogueTone::Neutral, 2, true) => {
            format!("Does everything seem all right here, {name}?")
        }
        (DialogueTone::Neutral, 2, false) => {
            format!("Do you feel safe around {location}, {name}?")
        }
        (DialogueTone::Neutral, 3, true) => format!("I'd value your honest opinion, {name}."),
        (DialogueTone::Neutral, 3, false) => {
            format!("Let me speak plainly, {name}: how are things?")
        }
        (DialogueTone::Neutral, 4, true) => format!("What are you working toward, {name}?"),
        (DialogueTone::Neutral, 4, false) => format!("How is your work going, {name}?"),
        (DialogueTone::Neutral, 5, true) => format!("What should we do next, {name}?"),
        (DialogueTone::Neutral, 5, false) => {
            format!("Let's try something different today, {name}.")
        }
        _ => unreachable!(),
    };
    ProposedAction::Talk {
        target: companion.id,
        tone,
        message,
    }
}

fn dialogue_tone(observation: &AgentObservation, companion: &VisibleAgent) -> DialogueTone {
    let personality = &observation.self_description.personality;
    let relationship = companion.relationship;
    let belief = observation
        .beliefs
        .get(&companion.id)
        .copied()
        .unwrap_or_default();
    let closeness = relationship.score() - relationship.fear;

    if closeness < -0.5
        || belief.hostility * belief.confidence > 0.35
        || (observation.self_description.mood < -0.5 && personality.agreeableness < 0.7)
        || (personality.impulsiveness > 0.75 && personality.agreeableness < 0.35)
    {
        DialogueTone::Tense
    } else if belief.reliability * belief.confidence > 0.35
        || personality.agreeableness + personality.honesty >= 1.25
    {
        DialogueTone::Supportive
    } else if observation.self_description.mood > 0.35
        || closeness > 0.4
        || personality.agreeableness >= 0.55
    {
        DialogueTone::Friendly
    } else {
        DialogueTone::Neutral
    }
}

#[cfg(test)]
mod tests {
    use super::RandomDecisionEngine;
    use crate::{
        cognition::{ConfrontationAffordance, RumorSummary, TownEventObservation, perceive},
        sim::{
            Belief, DialogueTone, EventId, Goal, GoalKind, GoalTarget, IntentionGoal, Offering,
            ProposedAction, Relationship, Tick, World,
        },
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn town_events_drive_shelter_socializing_and_work() {
        let mut storm = World::briar_glen(0).expect("town");
        storm
            .advance_to(Tick(8 * 60 / Tick::MINUTES))
            .expect("storm");
        let actor = *storm.agents.keys().next().expect("resident");
        let mut observation = perceive(&storm, actor).expect("observation");
        observation.self_description.inventory.repair_kits = 1;
        observation.action_affordances.can_use_repair_kit = true;
        assert_eq!(
            RandomDecisionEngine::new(0)
                .decide(&observation)
                .await
                .expect("storm supplies"),
            ProposedAction::UseRepairKit
        );
        observation.self_description.inventory.repair_kits = 0;
        observation.action_affordances.can_use_repair_kit = false;
        assert_eq!(
            RandomDecisionEngine::new(0)
                .decide(&observation)
                .await
                .expect("storm shelter"),
            ProposedAction::Rest
        );

        let mut festival = World::briar_glen(1).expect("town");
        festival
            .advance_to(Tick(9 * 60 / Tick::MINUTES))
            .expect("festival");
        let actor = *festival.agents.keys().next().expect("resident");
        let mut observation = perceive(&festival, actor).expect("observation");
        observation.self_description.needs = crate::sim::Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.goals.clear();
        assert!(matches!(
            RandomDecisionEngine::new(1)
                .decide(&observation)
                .await
                .expect("festival decision"),
            ProposedAction::Talk { .. }
        ));
        for visible in &mut observation.visible_agents {
            visible.last_conversation = Some(observation.tick);
        }
        assert!(!matches!(
            RandomDecisionEngine::new(1)
                .decide(&observation)
                .await
                .expect("paced festival decision"),
            ProposedAction::Talk { .. }
        ));

        let mut market = World::briar_glen(3).expect("town");
        market
            .advance_to(Tick(11 * 60 / Tick::MINUTES))
            .expect("market day");
        let actor = *market
            .agents
            .iter()
            .find(|(_, agent)| agent.workplace.is_some())
            .expect("worker")
            .0;
        let mut observation = perceive(&market, actor).expect("observation");
        observation.self_description.needs = crate::sim::Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        assert!(matches!(
            RandomDecisionEngine::new(3)
                .decide(&observation)
                .await
                .expect("market decision"),
            ProposedAction::Work
                | ProposedAction::Pursue {
                    intention: IntentionGoal::Work
                }
        ));
    }

    #[tokio::test]
    async fn inventory_is_used_for_urgent_needs_and_stocked_outside_shortages() {
        let world = World::briar_glen(17).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs.food = 0.1;
        observation.self_description.inventory.meals = 1;
        observation.action_affordances.can_consume_meal = true;
        assert_eq!(
            RandomDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("meal"),
            ProposedAction::ConsumeMeal
        );

        observation.self_description.needs.food = 1.0;
        observation.self_description.needs.energy = 1.0;
        observation.self_description.needs.companionship = 1.0;
        observation.self_description.needs.safety = 0.1;
        observation.self_description.inventory.repair_kits = 1;
        observation.action_affordances.can_use_repair_kit = true;
        assert_eq!(
            RandomDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("repair"),
            ProposedAction::UseRepairKit
        );

        observation.self_description.needs.safety = 0.3;
        observation.self_description.inventory.repair_kits = 0;
        observation.self_description.inventory.supplies = 1;
        observation.action_affordances.can_use_repair_kit = false;
        observation.action_affordances.can_use_supplies = true;
        assert_eq!(
            RandomDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("supplies"),
            ProposedAction::UseSupplies
        );

        observation.self_description.needs = crate::sim::Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.self_description.inventory = Default::default();
        observation.action_affordances.can_use_supplies = false;
        observation.goals.clear();
        assert!(matches!(
            RandomDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("reserve"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Purchase { .. }
            }
        ));

        observation.town_event = Some(TownEventObservation {
            kind: crate::sim::TownEventKind::Shortage,
            remaining_ticks: 10,
        });
        assert!(!matches!(
            RandomDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("shortage"),
            ProposedAction::Purchase
                | ProposedAction::Pursue {
                    intention: IntentionGoal::Purchase { .. }
                }
        ));
    }

    #[tokio::test]
    async fn confident_beliefs_shape_companion_and_tone() {
        let world = World::briar_glen(17).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs.food = 0.5;
        observation.self_description.needs.energy = 0.5;
        observation.self_description.needs.companionship = 0.1;
        observation.self_description.needs.safety = 0.5;
        for visible in &mut observation.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        let preferred = observation.visible_agents[0].id;
        observation.beliefs.insert(
            preferred,
            Belief {
                sociability: 1.0,
                reliability: 1.0,
                hostility: 0.0,
                confidence: 1.0,
            },
        );
        let mut engine = RandomDecisionEngine::new(17);
        assert!(matches!(
            engine.decide(&observation).await.expect("decision"),
            ProposedAction::Talk { target, .. } if target == preferred
        ));

        observation
            .visible_agents
            .retain(|agent| agent.id == preferred);
        observation.beliefs.insert(
            preferred,
            Belief {
                sociability: 0.5,
                reliability: 0.5,
                hostility: 1.0,
                confidence: 1.0,
            },
        );
        assert!(matches!(
            engine.decide(&observation).await.expect("decision"),
            ProposedAction::Talk {
                tone: DialogueTone::Tense,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn credible_rumors_trigger_confrontations() {
        let world = World::briar_glen(18).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs.food = 0.5;
        observation.self_description.needs.energy = 0.5;
        observation.self_description.needs.companionship = 0.5;
        observation.self_description.needs.safety = 0.5;
        observation.self_description.needs.status = 0.5;
        let target = observation.visible_agents[0].id;
        let claim = EventId(Uuid::nil());
        observation.action_affordances.confront = vec![ConfrontationAffordance { target, claim }];
        observation.rumors = vec![RumorSummary {
            claim,
            subject: Some(target),
            report: "A known report".into(),
            source: "A resident".into(),
            depth: 1,
            confidence: 0.8,
            resolved: false,
        }];

        assert_eq!(
            RandomDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("decision"),
            ProposedAction::Confront { target, claim }
        );
    }

    #[tokio::test]
    async fn marketplace_choices_follow_the_need_each_offering_satisfies() {
        let mut world = World::briar_glen(23).expect("town");
        world.advance_to(Tick(8 * 12)).expect("business hours");
        let actor = *world.agents.keys().next().expect("resident");
        world.agents.get_mut(&actor).expect("resident").balance = 100;
        let mut engine = RandomDecisionEngine::new(23);

        for (offering, food, safety, status) in [
            (Offering::Meal, 0.1, 1.0, 1.0),
            (Offering::Repairs, 1.0, 0.1, 1.0),
            (Offering::Supplies, 1.0, 0.3, 1.0),
            (Offering::CivicServices, 1.0, 1.0, 0.1),
        ] {
            let needs = &mut world.agents.get_mut(&actor).expect("resident").needs;
            needs.food = food;
            needs.energy = 1.0;
            needs.companionship = 1.0;
            needs.safety = safety;
            needs.status = status;
            let observation = perceive(&world, actor).expect("observation");
            let destination = observation
                .route_hints
                .purchase
                .expect("market route")
                .destination;
            assert_eq!(
                world.locations[&destination]
                    .business
                    .expect("business")
                    .offering,
                offering
            );
            assert_eq!(
                engine.decide(&observation).await.expect("decision"),
                ProposedAction::Pursue {
                    intention: IntentionGoal::Purchase { destination },
                }
            );
        }
    }

    #[tokio::test]
    async fn routines_follow_multi_hop_route_hints() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = world
            .agents
            .values()
            .find(|agent| agent.name == "Clara Voss")
            .map(|agent| agent.id)
            .expect("resident");
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 1.0;
        agent.needs.companionship = 1.0;
        agent.needs.safety = 1.0;
        agent.needs.status = 1.0;
        agent.personality.ambition = 1.0;
        agent.personality.openness = 0.0;
        agent.personality.agreeableness = 0.0;
        agent.personality.neuroticism = 0.0;
        agent.personality.impulsiveness = 0.0;
        world.advance_to(Tick(8 * 12)).expect("morning");

        let observation = perceive(&world, actor).expect("observation");
        let next_hop = observation.route_hints.workplace.expect("route");
        assert_ne!(next_hop.next_hop, next_hop.destination);
        assert_eq!(
            next_hop.destination,
            observation
                .self_description
                .workplace
                .as_ref()
                .expect("workplace")
                .id
        );
        assert_eq!(
            RandomDecisionEngine::new(7)
                .decide(&observation)
                .await
                .expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );
    }

    #[tokio::test]
    async fn urgent_needs_and_time_drive_routines() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut engine = RandomDecisionEngine::new(7);

        let hungry = perceive(&world, actor).expect("observation");
        assert!(matches!(
            engine.decide(&hungry).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Purchase { .. }
            }
        ));
        let mut tired = hungry.clone();
        tired.self_description.needs.food = 0.5;
        tired.self_description.needs.energy = 0.1;
        assert_eq!(
            engine.decide(&tired).await.expect("decision"),
            ProposedAction::Rest
        );
        let mut broke = hungry.clone();
        broke.self_description.balance = 0;
        broke.self_description.needs.food = 0.5;
        broke.self_description.needs.energy = 0.5;
        broke.self_description.needs.companionship = 0.5;
        broke.self_description.needs.safety = 0.5;
        broke.self_description.needs.status = 0.5;
        assert_eq!(
            engine.decide(&broke).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );
        let mut insolvent = broke.clone();
        insolvent
            .self_description
            .workplace
            .as_mut()
            .expect("workplace")
            .business
            .as_mut()
            .expect("ledger")
            .cash = 0;
        assert!(!matches!(
            engine.decide(&insolvent).await.expect("decision"),
            ProposedAction::Work
                | ProposedAction::Pursue {
                    intention: IntentionGoal::Work
                }
        ));

        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 0.5;
        agent.needs.companionship = 0.5;
        agent.needs.safety = 0.5;
        agent.needs.status = 1.0;
        agent.personality.openness = 0.0;
        agent.personality.agreeableness = 0.0;
        agent.personality.neuroticism = 0.0;
        agent.personality.ambition = 1.0;
        agent.personality.impulsiveness = 0.0;
        world.advance_to(Tick(8 * 12)).expect("morning");
        let working = perceive(&world, actor).expect("observation");
        let workplace = working
            .self_description
            .workplace
            .as_ref()
            .expect("workplace")
            .id;
        assert_eq!(
            engine.decide(&working).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );

        world.execute(
            actor,
            ProposedAction::Move {
                destination: workplace,
            },
        );
        assert_eq!(
            engine
                .decide(&perceive(&world, actor).expect("work observation"))
                .await
                .expect("decision"),
            ProposedAction::Work
        );
        world.advance_to(Tick(21 * 12)).expect("night");
        let heading_home = perceive(&world, actor).expect("observation");
        assert_eq!(
            engine.decide(&heading_home).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Rest,
            }
        );

        let mut lonely = hungry;
        lonely.tick = Tick(12 * 12);
        lonely.self_description.needs.food = 0.5;
        lonely.self_description.needs.energy = 0.5;
        lonely.self_description.needs.companionship = 0.1;
        lonely.self_description.needs.safety = 0.5;
        for visible in &mut lonely.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        let preferred = lonely.visible_agents[0].id;
        lonely.visible_agents[0].relationship.affection = 1.0;
        let companion_name = lonely.visible_agents[0].name.clone();
        assert!(matches!(
            engine.decide(&lonely).await.expect("decision"),
            ProposedAction::Talk { target, message, .. }
                if target == preferred && message.contains(&companion_name)
        ));

        let mut purposeful = lonely;
        purposeful.self_description.needs.companionship = 0.5;
        purposeful.self_description.needs.status = 0.5;
        let destination = purposeful.action_affordances.move_to[0];
        purposeful.goals = vec![Goal::new(
            "Visit somewhere specific",
            GoalKind::Exploration,
            GoalTarget::Visit { destination },
            1,
            Tick(999),
        )];
        assert_eq!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Move { destination }
        );

        purposeful.goals = vec![Goal::new(
            "Talk to a specific resident",
            GoalKind::Community,
            GoalTarget::Talk {
                resident: preferred,
            },
            1,
            Tick(999),
        )];
        let personality = &mut purposeful.self_description.personality;
        personality.openness = 0.0;
        personality.agreeableness = 1.0;
        personality.neuroticism = 0.0;
        personality.ambition = 0.0;
        personality.impulsiveness = 0.0;
        assert!(matches!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Talk {
                target,
                tone: DialogueTone::Supportive,
                message,
                ..
            } if target == preferred && message.contains(&companion_name)
        ));
        for visible in &mut purposeful.visible_agents {
            visible.relationship = Relationship::NEUTRAL;
        }
        purposeful.self_description.personality.agreeableness = 0.6;
        purposeful.self_description.personality.honesty = 0.4;
        purposeful.self_description.mood = -1.0;
        assert!(matches!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Talk {
                target,
                tone: DialogueTone::Tense,
                ..
            } if target == preferred
        ));

        purposeful.self_description.mood = 0.0;
        purposeful.self_description.personality.agreeableness = 0.0;
        purposeful.self_description.personality.ambition = 1.0;
        let workplace = purposeful
            .self_description
            .workplace
            .as_ref()
            .expect("workplace")
            .id;
        purposeful.goals = vec![Goal::new(
            "Work somewhere specific",
            GoalKind::Livelihood,
            GoalTarget::Work { workplace },
            1,
            Tick(999),
        )];
        assert_eq!(
            engine.decide(&purposeful).await.expect("decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );
    }

    #[tokio::test]
    async fn medical_needs_prioritize_medicine_and_clinic_treatment() {
        let mut world = World::briar_glen(18).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        world.advance_to(Tick(8 * 12)).expect("clinic opening");
        world.agents.get_mut(&actor).expect("resident").injury = true;
        world.agents.get_mut(&actor).expect("resident").health = 0.4;
        world.agents.get_mut(&actor).expect("resident").balance = 100;
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.inventory.medicine = 1;
        observation.action_affordances.can_use_medicine = true;
        assert_eq!(
            RandomDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("medicine decision"),
            ProposedAction::UseMedicine
        );

        observation.self_description.inventory.medicine = 0;
        observation.action_affordances.can_use_medicine = false;
        assert!(matches!(
            RandomDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("clinic route decision"),
            ProposedAction::Pursue {
                intention: IntentionGoal::SeekTreatment
            }
        ));
        let clinic = world.clinic_location().expect("clinic");
        world.relocate(actor, clinic);
        let observation = perceive(&world, actor).expect("clinic observation");
        assert!(observation.action_affordances.can_seek_treatment);
        assert_eq!(
            RandomDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("treatment decision"),
            ProposedAction::SeekTreatment
        );
    }
}
