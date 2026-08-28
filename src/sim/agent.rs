use super::{AgentId, Event, EventKind, Intention, LocationId, Relationship, Tick};
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Belief {
    pub sociability: f32,
    pub reliability: f32,
    pub hostility: f32,
    pub confidence: f32,
}

impl Default for Belief {
    fn default() -> Self {
        Self {
            sociability: 0.5,
            reliability: 0.5,
            hostility: 0.0,
            confidence: 0.0,
        }
    }
}

impl Belief {
    pub fn is_normalized(self) -> bool {
        [
            self.sociability,
            self.reliability,
            self.hostility,
            self.confidence,
        ]
        .into_iter()
        .all(|value| (0.0..=1.0).contains(&value))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rumor {
    pub event: Event,
    pub source: AgentId,
    pub depth: u8,
    pub confidence: f32,
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Travelling,
    Conversing,
    Observing,
    Eating,
    Resting,
    Working,
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub kind: ActivityKind,
    pub until: Tick,
}

impl Activity {
    pub(super) fn from_event(event: &EventKind, now: Tick) -> Option<Self> {
        let (kind, duration) = match event {
            EventKind::Moved { .. } => (ActivityKind::Travelling, 2),
            EventKind::Spoke { .. } | EventKind::Confronted { .. } => (ActivityKind::Conversing, 3),
            EventKind::Observed { .. } => (ActivityKind::Observing, 1),
            EventKind::Ate { .. } => (ActivityKind::Eating, 3),
            EventKind::Rested { .. } => (ActivityKind::Resting, 12),
            EventKind::Worked { .. } => (ActivityKind::Working, 12),
            EventKind::Waited { .. } => (ActivityKind::Waiting, 1),
            EventKind::GoalCompleted { .. } | EventKind::ActionRejected { .. } => return None,
        };
        Some(Self {
            kind,
            until: Tick(now.0.checked_add(duration)?),
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
    #[serde(default)]
    pub activity: Option<Activity>,
    pub intention: Option<Intention>,
    #[serde(default)]
    pub mood: f32,
    pub relationships: BTreeMap<AgentId, Relationship>,
    #[serde(default)]
    pub beliefs: BTreeMap<AgentId, Belief>,
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub memories: Vec<Event>,
    #[serde(default)]
    pub rumors: Vec<Rumor>,
}

impl Agent {
    pub(super) fn decay_mood(&mut self, ticks: u64) {
        let decay = 0.002 * ticks as f32;
        self.mood = if self.mood > 0.0 {
            (self.mood - decay).max(0.0)
        } else {
            (self.mood + decay).min(0.0)
        };
    }

    pub(super) fn learn_about_weighted(
        &mut self,
        subject: AgentId,
        sociability: f32,
        reliability: f32,
        hostility: f32,
        weight: f32,
    ) {
        let belief = self.beliefs.entry(subject).or_default();
        belief.sociability = (belief.sociability + sociability * weight).clamp(0.0, 1.0);
        belief.reliability = (belief.reliability + reliability * weight).clamp(0.0, 1.0);
        belief.hostility = (belief.hostility + hostility * weight).clamp(0.0, 1.0);
        belief.confidence = (belief.confidence + 0.15 * weight).min(1.0);
    }

    pub(super) fn decay_beliefs(&mut self, ticks: u64) {
        let decay = 0.001 * ticks as f32;
        for belief in self.beliefs.values_mut() {
            belief.confidence = (belief.confidence - decay).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Agent, Goal, GoalKind};
    use crate::sim::World;

    #[test]
    fn legacy_string_goals_remain_loadable() {
        let goal: Goal = serde_json::from_str("\"Succeed as Alice\"").expect("legacy goal");
        assert_eq!(goal.description, "Succeed as Alice");
        assert_eq!(goal.kind, GoalKind::Livelihood);
        assert_eq!(goal.progress, 0.0);
    }

    #[test]
    fn legacy_agents_default_new_cognition_state() {
        let world = World::briar_glen(1).expect("town");
        let mut value = serde_json::to_value(world.agents.values().next().expect("resident"))
            .expect("agent JSON");
        let object = value.as_object_mut().expect("agent object");
        object.remove("activity");
        object.remove("mood");
        object.remove("beliefs");
        object.remove("rumors");
        let agent: Agent = serde_json::from_value(value).expect("legacy agent");
        assert_eq!(agent.activity, None);
        assert_eq!(agent.mood, 0.0);
        assert!(agent.beliefs.is_empty());
        assert!(agent.rumors.is_empty());
    }
}
