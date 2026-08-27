use super::{AgentId, LocationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningHours {
    pub opens_at_hour: u64,
    pub closes_at_hour: u64,
}

impl OpeningHours {
    pub fn contains(self, hour: u64) -> bool {
        (self.opens_at_hour..self.closes_at_hour).contains(&hour)
    }

    pub fn is_valid(self) -> bool {
        self.opens_at_hour < self.closes_at_hour && self.closes_at_hour <= 24
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: LocationId,
    pub name: String,
    #[serde(default)]
    pub serves_food: bool,
    pub opening_hours: Option<OpeningHours>,
    pub connected: BTreeSet<LocationId>,
    pub agents: BTreeSet<AgentId>,
}

impl Location {
    pub fn is_open(&self, hour: u64) -> bool {
        self.opening_hours.is_none_or(|hours| hours.contains(hour))
    }
}

#[cfg(test)]
mod tests {
    use super::OpeningHours;

    #[test]
    fn closing_hour_is_exclusive() {
        let hours = OpeningHours {
            opens_at_hour: 8,
            closes_at_hour: 18,
        };
        assert!(!hours.contains(7));
        assert!(hours.contains(8));
        assert!(hours.contains(17));
        assert!(!hours.contains(18));
    }
}
