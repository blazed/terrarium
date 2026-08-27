use super::{AgentId, LocationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: LocationId,
    pub name: String,
    #[serde(default)]
    pub serves_food: bool,
    pub connected: BTreeSet<LocationId>,
    pub agents: BTreeSet<AgentId>,
}
