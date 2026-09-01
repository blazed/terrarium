use super::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::Deserialize;
use thiserror::Error;

/// The built-in Briar Glen town definition. Default town when no `--town` is given.
pub const BRIAR_GLEN: &str = include_str!("../../../assets/briar_glen.json");

/// A town as authored data. `connections` are undirected edges, each listed once.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TownSpec {
    name: String,
    locations: Vec<LocationSpec>,
    connections: Vec<[String; 2]>,
    residents: Vec<ResidentSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocationSpec {
    name: String,
    kind: LocationKind,
    business: Option<BusinessSpec>,
    opening_hours: Option<OpeningHours>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LocationKind {
    Home,
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BusinessSpec {
    offering: Offering,
    price: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentSpec {
    name: String,
    age: u32,
    occupation: Occupation,
    home: String,
    workplace: Option<String>,
    personality: PersonalityRanges,
    needs: NeedsRanges,
}

/// A trait sampled as `(base + rng.random_range(-spread..=spread)).clamp(0.0, 1.0)`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct CenteredRange {
    base: f32,
    spread: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalityRanges {
    openness: CenteredRange,
    agreeableness: CenteredRange,
    neuroticism: CenteredRange,
    honesty: CenteredRange,
    ambition: CenteredRange,
    impulsiveness: CenteredRange,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NeedsRanges {
    money: CenteredRange,
    food: CenteredRange,
    companionship: CenteredRange,
    safety: CenteredRange,
    status: CenteredRange,
    energy: CenteredRange,
}

#[derive(Debug, Error)]
pub enum TownSpecError {
    #[error("invalid JSON: {0}")]
    MalformedJson(#[from] serde_json::Error),
    #[error("town name must not be empty")]
    EmptyTownName,
    #[error("town must have at least one location")]
    NoLocations,
    #[error("town must have at least one resident")]
    NoResidents,
    #[error("location name must not be empty")]
    EmptyLocationName,
    #[error("resident name must not be empty")]
    EmptyResidentName,
    #[error("duplicate location name '{0}'")]
    DuplicateLocationName(String),
    #[error("duplicate resident name '{0}'")]
    DuplicateResidentName(String),
    #[error("unknown connection endpoint '{0}'")]
    UnknownConnectionEndpoint(String),
    #[error("self-connection on '{0}'")]
    SelfConnection(String),
    #[error("duplicate connection between '{0}' and '{1}'")]
    DuplicateConnection(String, String),
    #[error("unknown home '{0}'")]
    UnknownHome(String),
    #[error("home '{0}' is not a Home-kind location")]
    HomeNotHome(String),
    #[error("Home location '{0}' cannot have opening hours")]
    HomeWithOpeningHours(String),
    #[error("unknown workplace '{0}'")]
    UnknownWorkplace(String),
    #[error("workplace '{0}' has no business")]
    WorkplaceWithoutBusiness(String),
    #[error("invalid opening hours at '{0}'")]
    InvalidOpeningHours(String),
    #[error("business at '{0}' must have a price greater than zero")]
    ZeroBusinessPrice(String),
    #[error("invalid {0} range")]
    InvalidRange(String),
    #[error("sheriff present but no location named Jail")]
    SheriffWithoutJail,
    #[error("invalid generated world: {0}")]
    World(#[from] WorldError),
}

impl World {
    /// Build a world from authored town data. Location and resident array order
    /// determines seeded IDs; all references use unique names.
    pub fn from_spec(json: &str, seed: u64) -> Result<Self, TownSpecError> {
        let spec: TownSpec = serde_json::from_str(json)?;
        validate_spec(&spec)?;
        let mut world = build_world(&spec, seed);
        let agent_ids: Vec<_> = world.agents.keys().copied().collect();
        for agent in agent_ids {
            world.refresh_goals(agent);
        }
        world.validate()?;
        Ok(world)
    }
}

fn validate_spec(spec: &TownSpec) -> Result<(), TownSpecError> {
    if spec.name.trim().is_empty() {
        return Err(TownSpecError::EmptyTownName);
    }
    if spec.locations.is_empty() {
        return Err(TownSpecError::NoLocations);
    }
    if spec.residents.is_empty() {
        return Err(TownSpecError::NoResidents);
    }

    let mut location_names = BTreeSet::new();
    for location in &spec.locations {
        if location.name.trim().is_empty() {
            return Err(TownSpecError::EmptyLocationName);
        }
        if !location_names.insert(&location.name) {
            return Err(TownSpecError::DuplicateLocationName(location.name.clone()));
        }
        if let Some(hours) = location.opening_hours {
            if !hours.is_valid() {
                return Err(TownSpecError::InvalidOpeningHours(location.name.clone()));
            }
            if location.kind == LocationKind::Home {
                return Err(TownSpecError::HomeWithOpeningHours(location.name.clone()));
            }
        }
        if location.business.as_ref().map(|business| business.price) == Some(0) {
            return Err(TownSpecError::ZeroBusinessPrice(location.name.clone()));
        }
    }

    let mut seen_connections = BTreeSet::new();
    for [left, right] in &spec.connections {
        if !location_names.contains(left) {
            return Err(TownSpecError::UnknownConnectionEndpoint(left.clone()));
        }
        if !location_names.contains(right) {
            return Err(TownSpecError::UnknownConnectionEndpoint(right.clone()));
        }
        if left == right {
            return Err(TownSpecError::SelfConnection(left.clone()));
        }
        let (key_left, key_right) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        if !seen_connections.insert((key_left, key_right)) {
            return Err(TownSpecError::DuplicateConnection(
                left.clone(),
                right.clone(),
            ));
        }
    }

    let has_jail = spec.locations.iter().any(|l| l.name == "Jail");
    let mut resident_names = BTreeSet::new();
    for resident in &spec.residents {
        if resident.name.trim().is_empty() {
            return Err(TownSpecError::EmptyResidentName);
        }
        if !resident_names.insert(&resident.name) {
            return Err(TownSpecError::DuplicateResidentName(resident.name.clone()));
        }
        let home = lookup(spec, &resident.home)
            .ok_or_else(|| TownSpecError::UnknownHome(resident.home.clone()))?;
        if home.kind != LocationKind::Home {
            return Err(TownSpecError::HomeNotHome(resident.home.clone()));
        }
        if let Some(workplace) = &resident.workplace {
            let workplace_location = lookup(spec, workplace)
                .ok_or_else(|| TownSpecError::UnknownWorkplace(workplace.clone()))?;
            if workplace_location.business.is_none() {
                return Err(TownSpecError::WorkplaceWithoutBusiness(workplace.clone()));
            }
        }
        validate_ranges(
            "personality",
            [
                ("openness", resident.personality.openness),
                ("agreeableness", resident.personality.agreeableness),
                ("neuroticism", resident.personality.neuroticism),
                ("honesty", resident.personality.honesty),
                ("ambition", resident.personality.ambition),
                ("impulsiveness", resident.personality.impulsiveness),
            ],
        )?;
        validate_ranges(
            "need",
            [
                ("money", resident.needs.money),
                ("food", resident.needs.food),
                ("companionship", resident.needs.companionship),
                ("safety", resident.needs.safety),
                ("status", resident.needs.status),
                ("energy", resident.needs.energy),
            ],
        )?;
        if resident.occupation == Occupation::Sheriff && !has_jail {
            return Err(TownSpecError::SheriffWithoutJail);
        }
    }
    Ok(())
}

fn lookup<'a>(spec: &'a TownSpec, name: &str) -> Option<&'a LocationSpec> {
    spec.locations.iter().find(|l| l.name == name)
}

fn validate_ranges(what: &str, fields: [(&str, CenteredRange); 6]) -> Result<(), TownSpecError> {
    for (field, range) in fields {
        if !range.base.is_finite()
            || !range.spread.is_finite()
            || range.spread < 0.0
            || range.base - range.spread < 0.0
            || range.base + range.spread > 1.0
        {
            return Err(TownSpecError::InvalidRange(format!("{what}.{field}")));
        }
    }
    Ok(())
}

fn build_world(spec: &TownSpec, seed: u64) -> World {
    let location_ids: Vec<_> = (0..spec.locations.len())
        .map(|index| LocationId(seeded_uuid(2, seed, index as u32)))
        .collect();
    let id_of: BTreeMap<_, _> = spec
        .locations
        .iter()
        .zip(&location_ids)
        .map(|(location, id)| (location.name.as_str(), *id))
        .collect();

    let mut locations: BTreeMap<_, _> = location_ids
        .iter()
        .copied()
        .zip(&spec.locations)
        .map(|(id, location)| {
            (
                id,
                Location {
                    id,
                    name: location.name.clone(),
                    business: location.business.as_ref().map(|business| Business {
                        offering: business.offering,
                        price: business.price,
                        cash: BUSINESS_STARTING_CASH,
                        stock: STARTING_STOCK,
                        revenue: 0,
                        wages_paid: 0,
                    }),
                    opening_hours: location.opening_hours,
                    connected: BTreeSet::new(),
                    agents: BTreeSet::new(),
                },
            )
        })
        .collect();

    for [left, right] in &spec.connections {
        let left_id = id_of[left.as_str()];
        let right_id = id_of[right.as_str()];
        locations
            .get_mut(&left_id)
            .expect("validated connection endpoint")
            .connected
            .insert(right_id);
        locations
            .get_mut(&right_id)
            .expect("validated connection endpoint")
            .connected
            .insert(left_id);
    }

    let mut agents = BTreeMap::new();
    let mut rng = StdRng::seed_from_u64(seed);
    for (index, resident) in spec.residents.iter().enumerate() {
        let id = AgentId(seeded_uuid(1, seed, index as u32));
        let home = id_of[resident.home.as_str()];
        let workplace = resident.workplace.as_ref().map(|name| id_of[name.as_str()]);
        let agent = Agent {
            id,
            name: resident.name.clone(),
            age: resident.age,
            occupation: resident.occupation.clone(),
            home,
            workplace,
            location: home,
            personality: Personality {
                openness: sample(&mut rng, resident.personality.openness),
                agreeableness: sample(&mut rng, resident.personality.agreeableness),
                neuroticism: sample(&mut rng, resident.personality.neuroticism),
                honesty: sample(&mut rng, resident.personality.honesty),
                ambition: sample(&mut rng, resident.personality.ambition),
                impulsiveness: sample(&mut rng, resident.personality.impulsiveness),
            },
            needs: Needs {
                money: sample(&mut rng, resident.needs.money),
                food: sample(&mut rng, resident.needs.food),
                companionship: sample(&mut rng, resident.needs.companionship),
                safety: sample(&mut rng, resident.needs.safety),
                status: sample(&mut rng, resident.needs.status),
                energy: sample(&mut rng, resident.needs.energy),
            },
            health: 1.0,
            injury: false,
            disease: DiseaseState::Susceptible,
            life: LifeState::Alive,
            balance: 20,
            routing: RoutingStats::default(),
            inventory: Inventory::default(),
            activity: None,
            intention: None,
            llm_intention: false,
            mood: 0.0,
            relationships: BTreeMap::new(),
            beliefs: BTreeMap::new(),
            goals: Vec::new(),
            memories: Vec::new(),
            rumors: Vec::new(),
        };
        locations
            .get_mut(&home)
            .expect("validated home")
            .agents
            .insert(id);
        agents.insert(id, agent);
    }

    let agent_ids: Vec<_> = agents.keys().copied().collect();
    for pair in agent_ids.windows(2) {
        agents
            .get_mut(&pair[0])
            .expect("validated agent")
            .relationships
            .insert(
                pair[1],
                Relationship {
                    affection: 0.2,
                    trust: 0.1,
                    respect: 0.1,
                    ..Relationship::NEUTRAL
                },
            );
    }

    let tick = Tick(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES);
    World {
        name: spec.name.clone(),
        seed,
        tick,
        agents,
        locations,
        active_town_event: TownEvent::scheduled(seed, tick),
        events: Vec::new(),
    }
}

fn sample(rng: &mut StdRng, range: CenteredRange) -> f32 {
    (range.base + rng.random_range(-range.spread..=range.spread)).clamp(0.0, 1.0)
}
