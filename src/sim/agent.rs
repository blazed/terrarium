use super::{AgentId, Event, LocationId, Relationship};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Occupation {
    Baker,
    Carpenter,
    Shopkeeper,
    Laborer,
    Teacher,
    Sheriff,
    Publican,
    Clerk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Personality {
    pub openness: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
    pub honesty: f32,
    pub ambition: f32,
    pub impulsiveness: f32,
}

impl Personality {
    pub fn is_normalized(&self) -> bool {
        [
            self.openness,
            self.agreeableness,
            self.neuroticism,
            self.honesty,
            self.ambition,
            self.impulsiveness,
        ]
        .into_iter()
        .all(|value| (0.0..=1.0).contains(&value))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Needs {
    pub money: f32,
    pub food: f32,
    pub companionship: f32,
    pub safety: f32,
    pub status: f32,
    pub energy: f32,
}

impl Needs {
    pub fn is_normalized(&self) -> bool {
        [
            self.money,
            self.food,
            self.companionship,
            self.safety,
            self.status,
            self.energy,
        ]
        .into_iter()
        .all(|value| (0.0..=1.0).contains(&value))
    }

    pub(super) fn decay(&mut self, ticks: u64) {
        let ticks = ticks as f32;
        self.money = (self.money - 0.0002 * ticks).max(0.0);
        self.food = (self.food - 0.001 * ticks).max(0.0);
        self.companionship = (self.companionship - 0.0005 * ticks).max(0.0);
        self.safety = (self.safety - 0.0001 * ticks).max(0.0);
        self.status = (self.status - 0.0002 * ticks).max(0.0);
        self.energy = (self.energy - 0.0007 * ticks).max(0.0);
    }

    pub(super) fn restore(value: &mut f32, amount: f32) {
        *value = (*value + amount).min(1.0);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalKind {
    #[default]
    Livelihood,
    Community,
    Exploration,
    Wellbeing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Goal {
    pub description: String,
    pub kind: GoalKind,
    pub progress: f32,
}

impl Goal {
    pub fn new(description: impl Into<String>, kind: GoalKind) -> Self {
        Self {
            description: description.into(),
            kind,
            progress: 0.0,
        }
    }
}

impl<'de> Deserialize<'de> for Goal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StoredGoal {
            Legacy(String),
            Current {
                description: String,
                #[serde(default)]
                kind: GoalKind,
                #[serde(default)]
                progress: f32,
            },
        }

        Ok(match StoredGoal::deserialize(deserializer)? {
            StoredGoal::Legacy(description) => Self::new(description, GoalKind::Livelihood),
            StoredGoal::Current {
                description,
                kind,
                progress,
            } => Self {
                description,
                kind,
                progress,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub age: u32,
    pub occupation: Occupation,
    pub home: LocationId,
    pub workplace: Option<LocationId>,
    pub location: LocationId,
    pub personality: Personality,
    pub needs: Needs,
    pub relationships: BTreeMap<AgentId, Relationship>,
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub memories: Vec<Event>,
}

#[cfg(test)]
mod tests {
    use super::{Goal, GoalKind};

    #[test]
    fn legacy_string_goals_remain_loadable() {
        let goal: Goal = serde_json::from_str("\"Succeed as Alice\"").expect("legacy goal");
        assert_eq!(goal.description, "Succeed as Alice");
        assert_eq!(goal.kind, GoalKind::Livelihood);
        assert_eq!(goal.progress, 0.0);
    }
}
