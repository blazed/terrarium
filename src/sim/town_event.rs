use super::Tick;
use serde::{Deserialize, Serialize};

pub const TOWN_EVENT_DURATION: u64 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TownEventKind {
    Storm,
    Festival,
    Shortage,
    MarketDay,
}

impl std::fmt::Display for TownEventKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Storm => "storm",
            Self::Festival => "festival",
            Self::Shortage => "shortage",
            Self::MarketDay => "market day",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TownEvent {
    pub kind: TownEventKind,
    pub starts_at: Tick,
    pub ends_at: Tick,
}

impl TownEvent {
    pub fn scheduled(seed: u64, tick: Tick) -> Option<Self> {
        let day_start = tick.0 / Tick::PER_DAY * Tick::PER_DAY;
        let starts_at = Tick(day_start + (8 + seed % 4) * 60 / Tick::MINUTES);
        let ends_at = Tick(starts_at.0 + TOWN_EVENT_DURATION);
        (tick >= starts_at && tick < ends_at).then_some(Self {
            kind: match (tick.0 / Tick::PER_DAY).wrapping_add(seed) % 4 {
                0 => TownEventKind::Storm,
                1 => TownEventKind::Festival,
                2 => TownEventKind::Shortage,
                _ => TownEventKind::MarketDay,
            },
            starts_at,
            ends_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TownEvent, TownEventKind};
    use crate::sim::Tick;

    #[test]
    fn seeded_daily_schedule_cycles_without_overlap() {
        let kinds = (0..4)
            .map(|seed| {
                let start = Tick((8 + seed) * 60 / Tick::MINUTES);
                let event = TownEvent::scheduled(seed, start).expect("daily event");
                assert_eq!(TownEvent::scheduled(seed, event.ends_at), None);
                event.kind
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                TownEventKind::Storm,
                TownEventKind::Festival,
                TownEventKind::Shortage,
                TownEventKind::MarketDay,
            ]
        );
    }
}
