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
            let observation = perceive(&world, agent)?;
            let action = match engine.decide(&observation).await {
                Ok(action) => action,
                Err(error) => {
                    warn!(?agent, %error, "decision failed; waiting instead");
                    ProposedAction::Wait
                }
            };
            debug!(?agent, ?action, "executing proposed action");
            let previous_events = world.events().len();
            world.execute(agent, action);
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
        sim::{EventKind, ProposedAction, World},
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
        assert_eq!(emitted, 2_000);
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
        let mut engine = RandomDecisionEngine::new(1_234);
        let result = run_simulation(world, 30 * 288, &mut engine)
            .await
            .expect("simulation");
        assert_eq!(result.tick.0, 30 * 288);
        result.validate().expect("valid world");
    }
}
