use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::{AgentObservation, RouteHint, StealAffordance, VisibleAgent},
    sim::{
        AgentId, Business, Decision, DialogueTone, GoalKind, GoalTarget, HealthCondition,
        IntentionGoal, Item, Loot, ObservationTarget, Occupation, Offering, ProposedAction, Tick,
        TownEventKind,
    },
};
use rand::{Rng, SeedableRng, rngs::StdRng};

const RECENT_CONVERSATION_TICKS: u64 = Tick::PER_DAY / 4;

fn loot_value(loot: Loot) -> u32 {
    match loot {
        Loot::Coins(amount) => amount as u32,
        Loot::Item(Item::Meal) => 5,
        Loot::Item(Item::Supplies) => 6,
        Loot::Item(Item::RepairKit) => 8,
        Loot::Item(Item::Medicine) => 12,
    }
}

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

pub struct LocalDecisionEngine {
    seed: u64,
}

impl LocalDecisionEngine {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl LocalDecisionEngine {
    fn choose(&mut self, observation: &AgentObservation) -> Result<ProposedAction, DecisionError> {
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
        let rest_at_home = || {
            if observation.current_location.id == observation.self_description.home.id {
                ProposedAction::Rest
            } else {
                follow_route(observation.route_hints.home, IntentionGoal::Rest)
            }
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
        if observation.self_description.health < 0.25 {
            if observation.action_affordances.can_use_repair_kit {
                return Ok(ProposedAction::UseRepairKit);
            }
            return Ok(rest_at_home());
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
            return Ok(rest_at_home());
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

        // The sheriff enforces the law during work hours: arrest the best-credentialed
        // claim (highest confidence, lowest claim id on ties). Only claims exposed by
        // the arrest affordance are legal; the world stays authoritative. The existing
        // confront branch handles investigation, and patrol replaces the idle fallback.
        if observation.self_description.occupation == Occupation::Sheriff
            && (7..21).contains(&observation.local_time.hour)
            && !observation.action_affordances.arrest.is_empty()
        {
            let best = observation
                .action_affordances
                .arrest
                .iter()
                .max_by(|left, right| {
                    left.confidence
                        .total_cmp(&right.confidence)
                        .then_with(|| right.claim.cmp(&left.claim))
                })
                .expect("non-empty arrest affordances");
            return Ok(ProposedAction::Arrest {
                target: best.target,
                claim: best.claim,
            });
        }
        if needs.energy < 0.2 + 0.1 * personality.neuroticism {
            return Ok(rest_at_home());
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

        // Crime: desperate, impulsive, dishonest residents steal; aggrieved,
        // hostile residents attack. Both branches are observation-only, pick
        // deterministically (no rng), and are paced by a cooldown derived from the
        // crime event log, so checkpoint resumes match uninterrupted runs.
        // ponytail: gates tuned to briar_glen's personality spread (honesty ~0.5..0.9,
        // agreeableness ~0.4..0.85): the caps admit only the less honest / less
        // agreeable residents, and CRIME_COOLDOWN_TICKS brakes repeat offenses. Baseline
        // 10 seeds x 30 days at these values landed ~67 thefts and ~2 assaults per seed
        // (>1 of each across seeds). Confidence decay was slowed to 0.0002/tick so
        // hostility beliefs survive days (see agent.rs decay_beliefs). Re-run baseline
        // when the town roster changes. Upgrade path: move caps into town definition
        // data once several towns exist.
        if (needs.money < 0.35 || needs.food < 0.3)
            && personality.honesty < 0.75
            && personality.impulsiveness >= 0.4
            && observation
                .self_description
                .next_crime
                .is_none_or(|until| observation.tick >= until)
            && !observation.action_affordances.steal_from.is_empty()
        {
            let occupied = |affordance: &StealAffordance| {
                observation
                    .visible_agents
                    .iter()
                    .find(|visible| visible.id == affordance.target)
                    .is_some_and(|visible| visible.activity.is_some())
            };
            if let Some(best) =
                observation
                    .action_affordances
                    .steal_from
                    .iter()
                    .max_by(|left, right| {
                        occupied(left)
                            .cmp(&occupied(right))
                            .then_with(|| loot_value(left.loot).cmp(&loot_value(right.loot)))
                            .then_with(|| right.target.cmp(&left.target))
                    })
            {
                return Ok(ProposedAction::Steal {
                    target: best.target,
                    loot: best.loot,
                });
            }
        }
        if personality.agreeableness < 0.6
            && mood < 0.1
            && observation
                .self_description
                .next_crime
                .is_none_or(|until| observation.tick >= until)
            && !observation.action_affordances.attack.is_empty()
        {
            let hostile = |target: AgentId| {
                observation
                    .beliefs
                    .get(&target)
                    .is_some_and(|belief| belief.hostility >= 0.6 && belief.confidence >= 0.35)
            };
            if let Some(target) = observation
                .action_affordances
                .attack
                .iter()
                .copied()
                .filter(|target| hostile(*target))
                .max_by(|left, right| {
                    // Deterministic: most hostile belief wins, lowest id breaks ties.
                    let hostility = |id: AgentId| observation.beliefs[&id].hostility;
                    hostility(*left)
                        .total_cmp(&hostility(*right))
                        .then_with(|| right.cmp(left))
                })
            {
                return Ok(ProposedAction::Attack { target });
            }
        }

        if let Some(aid) = observation
            .action_affordances
            .give
            .iter()
            .filter(|aid| observation.self_description.inventory.count(aid.item) > 1)
            .filter(|aid| {
                observation
                    .visible_agents
                    .iter()
                    .find(|visible| visible.id == aid.target)
                    .is_some_and(|visible| {
                        personality.agreeableness + visible.relationship.score() >= 0.6
                    })
            })
            .max_by(|left, right| {
                let score = |target| {
                    observation
                        .visible_agents
                        .iter()
                        .find(|visible| visible.id == target)
                        .map_or(f32::MIN, |visible| visible.relationship.score())
                };
                score(left.target).total_cmp(&score(right.target))
            })
        {
            return Ok(ProposedAction::Give {
                target: aid.target,
                item: aid.item,
            });
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
            return Ok(rest_at_home());
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

        let shortage = observation
            .town_event
            .as_ref()
            .is_some_and(|event| event.kind == TownEventKind::Shortage);
        if let Some(action) = observation
            .self_description
            .inventory
            .reserve_offering(shortage)
            .and_then(|offering| purchase_action(observation, offering))
        {
            return Ok(action);
        }

        let weights = [
            0.5 + personality.openness + personality.impulsiveness + mood.max(0.0),
            0.5 + personality.agreeableness + mood.max(0.0),
            0.5 + personality.openness + personality.neuroticism + (-mood).max(0.0),
            1.5 - personality.impulsiveness + (-mood).max(0.0),
        ];

        // The sheriff patrols open connected locations instead of idling during
        // work hours; at night they behave like anyone else. The rotation is a pure
        // function of the tick (no engine state), so checkpoint resumes pick the
        // same routes as uninterrupted runs.
        if observation.self_description.occupation == Occupation::Sheriff
            && (7..21).contains(&observation.local_time.hour)
            && !observation.action_affordances.move_to.is_empty()
        {
            let destinations = &observation.action_affordances.move_to;
            let destination = destinations[observation.tick.0 as usize % destinations.len()];
            return Ok(ProposedAction::Move { destination });
        }

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

impl DecisionEngine for LocalDecisionEngine {
    async fn decide(&mut self, observation: &AgentObservation) -> Result<Decision, DecisionError> {
        self.choose(observation).map(Decision::local)
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
    use super::LocalDecisionEngine;
    use crate::{
        cognition::{ConfrontationAffordance, RumorSummary, TownEventObservation, perceive},
        decision::DecisionEngine,
        sim::{
            ActionResult, Activity, ActivityKind, Belief, DialogueTone, EventId, Goal, GoalKind,
            GoalTarget, IntentionGoal, Item, Loot, Needs, Occupation, Offering, ProposedAction,
            Relationship, Tick, World,
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
            LocalDecisionEngine::new(0)
                .decide(&observation)
                .await
                .expect("storm supplies")
                .action,
            ProposedAction::UseRepairKit
        );
        observation.self_description.inventory.repair_kits = 0;
        observation.action_affordances.can_use_repair_kit = false;
        assert_eq!(
            LocalDecisionEngine::new(0)
                .decide(&observation)
                .await
                .expect("storm shelter")
                .action,
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
            LocalDecisionEngine::new(1)
                .decide(&observation)
                .await
                .expect("festival decision")
                .action,
            ProposedAction::Talk { .. }
        ));
        for visible in &mut observation.visible_agents {
            visible.last_conversation = Some(observation.tick);
        }
        assert!(!matches!(
            LocalDecisionEngine::new(1)
                .decide(&observation)
                .await
                .expect("paced festival decision")
                .action,
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
            LocalDecisionEngine::new(3)
                .decide(&observation)
                .await
                .expect("market decision")
                .action,
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
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("meal")
                .action,
            ProposedAction::ConsumeMeal
        );

        observation.self_description.needs.food = 1.0;
        observation.self_description.needs.energy = 1.0;
        observation.self_description.needs.companionship = 1.0;
        observation.self_description.needs.safety = 0.1;
        observation.self_description.inventory.repair_kits = 1;
        observation.action_affordances.can_use_repair_kit = true;
        assert_eq!(
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("repair")
                .action,
            ProposedAction::UseRepairKit
        );

        observation.self_description.needs.safety = 0.3;
        observation.self_description.inventory.repair_kits = 0;
        observation.self_description.inventory.supplies = 1;
        observation.action_affordances.can_use_repair_kit = false;
        observation.action_affordances.can_use_supplies = true;
        assert_eq!(
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("supplies")
                .action,
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
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("reserve")
                .action,
            ProposedAction::Pursue {
                intention: IntentionGoal::Purchase { .. }
            }
        ));

        observation.town_event = Some(TownEventObservation {
            kind: crate::sim::TownEventKind::Shortage,
            remaining_ticks: 10,
        });
        assert!(!matches!(
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("shortage")
                .action,
            ProposedAction::Purchase
                | ProposedAction::Pursue {
                    intention: IntentionGoal::Purchase { .. }
                }
        ));
    }

    #[tokio::test]
    async fn agreeable_residents_offer_useful_surplus_inventory() {
        let mut world = World::briar_glen(17).expect("town");
        let residents = world.agents.keys().copied().take(2).collect::<Vec<_>>();
        let actor = residents[0];
        let receiver = residents[1];
        let location = world.agents[&actor].location;
        world.relocate(receiver, location);
        world.agents.get_mut(&actor).expect("actor").inventory.meals = 2;
        world
            .agents
            .get_mut(&receiver)
            .expect("receiver")
            .needs
            .food = 0.1;
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.needs = crate::sim::Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.self_description.personality.agreeableness = 1.0;

        assert_eq!(
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("aid")
                .action,
            ProposedAction::Give {
                target: receiver,
                item: Item::Meal,
            }
        );

        observation.self_description.inventory.meals = 1;
        assert!(!matches!(
            LocalDecisionEngine::new(17)
                .decide(&observation)
                .await
                .expect("keep reserve")
                .action,
            ProposedAction::Give { .. }
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
        let mut engine = LocalDecisionEngine::new(17);
        assert!(matches!(
            engine.decide(&observation).await.expect("decision").action,
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
            engine.decide(&observation).await.expect("decision").action,
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
            LocalDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("decision")
                .action,
            ProposedAction::Confront { target, claim }
        );
    }

    #[tokio::test]
    async fn marketplace_choices_follow_the_need_each_offering_satisfies() {
        let mut world = World::briar_glen(23).expect("town");
        world.advance_to(Tick(8 * 12)).expect("business hours");
        let actor = *world.agents.keys().next().expect("resident");
        world.agents.get_mut(&actor).expect("resident").balance = 100;
        let mut engine = LocalDecisionEngine::new(23);

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
                engine.decide(&observation).await.expect("decision").action,
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
            LocalDecisionEngine::new(7)
                .decide(&observation)
                .await
                .expect("decision")
                .action,
            ProposedAction::Pursue {
                intention: IntentionGoal::Work,
            }
        );
    }

    #[tokio::test]
    async fn urgent_needs_and_time_drive_routines() {
        let mut world = World::briar_glen(7).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut engine = LocalDecisionEngine::new(7);

        let hungry = perceive(&world, actor).expect("observation");
        assert!(matches!(
            engine.decide(&hungry).await.expect("decision").action,
            ProposedAction::Pursue {
                intention: IntentionGoal::Purchase { .. }
            }
        ));
        let mut tired = hungry.clone();
        tired.self_description.needs.food = 0.5;
        tired.self_description.needs.energy = 0.1;
        assert_eq!(
            engine.decide(&tired).await.expect("decision").action,
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
            engine.decide(&broke).await.expect("decision").action,
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
            engine.decide(&insolvent).await.expect("decision").action,
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
            engine.decide(&working).await.expect("decision").action,
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
                .expect("decision")
                .action,
            ProposedAction::Work
        );
        world.advance_to(Tick(21 * 12)).expect("night");
        let heading_home = perceive(&world, actor).expect("observation");
        assert_eq!(
            engine.decide(&heading_home).await.expect("decision").action,
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
            engine.decide(&lonely).await.expect("decision").action,
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
            engine.decide(&purposeful).await.expect("decision").action,
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
            engine.decide(&purposeful).await.expect("decision").action,
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
            engine.decide(&purposeful).await.expect("decision").action,
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
            engine.decide(&purposeful).await.expect("decision").action,
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
            LocalDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("medicine decision")
                .action,
            ProposedAction::UseMedicine
        );

        observation.self_description.inventory.medicine = 0;
        observation.action_affordances.can_use_medicine = false;
        assert!(matches!(
            LocalDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("clinic route decision")
                .action,
            ProposedAction::Pursue {
                intention: IntentionGoal::SeekTreatment
            }
        ));
        let clinic = world.clinic_location().expect("clinic");
        world.relocate(actor, clinic);
        let observation = perceive(&world, actor).expect("clinic observation");
        assert!(observation.action_affordances.can_seek_treatment);
        assert_eq!(
            LocalDecisionEngine::new(18)
                .decide(&observation)
                .await
                .expect("treatment decision")
                .action,
            ProposedAction::SeekTreatment
        );
    }

    #[tokio::test]
    async fn low_honesty_broke_resident_steals_and_prefers_occupied_targets() {
        let world = World::briar_glen(41).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.personality.honesty = 0.3;
        observation.self_description.personality.impulsiveness = 0.8;
        // All needs healthy except money so no survival branch steals the turn.
        observation.self_description.needs = Needs {
            money: 0.2,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.goals.clear();
        assert!(!observation.action_affordances.steal_from.is_empty());
        assert!(matches!(
            LocalDecisionEngine::new(41)
                .decide(&observation)
                .await
                .expect("steal decision")
                .action,
            ProposedAction::Steal { .. }
        ));

        // Mark one resident busy: the thief must pick them over idle victims.
        let busy = observation.visible_agents[1].id;
        observation.visible_agents[1].activity = Some(Activity {
            kind: ActivityKind::Resting,
            until: Tick(10_000),
        });
        assert_eq!(
            LocalDecisionEngine::new(41)
                .decide(&observation)
                .await
                .expect("steal decision")
                .action,
            ProposedAction::Steal {
                target: busy,
                loot: Loot::Coins(10)
            }
        );
    }

    #[tokio::test]
    async fn high_honesty_resident_never_steals() {
        let world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.personality.honesty = 1.0;
        observation.self_description.personality.impulsiveness = 0.8;
        observation.self_description.needs = Needs {
            money: 0.2,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.goals.clear();
        assert!(!observation.action_affordances.steal_from.is_empty());
        assert!(!matches!(
            LocalDecisionEngine::new(42)
                .decide(&observation)
                .await
                .expect("honest decision")
                .action,
            ProposedAction::Steal { .. }
        ));
    }

    #[tokio::test]
    async fn hostile_resident_attacks_the_believed_target_only() {
        // A low-agreeableness resident in a bad mood attacks the person they
        // hold a confident high-hostility belief about.
        let mut world = World::briar_glen(43).expect("town");
        world
            .advance_to(Tick(9 * 60 / Tick::MINUTES))
            .expect("morning");
        let actor = *world.agents.keys().next().expect("resident");
        let mut observation = perceive(&world, actor).expect("observation");
        observation.self_description.personality.agreeableness = 0.2;
        observation.self_description.mood = -0.4;
        observation.self_description.needs = Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.goals.clear();
        let target = observation.visible_agents[1].id;
        observation.beliefs.insert(
            target,
            Belief {
                sociability: 0.5,
                reliability: 0.5,
                hostility: 0.9,
                confidence: 0.8,
            },
        );
        assert_eq!(
            LocalDecisionEngine::new(43)
                .decide(&observation)
                .await
                .expect("attack decision")
                .action,
            ProposedAction::Attack { target }
        );

        // A neutral mood means no attack, even with the same belief.
        observation.self_description.mood = 0.1;
        assert!(!matches!(
            LocalDecisionEngine::new(43)
                .decide(&observation)
                .await
                .expect("calm decision")
                .action,
            ProposedAction::Attack { .. }
        ));
    }

    #[tokio::test]
    async fn sheriff_arrests_on_a_legal_witnessed_claim() {
        let mut world = World::briar_glen(41).expect("town");
        let sheriff = world
            .agents
            .values()
            .find(|agent| agent.occupation == Occupation::Sheriff)
            .expect("sheriff")
            .id;
        let thief = world
            .agents
            .values()
            .find(|agent| agent.id != sheriff)
            .expect("thief")
            .id;
        let victim = world
            .agents
            .values()
            .find(|agent| agent.id != sheriff && agent.id != thief)
            .expect("victim")
            .id;
        // Everyone starts idle together at Riverside Houses, so the sheriff
        // witnesses the theft attempt and gains a legal claim.
        assert!(matches!(
            world.execute(
                thief,
                ProposedAction::Steal {
                    target: victim,
                    loot: Loot::Coins(1),
                }
            ),
            ActionResult::Success(_)
        ));
        let mut observation = perceive(&world, sheriff).expect("observation");
        observation.self_description.needs = Needs {
            money: 1.0,
            food: 1.0,
            companionship: 1.0,
            safety: 1.0,
            status: 1.0,
            energy: 1.0,
        };
        observation.goals.clear();
        assert!(!observation.action_affordances.arrest.is_empty());
        let decision = LocalDecisionEngine::new(41)
            .decide(&observation)
            .await
            .expect("sheriff decision");
        assert!(
            matches!(decision.action, ProposedAction::Arrest { target, .. } if target == thief)
        );

        // Without the memory (or a credible rumor) the same observation offers
        // no legal claim, so the sheriff never proposes an arrest.
        world
            .agents
            .get_mut(&sheriff)
            .expect("sheriff")
            .memories
            .clear();
        world
            .agents
            .get_mut(&sheriff)
            .expect("sheriff")
            .rumors
            .clear();
        let bare = perceive(&world, sheriff).expect("observation");
        assert!(bare.action_affordances.arrest.is_empty());
        let decision = LocalDecisionEngine::new(41)
            .decide(&bare)
            .await
            .expect("bare decision");
        assert!(!matches!(decision.action, ProposedAction::Arrest { .. }));
    }

    #[tokio::test]
    async fn sheriff_patrols_during_work_hours_with_a_rotating_cursor() {
        // Find a seed where everything else is blocked: full needs, no goals, no
        // town event, sheriff at work-hour patrol with no legal claims.
        for seed in 0..64u64 {
            let mut world = World::briar_glen(seed).expect("town");
            if world.active_town_event.is_some() {
                continue;
            }
            let sheriff = world
                .agents
                .values()
                .find(|agent| agent.occupation == Occupation::Sheriff)
                .expect("sheriff")
                .id;
            world
                .advance_to(Tick(10 * 60 / Tick::MINUTES))
                .expect("morning");
            let mut observation = perceive(&world, sheriff).expect("observation");
            observation.self_description.needs = Needs {
                money: 1.0,
                food: 1.0,
                companionship: 1.0,
                safety: 1.0,
                status: 1.0,
                energy: 1.0,
            };
            observation.goals.clear();
            // Stocked inventory so the reserve-purchase branch stays quiet too.
            observation.self_description.inventory.meals = 2;
            observation.self_description.inventory.supplies = 1;
            observation.self_description.inventory.repair_kits = 1;
            if observation.town_event.is_none()
                && observation.action_affordances.arrest.is_empty()
                && observation.action_affordances.move_to.len() >= 2
            {
                let destinations = observation.action_affordances.move_to.clone();
                let index = observation.tick.0 as usize % destinations.len();
                let mut engine = LocalDecisionEngine::new(seed);
                assert_eq!(
                    engine.decide(&observation).await.expect("patrol").action,
                    ProposedAction::Move {
                        destination: destinations[index]
                    }
                );
                return;
            }
        }
        panic!("no seed produced a clean sheriff patrol within 64 trials");
    }

    #[tokio::test]
    async fn sheriff_does_not_patrol_at_night() {
        for seed in 0..64u64 {
            let mut world = World::briar_glen(seed).expect("town");
            let sheriff = world
                .agents
                .values()
                .find(|agent| agent.occupation == Occupation::Sheriff)
                .expect("sheriff")
                .id;
            world
                .advance_to(Tick(22 * 60 / Tick::MINUTES))
                .expect("night");
            let mut night = perceive(&world, sheriff).expect("observation");
            night.self_description.needs = Needs {
                money: 1.0,
                food: 1.0,
                companionship: 1.0,
                safety: 1.0,
                status: 1.0,
                energy: 1.0,
            };
            night.goals.clear();
            night.self_description.inventory.meals = 2;
            night.self_description.inventory.supplies = 1;
            night.self_description.inventory.repair_kits = 1;
            if night.town_event.is_none()
                && night.action_affordances.arrest.is_empty()
                && !night.action_affordances.move_to.is_empty()
            {
                // At 22:00 (outside 7..21) the sheriff falls back to plain behavior;
                // the decision is not the deterministic patrol rotation.
                let mut engine = LocalDecisionEngine::new(seed);
                let decision = engine.decide(&night).await.expect("night decision");
                assert!(!matches!(decision.action, ProposedAction::Move { .. }));
                return;
            }
        }
        panic!("no seed produced a clean non-move night decision within 64 trials");
    }

    #[tokio::test]
    async fn non_sheriffs_never_see_arrest_affordances() {
        let mut world = World::briar_glen(41).expect("town");
        let civilian = world
            .agents
            .values()
            .find(|agent| agent.occupation != Occupation::Sheriff)
            .expect("civilian")
            .id;
        let thief = world
            .agents
            .values()
            .find(|agent| agent.id != civilian)
            .expect("thief")
            .id;
        let victim = world
            .agents
            .values()
            .find(|agent| agent.id != civilian && agent.id != thief)
            .expect("victim")
            .id;
        world.execute(
            thief,
            ProposedAction::Steal {
                target: victim,
                loot: Loot::Coins(1),
            },
        );
        let observation = perceive(&world, civilian).expect("observation");
        assert!(observation.action_affordances.arrest.is_empty());
    }
}
