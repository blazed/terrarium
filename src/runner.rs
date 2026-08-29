use crate::{
    cognition::{ObservationError, perceive},
    decision::DecisionEngine,
    sim::{
        ActionResult, AgentId, Decision, DecisionSource, Event, Intention, IntentionGoal,
        ProposedAction, Scheduler, Tick, TownEventKind, World, WorldError,
    },
};
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(transparent)]
    Observation(#[from] ObservationError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmDecisionAudit {
    pub tick: Tick,
    pub resident_id: AgentId,
    pub resident: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProposedAction>,
    pub intention: IntentionGoal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ProposedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

struct IntentionSnapshot {
    id: AgentId,
    resident: String,
    intention: Intention,
    completed: u64,
    interrupted: u64,
}

pub async fn run_simulation(
    world: World,
    ticks: u64,
    engine: &mut impl DecisionEngine,
) -> Result<World, SimulationError> {
    run_simulation_with_audit(world, ticks, engine, |_, _| {}, |_| {}).await
}

pub async fn run_simulation_with_audit(
    mut world: World,
    ticks: u64,
    engine: &mut impl DecisionEngine,
    mut on_event: impl FnMut(&World, &Event),
    mut on_audit: impl FnMut(&LlmDecisionAudit),
) -> Result<World, SimulationError> {
    let scheduler = Scheduler;
    for _ in 0..ticks {
        if !world.agents.values().any(|agent| agent.is_alive()) {
            break;
        }
        let active_intentions = llm_intentions(&world);
        let previous_events = world.events().len();
        world.advance_tick()?;
        for snapshot in active_intentions {
            if !world.agents[&snapshot.id].llm_intention {
                on_audit(&terminal_audit(
                    &world,
                    &snapshot,
                    None,
                    "world_state_changed",
                ));
            }
        }
        for event in &world.events()[previous_events..] {
            on_event(&world, event);
        }
        for agent in scheduler.agents_to_act(&world) {
            let previous_events = world.events().len();
            let active_intention = llm_intention(&world, agent);
            let planned_action = active_intention.as_ref().and_then(|snapshot| {
                world
                    .intention_action(agent, &snapshot.intention)
                    .ok()
                    .flatten()
            });
            let continued = world.continue_intention(agent);
            if let Some(snapshot) = active_intention {
                if let Some(result) = &continued {
                    on_audit(&LlmDecisionAudit {
                        tick: world.tick,
                        resident_id: agent,
                        resident: snapshot.resident.clone(),
                        status: "step",
                        proposal: None,
                        intention: snapshot.intention.goal.clone(),
                        action: planned_action,
                        result: Some(action_result(result)),
                        reason: None,
                    });
                }
                if !world.agents[&agent].llm_intention {
                    on_audit(&terminal_audit(
                        &world,
                        &snapshot,
                        continued.as_ref(),
                        "urgent_need",
                    ));
                }
            }
            if continued.is_none() && world.agents[&agent].intention.is_none() {
                let observation = perceive(&world, agent)?;
                let decision = match engine.decide(&observation).await {
                    Ok(decision) => decision,
                    Err(error) => {
                        warn!(?agent, %error, "decision failed; waiting instead");
                        Decision::local(ProposedAction::Wait)
                    }
                };
                world
                    .agents
                    .get_mut(&agent)
                    .expect("scheduled resident")
                    .routing
                    .record(world.tick, decision.source);
                debug!(?agent, ?decision.action, "executing proposed action");
                let llm_start = (decision.source == DecisionSource::Llm)
                    .then(|| match &decision.action {
                        ProposedAction::Pursue { intention } => Some((
                            decision
                                .llm_proposal
                                .clone()
                                .unwrap_or_else(|| decision.action.clone()),
                            intention.clone(),
                        )),
                        _ => None,
                    })
                    .flatten();
                let before = world.agents[&agent].routing;
                let initial_action = llm_start.as_ref().and_then(|(_, goal)| {
                    world
                        .intention_action(
                            agent,
                            &Intention {
                                goal: goal.clone(),
                                expires_at: world.tick,
                            },
                        )
                        .ok()
                        .flatten()
                });
                let result = world.execute_decision(agent, decision);
                if let Some((proposal, intention)) = &llm_start
                    && world.agents[&agent].routing.llm_intentions_started
                        == before.llm_intentions_started
                {
                    on_audit(&LlmDecisionAudit {
                        tick: world.tick,
                        resident_id: agent,
                        resident: world.agents[&agent].name.clone(),
                        status: "rejected",
                        proposal: Some(proposal.clone()),
                        intention: intention.clone(),
                        action: initial_action.clone(),
                        result: Some(action_result(&result)),
                        reason: Some("action_rejected"),
                    });
                }
                if let Some((proposal, intention)) = llm_start
                    && world.agents[&agent].routing.llm_intentions_started
                        > before.llm_intentions_started
                {
                    let snapshot = IntentionSnapshot {
                        id: agent,
                        resident: world.agents[&agent].name.clone(),
                        intention: Intention {
                            goal: intention.clone(),
                            expires_at: world.agents[&agent]
                                .intention
                                .as_ref()
                                .map_or(Tick(u64::MAX), |active| active.expires_at),
                        },
                        completed: before.llm_intentions_completed,
                        interrupted: before.llm_intentions_interrupted,
                    };
                    let action = if world.agents[&agent].routing.llm_intentions_interrupted
                        > before.llm_intentions_interrupted
                        && matches!(result, ActionResult::Success(_))
                    {
                        Some(ProposedAction::Wait)
                    } else {
                        initial_action
                    };
                    on_audit(&LlmDecisionAudit {
                        tick: world.tick,
                        resident_id: agent,
                        resident: snapshot.resident.clone(),
                        status: "started",
                        proposal: Some(proposal),
                        intention,
                        action,
                        result: Some(action_result(&result)),
                        reason: None,
                    });
                    if !world.agents[&agent].llm_intention {
                        on_audit(&terminal_audit(
                            &world,
                            &snapshot,
                            Some(&result),
                            "urgent_need",
                        ));
                    }
                }
            }
            world.validate()?;
            for event in &world.events()[previous_events..] {
                on_event(&world, event);
            }
        }
    }
    Ok(world)
}

fn llm_intentions(world: &World) -> Vec<IntentionSnapshot> {
    world
        .agents
        .values()
        .filter_map(|agent| llm_intention(world, agent.id))
        .collect()
}

fn llm_intention(world: &World, agent: AgentId) -> Option<IntentionSnapshot> {
    let agent = world.agents.get(&agent)?;
    agent.llm_intention.then(|| IntentionSnapshot {
        id: agent.id,
        resident: agent.name.clone(),
        intention: agent.intention.clone().expect("validated LLM intention"),
        completed: agent.routing.llm_intentions_completed,
        interrupted: agent.routing.llm_intentions_interrupted,
    })
}

fn terminal_audit(
    world: &World,
    snapshot: &IntentionSnapshot,
    result: Option<&ActionResult>,
    fallback_reason: &'static str,
) -> LlmDecisionAudit {
    let agent = &world.agents[&snapshot.id];
    let completed = agent.routing.llm_intentions_completed > snapshot.completed;
    let rejected = result.is_some_and(|result| matches!(result, ActionResult::Rejected(_)));
    let reason = if completed {
        match snapshot.intention.goal {
            IntentionGoal::Rest => "energy_threshold_reached",
            IntentionGoal::Work if snapshot.intention.expires_at <= world.tick => {
                "planned_duration_reached"
            }
            _ => "objective_reached",
        }
    } else if rejected {
        "action_rejected"
    } else if !agent.is_alive() {
        "resident_died"
    } else if world
        .active_town_event
        .is_some_and(|event| event.kind == TownEventKind::Storm)
    {
        "town_disruption"
    } else if snapshot.intention.expires_at <= world.tick {
        "expired"
    } else {
        fallback_reason
    };
    debug_assert!(
        completed || agent.routing.llm_intentions_interrupted > snapshot.interrupted || rejected
    );
    LlmDecisionAudit {
        tick: world.tick,
        resident_id: snapshot.id,
        resident: snapshot.resident.clone(),
        status: if completed {
            "completed"
        } else {
            "interrupted"
        },
        proposal: None,
        intention: snapshot.intention.goal.clone(),
        action: None,
        result: None,
        reason: Some(reason),
    }
}

fn action_result(result: &ActionResult) -> &'static str {
    match result {
        ActionResult::Success(_) => "success",
        ActionResult::Rejected(_) => "rejected",
    }
}

#[cfg(test)]
mod tests {
    use super::{run_simulation, run_simulation_with_audit};
    use crate::{
        cognition::AgentObservation,
        decision::{DecisionEngine, DecisionError, LocalDecisionEngine},
        sim::{
            DeathCause, Decision, EventKind, Intention, IntentionGoal, LifeState, LocationId,
            ProposedAction, Tick, World,
        },
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn simulation_stops_when_every_resident_is_dead() {
        let mut world = World::briar_glen(1).expect("town");
        for location in world.locations.values_mut() {
            location.agents.clear();
        }
        for agent in world.agents.values_mut() {
            agent.health = 0.0;
            agent.life = LifeState::Dead {
                tick: world.tick,
                cause: DeathCause::Injury,
            };
            agent.activity = None;
            agent.intention = None;
            agent.goals.clear();
        }
        world.validate().expect("valid empty population");
        let before = world.clone();
        let mut engine = LocalDecisionEngine::new(1);
        assert_eq!(
            run_simulation(world, 100, &mut engine)
                .await
                .expect("simulation"),
            before
        );
    }

    #[tokio::test]
    async fn seeded_runs_are_reproducible_and_exercise_actions() {
        let left_world = World::briar_glen(1_234).expect("town");
        let right_world = left_world.clone();
        let mut left_engine = LocalDecisionEngine::new(1_234);
        let mut right_engine = LocalDecisionEngine::new(1_234);

        let mut emitted = 0;
        let left = run_simulation_with_audit(
            left_world,
            2_000,
            &mut left_engine,
            |_, _| emitted += 1,
            |_| {},
        )
        .await
        .expect("simulation");
        let right = run_simulation(right_world, 2_000, &mut right_engine)
            .await
            .expect("simulation");

        assert_eq!(left, right);
        assert_eq!(emitted, left.events().len());
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Moved { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Spoke { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Observed { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Purchased { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Rested { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Worked { .. }))
        );
        assert!(
            left.events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::Waited { .. }))
        );
        left.validate().expect("valid world");
    }

    #[tokio::test]
    async fn different_seeds_diverge_within_ten_ticks() {
        let mut signatures = Vec::new();
        for seed in [814_921, 2_643, 4_375, 5_276] {
            let world = World::briar_glen(seed).expect("town");
            let mut engine = LocalDecisionEngine::new(seed);
            let world = run_simulation(world, 10, &mut engine)
                .await
                .expect("simulation");
            let signature = world
                .events()
                .iter()
                .map(|event| std::mem::discriminant(&event.kind))
                .collect::<Vec<_>>();
            assert!(!signatures.contains(&signature));
            signatures.push(signature);
        }
    }

    struct CountingEngine(usize);

    impl DecisionEngine for CountingEngine {
        async fn decide(
            &mut self,
            _observation: &AgentObservation,
        ) -> Result<Decision, DecisionError> {
            self.0 += 1;
            Ok(Decision::local(ProposedAction::Wait))
        }
    }

    #[tokio::test]
    async fn continued_intentions_skip_new_decisions() {
        let mut world = World::briar_glen(1).expect("town");
        world.advance_to(Tick(12 * 12)).expect("noon");
        let actor = world
            .agents
            .values()
            .nth((world.tick.0 as usize + 1) % world.agents.len())
            .expect("scheduled resident")
            .id;
        let destination = world
            .locations
            .values()
            .find(|location| location.name == "Town Hall")
            .expect("town hall")
            .id;
        let agent = world.agents.get_mut(&actor).expect("resident");
        agent.needs.food = 1.0;
        agent.needs.energy = 1.0;
        agent.needs.safety = 1.0;
        agent.intention = Some(Intention {
            goal: IntentionGoal::Visit { destination },
            expires_at: Tick(world.tick.0 + 36),
        });
        let mut engine = CountingEngine(0);

        let world = run_simulation(world, 1, &mut engine)
            .await
            .expect("simulation");

        assert_eq!(engine.0, 0);
        assert!(world.agents[&actor].intention.is_some());
        assert!(matches!(
            world.events().last().expect("movement").kind,
            EventKind::Moved { agent, .. } if agent == actor
        ));
    }

    #[derive(Clone, Copy)]
    enum LlmChoice {
        Observe,
        Rest,
        InvalidVisit,
    }

    struct FixedLlmEngine(LlmChoice);

    impl DecisionEngine for FixedLlmEngine {
        async fn decide(
            &mut self,
            observation: &AgentObservation,
        ) -> Result<Decision, DecisionError> {
            let (proposal, intention) = match self.0 {
                LlmChoice::Observe => {
                    let target =
                        crate::sim::ObservationTarget::Location(observation.current_location.id);
                    (
                        ProposedAction::Observe {
                            target: target.clone(),
                        },
                        IntentionGoal::Observe { target },
                    )
                }
                LlmChoice::Rest => (ProposedAction::Rest, IntentionGoal::Rest),
                LlmChoice::InvalidVisit => {
                    let destination = LocationId(Uuid::nil());
                    (
                        ProposedAction::Move { destination },
                        IntentionGoal::Visit { destination },
                    )
                }
            };
            Ok(Decision::llm_intention(proposal, intention))
        }
    }

    #[tokio::test]
    async fn audit_records_immediate_llm_completion_as_json() {
        let mut world = World::briar_glen(1).expect("town");
        for agent in world.agents.values_mut() {
            agent.needs.food = 1.0;
            agent.needs.energy = 1.0;
            agent.needs.safety = 1.0;
            agent.health = 1.0;
            agent.injury = false;
        }
        let mut entries = Vec::new();
        run_simulation_with_audit(
            world,
            1,
            &mut FixedLlmEngine(LlmChoice::Observe),
            |_, _| {},
            |entry| entries.push(entry.clone()),
        )
        .await
        .expect("simulation");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].status, "started");
        assert_eq!(entries[0].proposal, entries[0].action);
        assert_eq!(entries[1].status, "completed");
        assert_eq!(entries[1].reason, Some("objective_reached"));
        for entry in entries {
            let line = serde_json::to_string(&entry).expect("JSONL entry");
            serde_json::from_str::<serde_json::Value>(&line).expect("valid JSON");
        }
    }

    #[tokio::test]
    async fn audit_records_local_steps_and_urgent_interruptions() {
        let mut world = World::briar_glen(1).expect("town");
        for agent in world.agents.values_mut() {
            agent.needs.energy = 0.0;
            agent.needs.food = 1.0;
            agent.needs.safety = 1.0;
            agent.health = 1.0;
            agent.injury = false;
        }
        let mut entries = Vec::new();
        let world = run_simulation_with_audit(
            world,
            24,
            &mut FixedLlmEngine(LlmChoice::Rest),
            |_, _| {},
            |entry| entries.push(entry.clone()),
        )
        .await
        .expect("simulation");
        assert!(entries.iter().any(|entry| entry.status == "step"));

        let mut interrupted = world;
        for agent in interrupted.agents.values_mut() {
            agent.intention = None;
            agent.llm_intention = false;
            agent.activity = None;
            agent.needs.energy = 1.0;
            agent.needs.safety = 0.0;
        }
        let previous_interruptions = interrupted
            .agents
            .values()
            .map(|agent| agent.routing.llm_intentions_interrupted)
            .sum::<u64>();
        let mut interruption_entries = Vec::new();
        let interrupted = run_simulation_with_audit(
            interrupted,
            8,
            &mut FixedLlmEngine(LlmChoice::Rest),
            |_, _| {},
            |entry| interruption_entries.push(entry.clone()),
        )
        .await
        .expect("simulation");
        assert!(
            interruption_entries.iter().any(|entry| {
                entry.status == "interrupted" && entry.reason == Some("urgent_need")
            })
        );
        assert!(
            interruption_entries
                .iter()
                .filter(|entry| entry.status == "started")
                .all(|entry| entry.action == Some(ProposedAction::Wait))
        );
        assert_eq!(
            interruption_entries
                .iter()
                .filter(|entry| entry.status == "interrupted")
                .count() as u64,
            interrupted
                .agents
                .values()
                .map(|agent| agent.routing.llm_intentions_interrupted)
                .sum::<u64>()
                - previous_interruptions
        );

        let mut rejected_entries = Vec::new();
        let rejected = run_simulation_with_audit(
            World::briar_glen(2).expect("town"),
            1,
            &mut FixedLlmEngine(LlmChoice::InvalidVisit),
            |_, _| {},
            |entry| rejected_entries.push(entry.clone()),
        )
        .await
        .expect("simulation");
        assert_eq!(rejected_entries.len(), 1);
        assert_eq!(rejected_entries[0].status, "rejected");
        assert_eq!(rejected_entries[0].reason, Some("action_rejected"));
        assert_eq!(
            rejected
                .agents
                .values()
                .map(|agent| agent.routing.llm_intentions_started)
                .sum::<u64>(),
            0
        );
    }

    struct FailingEngine;

    impl DecisionEngine for FailingEngine {
        async fn decide(
            &mut self,
            _observation: &AgentObservation,
        ) -> Result<Decision, DecisionError> {
            Err(DecisionError::Unavailable("test failure".into()))
        }
    }

    #[tokio::test]
    async fn decision_failure_falls_back_to_wait() {
        let world = World::briar_glen(1).expect("town");
        let world = run_simulation(world, 1, &mut FailingEngine)
            .await
            .expect("simulation");
        assert!(matches!(world.events()[0].kind, EventKind::Waited { .. }));
    }

    #[tokio::test]
    async fn thirty_days_preserve_invariants() {
        let world = World::briar_glen(1_234).expect("town");
        let start = world.tick.0;
        let mut engine = LocalDecisionEngine::new(1_234);
        let result = run_simulation(world, 30 * 288, &mut engine)
            .await
            .expect("simulation");
        assert_eq!(result.tick.0, start + 30 * 288);
        result.validate().expect("valid world");
    }
}
