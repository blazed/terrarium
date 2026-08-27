use super::{AgentId, World};

/// Gives one resident a turn per tick, rotating in stable ID order.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scheduler;

impl Scheduler {
    pub fn agents_to_act(self, world: &World) -> Vec<AgentId> {
        if world.agents.is_empty() {
            return Vec::new();
        }
        world
            .agents
            .keys()
            .nth(world.tick.0 as usize % world.agents.len())
            .copied()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Scheduler;
    use crate::sim::World;

    #[test]
    fn thousands_of_ticks_schedule_reproducibly() {
        let mut left = World::briar_glen(22).expect("town");
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
        assert_eq!(left_schedule.len(), 10_000);
    }
}
