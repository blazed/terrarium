use serde::{Deserialize, Serialize};

/// Social values use -1.0 (strongly negative) through 1.0 (strongly positive).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub affection: f32,
    pub trust: f32,
    pub fear: f32,
    pub respect: f32,
    pub attraction: f32,
    pub suspicion: f32,
}

impl Relationship {
    pub const NEUTRAL: Self = Self {
        affection: 0.0,
        trust: 0.0,
        fear: 0.0,
        respect: 0.0,
        attraction: 0.0,
        suspicion: 0.0,
    };

    pub fn is_normalized(self) -> bool {
        [
            self.affection,
            self.trust,
            self.fear,
            self.respect,
            self.attraction,
            self.suspicion,
        ]
        .into_iter()
        .all(|value| (-1.0..=1.0).contains(&value))
    }
}
