use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::AgentObservation,
    sim::{Decision, IntentionGoal, ProposedAction, Tick},
};
use tracing::warn;

pub const DEFAULT_LLM_CALLS_PER_DAY: u8 = 2;

pub struct HybridDecisionEngine<L, E> {
    local: L,
    llm: E,
    calls_per_day: u8,
}

impl<L, E> HybridDecisionEngine<L, E> {
    pub fn new(local: L, llm: E, calls_per_day: u8) -> Self {
        Self {
            local,
            llm,
            calls_per_day,
        }
    }

    fn can_call_llm(&self, observation: &AgentObservation, action: &ProposedAction) -> bool {
        if !is_adaptive(action) {
            return false;
        }
        let routing = observation.self_description.routing;
        if routing.llm_calls_on(observation.tick.day()) >= self.calls_per_day {
            return false;
        }
        let minimum_spacing = Tick::PER_DAY / u64::from(self.calls_per_day);
        routing
            .last_llm_attempt
            .is_none_or(|last| observation.tick.0.saturating_sub(last.0) >= minimum_spacing)
    }
}

impl<L: DecisionEngine + Send, E: DecisionEngine + Send> DecisionEngine
    for HybridDecisionEngine<L, E>
{
    async fn decide(&mut self, observation: &AgentObservation) -> Result<Decision, DecisionError> {
        let local = self.local.decide(observation).await?;
        if !self.can_call_llm(observation, &local.action) {
            return Ok(Decision::local(local.action));
        }
        match self.llm.decide(observation).await {
            Ok(decision) => match durable_intention(observation, decision.action.clone()) {
                Some(intention) => Ok(Decision::llm_intention(decision.action, intention)),
                None => {
                    warn!("LLM returned no durable objective; using local proposal");
                    Ok(Decision::llm_fallback(local.action))
                }
            },
            Err(error) => {
                warn!(%error, "LLM decision failed; using local proposal");
                Ok(Decision::llm_fallback(local.action))
            }
        }
    }
}

fn durable_intention(
    observation: &AgentObservation,
    action: ProposedAction,
) -> Option<IntentionGoal> {
    match action {
        ProposedAction::Pursue { intention } => Some(intention),
        ProposedAction::Move { destination } => Some(IntentionGoal::Visit { destination }),
        ProposedAction::Talk {
            target,
            tone,
            message,
        } => Some(IntentionGoal::Talk {
            target,
            tone,
            message,
        }),
        ProposedAction::Confront { target, claim } => {
            Some(IntentionGoal::Confront { target, claim })
        }
        ProposedAction::Observe { target } => Some(IntentionGoal::Observe { target }),
        ProposedAction::Purchase => Some(IntentionGoal::Purchase {
            destination: observation
                .route_hints
                .purchase
                .map_or(observation.current_location.id, |route| route.destination),
        }),
        ProposedAction::SeekTreatment => Some(IntentionGoal::SeekTreatment),
        ProposedAction::Give { target, item } => Some(IntentionGoal::Give { target, item }),
        ProposedAction::Rest => Some(IntentionGoal::Rest),
        ProposedAction::Work => Some(IntentionGoal::Work),
        ProposedAction::Steal { target, loot } => Some(IntentionGoal::StealFrom { target, loot }),
        ProposedAction::Attack { target } => Some(IntentionGoal::Attack { target }),
        ProposedAction::Arrest { .. }
        | ProposedAction::ConsumeMeal
        | ProposedAction::UseSupplies
        | ProposedAction::UseRepairKit
        | ProposedAction::UseMedicine
        | ProposedAction::Wait => None,
    }
}

fn is_adaptive(action: &ProposedAction) -> bool {
    matches!(
        action,
        ProposedAction::Talk { .. }
            | ProposedAction::Confront { .. }
            | ProposedAction::Observe { .. }
            | ProposedAction::Wait
            | ProposedAction::Pursue {
                intention: IntentionGoal::Talk { .. }
            }
    )
}

#[cfg(test)]
mod tests {
    use super::{HybridDecisionEngine, durable_intention};
    use crate::{
        cognition::perceive,
        decision::{DecisionEngine, DecisionError, LocalDecisionEngine},
        persistence::{load_world, save_world},
        runner::run_simulation,
        sim::{
            BRIAR_GLEN, Decision, DecisionSource, IntentionGoal, ObservationTarget, ProposedAction,
            Tick, World,
        },
    };
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    struct FixedEngine {
        action: ProposedAction,
        calls: Arc<AtomicUsize>,
        fails: bool,
    }

    impl DecisionEngine for FixedEngine {
        async fn decide(
            &mut self,
            _observation: &crate::cognition::AgentObservation,
        ) -> Result<Decision, DecisionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fails {
                Err(DecisionError::Unavailable("test failure".into()))
            } else {
                Ok(Decision::local(self.action.clone()))
            }
        }
    }

    fn observation() -> crate::cognition::AgentObservation {
        let world = World::from_spec(BRIAR_GLEN, 1).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        perceive(&world, actor).expect("observation")
    }

    fn engine(
        local: ProposedAction,
        llm: ProposedAction,
        llm_calls: Arc<AtomicUsize>,
        fails: bool,
    ) -> HybridDecisionEngine<FixedEngine, FixedEngine> {
        HybridDecisionEngine::new(
            FixedEngine {
                action: local,
                calls: Arc::new(AtomicUsize::new(0)),
                fails: false,
            },
            FixedEngine {
                action: llm,
                calls: llm_calls,
                fails,
            },
            2,
        )
    }

    #[tokio::test]
    async fn routine_actions_never_call_the_llm() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut engine = engine(
            ProposedAction::Rest,
            ProposedAction::Wait,
            calls.clone(),
            false,
        );

        let decision = engine.decide(&observation()).await.expect("decision");

        assert_eq!(decision.action, ProposedAction::Rest);
        assert_eq!(decision.source, DecisionSource::Local);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn adaptive_actions_obey_budget_and_spacing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let llm_action = ProposedAction::Observe {
            target: ObservationTarget::Location(observation().current_location.id),
        };
        let mut engine = engine(
            ProposedAction::Wait,
            llm_action.clone(),
            calls.clone(),
            false,
        );
        let mut observation = observation();

        let decision = engine.decide(&observation).await.expect("first decision");
        assert_eq!(
            decision.action,
            ProposedAction::Pursue {
                intention: IntentionGoal::Observe {
                    target: ObservationTarget::Location(observation.current_location.id),
                },
            }
        );
        assert_eq!(decision.source, DecisionSource::Llm);

        observation.self_description.routing.llm_calls_today = 1;
        observation.self_description.routing.budget_day = observation.tick.day();
        observation.self_description.routing.last_llm_attempt = Some(observation.tick);
        assert_eq!(
            engine
                .decide(&observation)
                .await
                .expect("spaced decision")
                .source,
            DecisionSource::Local
        );

        observation.tick = Tick(observation.tick.0 + Tick::PER_DAY / 2);
        observation.self_description.routing.llm_calls_today = 2;
        assert_eq!(
            engine
                .decide(&observation)
                .await
                .expect("budgeted decision")
                .source,
            DecisionSource::Local
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn local_steps_continue_without_more_llm_calls() {
        let local_calls = Arc::new(AtomicUsize::new(0));
        let llm_calls = Arc::new(AtomicUsize::new(0));
        let mut engine = HybridDecisionEngine::new(
            FixedEngine {
                action: ProposedAction::Wait,
                calls: local_calls,
                fails: false,
            },
            FixedEngine {
                action: ProposedAction::Rest,
                calls: llm_calls.clone(),
                fails: false,
            },
            u8::MAX,
        );
        let mut world = World::from_spec(BRIAR_GLEN, 7).expect("town");
        let homes = world
            .agents
            .values()
            .map(|agent| (agent.id, agent.location, agent.home))
            .collect::<Vec<_>>();
        for (agent, location, home) in homes {
            world
                .locations
                .get_mut(&location)
                .expect("location")
                .agents
                .remove(&agent);
            world
                .locations
                .get_mut(&home)
                .expect("home")
                .agents
                .insert(agent);
            let resident = world.agents.get_mut(&agent).expect("resident");
            resident.location = home;
            resident.needs.energy = 0.0;
            resident.needs.food = 1.0;
            resident.needs.safety = 1.0;
            resident.health = 1.0;
            resident.injury = false;
        }

        let world = run_simulation(world, 24, &mut engine)
            .await
            .expect("simulation");

        assert_eq!(llm_calls.load(Ordering::Relaxed), world.agents.len());
        assert!(
            world
                .agents
                .values()
                .map(|agent| agent.routing.llm_intention_steps)
                .sum::<u64>()
                > 0
        );
    }

    #[tokio::test]
    async fn town_day_never_exceeds_two_calls_per_resident() {
        let calls = Arc::new(AtomicUsize::new(0));
        let llm = FixedEngine {
            action: ProposedAction::Wait,
            calls: calls.clone(),
            fails: false,
        };
        let mut engine = HybridDecisionEngine::new(LocalDecisionEngine::new(7), llm, 2);
        let mut world = World::from_spec(BRIAR_GLEN, 7).expect("town");
        world.advance_to(Tick(Tick::PER_DAY)).expect("day boundary");

        let world = run_simulation(world, Tick::PER_DAY, &mut engine)
            .await
            .expect("simulation");
        let attempts: u64 = world
            .agents
            .values()
            .map(|agent| agent.routing.llm_decisions + agent.routing.llm_fallbacks)
            .sum();

        assert!(calls.load(Ordering::Relaxed) > 0);
        assert!(calls.load(Ordering::Relaxed) <= world.agents.len() * 2);
        assert_eq!(attempts, calls.load(Ordering::Relaxed) as u64);
    }

    #[tokio::test]
    async fn routing_budget_survives_checkpoint_resume() {
        let run = |calls| {
            HybridDecisionEngine::new(
                LocalDecisionEngine::new(11),
                FixedEngine {
                    action: ProposedAction::Rest,
                    calls,
                    fails: false,
                },
                2,
            )
        };
        let mut continuous_engine = run(Arc::new(AtomicUsize::new(0)));
        let continuous = run_simulation(
            World::from_spec(BRIAR_GLEN, 11).expect("town"),
            300,
            &mut continuous_engine,
        )
        .await
        .expect("continuous simulation");

        let mut first_engine = run(Arc::new(AtomicUsize::new(0)));
        let first = run_simulation(
            World::from_spec(BRIAR_GLEN, 11).expect("town"),
            120,
            &mut first_engine,
        )
        .await
        .expect("first simulation");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "terrarium-hybrid-{}-{nonce}.sqlite",
            std::process::id()
        ));
        save_world(&path, &first).expect("save checkpoint");
        let resumed = load_world(&path).expect("load checkpoint");
        let mut resumed_engine = run(Arc::new(AtomicUsize::new(0)));
        let resumed = run_simulation(resumed, 180, &mut resumed_engine)
            .await
            .expect("resumed simulation");

        assert_eq!(resumed, continuous);
        fs::remove_file(path).expect("cleanup");
    }

    #[tokio::test]
    async fn llm_failure_returns_the_local_proposal() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut engine = engine(
            ProposedAction::Wait,
            ProposedAction::Rest,
            calls.clone(),
            true,
        );

        let decision = engine.decide(&observation()).await.expect("fallback");

        assert_eq!(decision.action, ProposedAction::Wait);
        assert_eq!(decision.source, DecisionSource::LlmFallback);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn crime_actions_never_trigger_an_llm_consult() {
        let calls = Arc::new(AtomicUsize::new(0));
        let target = observation().visible_agents[0].id;
        for local in [
            ProposedAction::Steal {
                target,
                loot: crate::sim::Loot::Coins(1),
            },
            ProposedAction::Attack { target },
        ] {
            let mut engine = engine(local.clone(), ProposedAction::Wait, calls.clone(), false);
            let decision = engine.decide(&observation()).await.expect("decision");
            assert_eq!(decision.action, local);
            assert_eq!(decision.source, DecisionSource::Local);
            assert_eq!(calls.load(Ordering::Relaxed), 0);
        }
    }

    #[tokio::test]
    async fn llm_proposed_crime_gets_a_durable_intention() {
        // The LLM can only be consulted for adaptive actions, but when it is, its
        // crime proposals must map onto durable intentions for the runner to pursue.
        let calls = Arc::new(AtomicUsize::new(0));
        let observation = observation();
        let target = observation.visible_agents[0].id;
        let mut engine = engine(
            ProposedAction::Talk {
                target,
                tone: crate::sim::DialogueTone::Friendly,
                message: "hello".into(),
            },
            ProposedAction::Steal {
                target,
                loot: crate::sim::Loot::Coins(1),
            },
            calls.clone(),
            false,
        );
        let decision = engine.decide(&observation).await.expect("decision");
        assert_eq!(
            decision.action,
            ProposedAction::Pursue {
                intention: IntentionGoal::StealFrom {
                    target,
                    loot: crate::sim::Loot::Coins(1)
                }
            }
        );
        assert_eq!(decision.source, DecisionSource::Llm);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        assert_eq!(
            durable_intention(&observation, ProposedAction::Attack { target }),
            Some(IntentionGoal::Attack { target })
        );
    }
}
