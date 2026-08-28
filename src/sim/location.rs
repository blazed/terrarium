use super::{AgentId, LocationId, Needs, WORK_WAGE};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Offering {
    Meal,
    Supplies,
    Repairs,
    CivicServices,
}

impl Offering {
    pub fn desired(needs: &Needs) -> Option<Self> {
        if needs.food < 0.25 {
            Some(Self::Meal)
        } else if needs.safety < 0.2 {
            Some(Self::Repairs)
        } else if needs.safety < 0.4 {
            Some(Self::Supplies)
        } else if needs.status < 0.4 {
            Some(Self::CivicServices)
        } else {
            None
        }
    }
}

impl std::fmt::Display for Offering {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Meal => "meal",
            Self::Supplies => "supplies",
            Self::Repairs => "repairs",
            Self::CivicServices => "civic services",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Business {
    pub offering: Offering,
    pub price: u64,
    pub cash: u64,
    pub stock: u32,
    pub revenue: u64,
    pub wages_paid: u64,
}

impl Business {
    pub fn solvent(self) -> bool {
        self.cash >= WORK_WAGE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: LocationId,
    pub name: String,
    pub business: Option<Business>,
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
