use super::{AgentId, Event, LocationId, Relationship};
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal(pub String);

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
