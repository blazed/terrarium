use crate::{
    cognition::{ObservationError, perceive},
    decision::DecisionEngine,
    sim::{Event, ProposedAction, Scheduler, World, WorldError},
};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(transparent)]
    Observation(#[from] ObservationError),
}

pub async fn run_simulation(
    world: World,
    ticks: u64,
    engine: &mut impl DecisionEngine,
) -> Result<World, SimulationError> {
    run_simulation_with_events(world, ticks, engine, |_, _| {}).await
}

pub async fn run_simulation_with_events(
    mut world: World,
    ticks: u64,
    engine: &mut impl DecisionEngine,
    mut on_event: impl FnMut(&World, &Event),
) -> Result<World, SimulationError> {
    let scheduler = Scheduler;
    for _ in 0..ticks {
        world.advance_tick()?;
        for agent in scheduler.agents_to_act(&world) {
            let previous_events = world.events().len();
            if world.continue_intention(agent).is_none() {
                let observation = perceive(&world, agent)?;
                let action = match engine.decide(&observation).await {
                    Ok(action) => action,
                    Err(error) => {
                        warn!(?agent, %error, "decision failed; waiting instead");
                        ProposedAction::Wait
                    }
                };
                debug!(?agent, ?action, "executing proposed action");
                world.execute(agent, action);
            }
            world.validate()?;
            for event in &world.events()[previous_events..] {
                on_event(&world, event);
            }
        }
    }
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::{run_simulation, run_simulation_with_events};
    use crate::{
        cognition::AgentObservation,
        decision::{DecisionEngine, DecisionError, RandomDecisionEngine},
        sim::{EventKind, Intention, IntentionGoal, ProposedAction, Tick, World},
    };

    #[tokio::test]
    async fn seeded_runs_are_reproducible_and_exercise_actions() {
        let left_world = World::briar_glen(1_234).expect("town");
        let right_world = left_world.clone();
        let mut left_engine = RandomDecisionEngine::new(1_234);
        let mut right_engine = RandomDecisionEngine::new(1_234);

        let mut emitted = 0;
        let left = run_simulation_with_events(left_world, 2_000, &mut left_engine, |_, _| {
            emitted += 1;
        })
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
                .any(|event| matches!(event.kind, EventKind::Ate { .. }))
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
            let mut engine = RandomDecisionEngine::new(seed);
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
        ) -> Result<ProposedAction, DecisionError> {
            self.0 += 1;
            Ok(ProposedAction::Wait)
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

    struct FailingEngine;

    impl DecisionEngine for FailingEngine {
        async fn decide(
            &mut self,
            _observation: &AgentObservation,
        ) -> Result<ProposedAction, DecisionError> {
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
        let mut engine = RandomDecisionEngine::new(1_234);
        let result = run_simulation(world, 30 * 288, &mut engine)
            .await
            .expect("simulation");
        assert_eq!(result.tick.0, start + 30 * 288);
        result.validate().expect("valid world");
    }
}
