use super::{AgentId, World};

/// Gives every living idle resident a turn each tick, in stable ID order.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scheduler;

impl Scheduler {
    pub fn agents_to_act(self, world: &World) -> Vec<AgentId> {
        world
            .agents
            .values()
            .filter(|agent| agent.is_alive() && agent.activity.is_none())
            .map(|agent| agent.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Scheduler;
    use crate::sim::{Activity, ActivityKind, BRIAR_GLEN, Tick, World};

    #[test]
    fn busy_residents_do_not_receive_turns() {
        let mut world = World::from_spec(BRIAR_GLEN, 22).expect("town");
        let scheduler = Scheduler;
        let actors = scheduler.agents_to_act(&world);
        let actor = actors[0];
        assert_eq!(actors.len(), world.agents.len());
        world.agents.get_mut(&actor).expect("resident").activity = Some(Activity {
            kind: ActivityKind::Resting,
            until: Tick(world.tick.0 + 12),
        });
        let actors = scheduler.agents_to_act(&world);
        assert_eq!(actors.len(), world.agents.len() - 1);
        assert!(!actors.contains(&actor));
    }

    #[test]
    fn thousands_of_ticks_schedule_reproducibly() {
        let mut left = World::from_spec(BRIAR_GLEN, 22).expect("town");
        let mut right = left.clone();
        let scheduler = Scheduler;
        let mut left_schedule = Vec::new();
        let mut right_schedule = Vec::new();

        for _ in 0..10_000 {
            left.advance_tick().expect("tick");
            right.advance_tick().expect("tick");
            left_schedule.extend(scheduler.agents_to_act(&left));
            right_schedule.extend(scheduler.agents_to_act(&right));
            left.validate().expect("valid world");
        }

        assert_eq!(left_schedule, right_schedule);
        assert!(left_schedule.len() > 10_000);
    }
}
