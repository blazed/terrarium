use super::{
    AgentId, DecisionSource, Event, EventKind, Intention, LocationId, Offering, Relationship, Tick,
};

pub const MAX_ITEMS_PER_KIND: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCause {
    Starvation,
    Exhaustion,
    Injury,
    Disease,
}

impl std::fmt::Display for DeathCause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Starvation => "starvation",
            Self::Exhaustion => "exhaustion",
            Self::Injury => "injury",
            Self::Disease => "disease",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LifeState {
    Alive,
    Dead { tick: Tick, cause: DeathCause },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCondition {
    Hungry,
    Exhausted,
    Injured,
    Sick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum DiseaseState {
    Susceptible,
    Incubating { until: Tick },
    Symptomatic { until: Tick },
    Recovering { until: Tick },
    Immune { until: Tick },
}

impl DiseaseState {
    pub fn is_symptomatic(self) -> bool {
        matches!(self, Self::Symptomatic { .. })
    }

    pub fn is_infected(self) -> bool {
        !matches!(self, Self::Susceptible)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Item {
    Meal,
    Supplies,
    RepairKit,
    Medicine,
}

impl std::fmt::Display for Item {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Meal => "meal",
            Self::Supplies => "supply pack",
            Self::RepairKit => "repair kit",
            Self::Medicine => "medicine",
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub meals: u8,
    pub supplies: u8,
    pub repair_kits: u8,
    pub medicine: u8,
}

impl Inventory {
    pub fn count(self, item: Item) -> u8 {
        match item {
            Item::Meal => self.meals,
            Item::Supplies => self.supplies,
            Item::RepairKit => self.repair_kits,
            Item::Medicine => self.medicine,
        }
    }

    pub fn has_capacity(self, item: Item) -> bool {
        self.count(item) < MAX_ITEMS_PER_KIND
    }

    pub(crate) fn reserve_offering(self, shortage: bool) -> Option<Offering> {
        if shortage {
            None
        } else if self.meals < 2 {
            Some(Offering::Meal)
        } else if self.supplies == 0 {
            Some(Offering::Supplies)
        } else if self.repair_kits == 0 {
            Some(Offering::Repairs)
        } else {
            None
        }
    }

    pub fn is_valid(self) -> bool {
        [self.meals, self.supplies, self.repair_kits, self.medicine]
            .into_iter()
            .all(|count| count <= MAX_ITEMS_PER_KIND)
    }

    pub(super) fn add(&mut self, item: Item) {
        let count = self.count_mut(item);
        *count += 1;
    }

    pub(super) fn remove(&mut self, item: Item) {
        let count = self.count_mut(item);
        *count -= 1;
    }

    fn count_mut(&mut self, item: Item) -> &mut u8 {
        match item {
            Item::Meal => &mut self.meals,
            Item::Supplies => &mut self.supplies,
            Item::RepairKit => &mut self.repair_kits,
            Item::Medicine => &mut self.medicine,
        }
    }
}
use serde::{Deserialize, Serialize};
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
    Doctor,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GoalTarget {
    Work { workplace: LocationId },
    Talk { resident: AgentId },
    Visit { destination: LocationId },
    Purchase { location: LocationId },
    Rest { home: LocationId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub kind: GoalKind,
    pub target: GoalTarget,
    pub progress: u8,
    pub required: u8,
    pub expires_at: Tick,
}

impl Goal {
    pub fn new(
        description: impl Into<String>,
        kind: GoalKind,
        target: GoalTarget,
        required: u8,
        expires_at: Tick,
    ) -> Self {
        Self {
            description: description.into(),
            kind,
            target,
            progress: 0,
            required,
            expires_at,
        }
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
    Shopping,
    UsingItem,
    Treating,
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
            EventKind::Purchased { .. } => (ActivityKind::Shopping, 3),
            EventKind::ItemUsed { .. } => (ActivityKind::UsingItem, 1),
            EventKind::Treated { .. } => (ActivityKind::Treating, 6),
            EventKind::Rested { .. } => (ActivityKind::Resting, 12),
            EventKind::Worked { .. } => (ActivityKind::Working, 12),
            EventKind::Waited { .. } => (ActivityKind::Waiting, 1),
            EventKind::TownEventStarted { .. }
            | EventKind::TownEventEnded { .. }
            | EventKind::GoalCompleted { .. }
            | EventKind::Died { .. }
            | EventKind::DiseaseInfected { .. }
            | EventKind::DiseaseSymptoms { .. }
            | EventKind::DiseaseRecovered { .. }
            | EventKind::DiseaseImmunityExpired { .. }
            | EventKind::ActionRejected { .. } => return None,
        };
        Some(Self {
            kind,
            until: Tick(now.0.checked_add(duration)?),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingStats {
    pub budget_day: u64,
    pub llm_calls_today: u8,
    pub last_llm_attempt: Option<Tick>,
    pub local_decisions: u64,
    pub llm_decisions: u64,
    pub llm_fallbacks: u64,
    pub llm_intentions_started: u64,
    pub llm_intention_steps: u64,
    pub llm_intentions_completed: u64,
    pub llm_intentions_interrupted: u64,
}

impl RoutingStats {
    pub fn llm_calls_on(&self, day: u64) -> u8 {
        if self.budget_day == day {
            self.llm_calls_today
        } else {
            0
        }
    }

    pub fn record(&mut self, tick: Tick, source: DecisionSource) {
        match source {
            DecisionSource::Local => {
                self.local_decisions = self.local_decisions.saturating_add(1);
            }
            DecisionSource::Llm | DecisionSource::LlmFallback => {
                let day = tick.day();
                if self.budget_day != day {
                    self.budget_day = day;
                    self.llm_calls_today = 0;
                }
                self.llm_calls_today = self.llm_calls_today.saturating_add(1);
                self.last_llm_attempt = Some(tick);
                if source == DecisionSource::Llm {
                    self.llm_decisions = self.llm_decisions.saturating_add(1);
                } else {
                    self.local_decisions = self.local_decisions.saturating_add(1);
                    self.llm_fallbacks = self.llm_fallbacks.saturating_add(1);
                }
            }
        }
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
    pub health: f32,
    pub injury: bool,
    pub disease: DiseaseState,
    pub life: LifeState,
    pub balance: u64,
    pub routing: RoutingStats,
    #[serde(default)]
    pub inventory: Inventory,
    #[serde(default)]
    pub activity: Option<Activity>,
    pub intention: Option<Intention>,
    pub llm_intention: bool,
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
    pub fn is_alive(&self) -> bool {
        matches!(self.life, LifeState::Alive)
    }

    pub fn is_hungry(&self) -> bool {
        self.needs.food < 0.1
    }

    pub fn is_exhausted(&self) -> bool {
        self.needs.energy < 0.1
    }

    pub fn health_conditions(&self) -> Vec<HealthCondition> {
        let mut conditions = Vec::new();
        if self.is_hungry() {
            conditions.push(HealthCondition::Hungry);
        }
        if self.is_exhausted() {
            conditions.push(HealthCondition::Exhausted);
        }
        if self.injury {
            conditions.push(HealthCondition::Injured);
        }
        if self.disease.is_symptomatic() {
            conditions.push(HealthCondition::Sick);
        }
        conditions
    }

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
    use super::Agent;
    use crate::sim::World;

    #[test]
    fn legacy_agents_default_new_cognition_state() {
        let world = World::briar_glen(1).expect("town");
        let mut value = serde_json::to_value(world.agents.values().next().expect("resident"))
            .expect("agent JSON");
        let object = value.as_object_mut().expect("agent object");
        object.remove("inventory");
        object.remove("activity");
        object.remove("mood");
        object.remove("beliefs");
        object.remove("rumors");
        let agent: Agent = serde_json::from_value(value).expect("legacy agent");
        assert_eq!(agent.inventory, super::Inventory::default());
        assert_eq!(agent.activity, None);
        assert_eq!(agent.mood, 0.0);
        assert!(agent.beliefs.is_empty());
        assert!(agent.rumors.is_empty());
    }
}
