use super::*;

impl World {
    pub fn briar_glen(seed: u64) -> Result<Self, WorldError> {
        let location_names = [
            "The Crooked Lantern",
            "Mara's Bakery",
            "Town Hall",
            "General Store",
            "Old Chapel",
            "Riverside Houses",
            "Abandoned Mill",
            "Carpenter's Workshop",
            "Briar Glen Clinic",
            "Jail",
        ];
        let location_ids: Vec<_> = (0..location_names.len())
            .map(|index| LocationId(seeded_uuid(2, seed, index as u32)))
            .collect();
        let mut locations: BTreeMap<_, _> = location_names
            .into_iter()
            .zip(location_ids.iter().copied())
            .map(|(name, id)| {
                let offering = match name {
                    "The Crooked Lantern" | "Mara's Bakery" => Some((Offering::Meal, 5)),
                    "General Store" | "Abandoned Mill" => Some((Offering::Supplies, 6)),
                    "Carpenter's Workshop" => Some((Offering::Repairs, 8)),
                    "Briar Glen Clinic" => Some((Offering::Medicine, CLINIC_PRICE)),
                    "Town Hall" => Some((Offering::CivicServices, 4)),
                    _ => None,
                };
                (
                    id,
                    Location {
                        id,
                        name: name.into(),
                        business: offering.map(|(offering, price)| Business {
                            offering,
                            price,
                            cash: BUSINESS_STARTING_CASH,
                            stock: STARTING_STOCK,
                            revenue: 0,
                            wages_paid: 0,
                        }),
                        opening_hours: match name {
                            "The Crooked Lantern" => Some(OpeningHours {
                                opens_at_hour: 12,
                                closes_at_hour: 23,
                            }),
                            "Mara's Bakery" => Some(OpeningHours {
                                opens_at_hour: 6,
                                closes_at_hour: 14,
                            }),
                            "Old Chapel" => Some(OpeningHours {
                                opens_at_hour: 6,
                                closes_at_hour: 20,
                            }),
                            "Briar Glen Clinic" => Some(OpeningHours {
                                opens_at_hour: 8,
                                closes_at_hour: 20,
                            }),
                            "Riverside Houses" => None,
                            _ => Some(OpeningHours {
                                opens_at_hour: 8,
                                closes_at_hour: 18,
                            }),
                        },
                        connected: BTreeSet::new(),
                        agents: BTreeSet::new(),
                    },
                )
            })
            .collect();

        for (left, right) in [
            (0, 1),
            (0, 2),
            (0, 5),
            (1, 3),
            (1, 5),
            (2, 3),
            (2, 4),
            (3, 5),
            (4, 5),
            (4, 6),
            (5, 6),
            (5, 7),
            (6, 7),
            (2, 8),
            (5, 8),
            (2, 9),
        ] {
            locations
                .get_mut(&location_ids[left])
                .ok_or(WorldError::UnknownLocation(location_ids[left]))?
                .connected
                .insert(location_ids[right]);
            locations
                .get_mut(&location_ids[right])
                .ok_or(WorldError::UnknownLocation(location_ids[right]))?
                .connected
                .insert(location_ids[left]);
        }

        let residents = [
            ("Mara Quinn", 41, Occupation::Baker, 1),
            ("Elias Ward", 46, Occupation::Carpenter, 7),
            ("Alice Vale", 35, Occupation::Shopkeeper, 3),
            ("Bob Mercer", 29, Occupation::Laborer, 6),
            ("Clara Voss", 38, Occupation::Teacher, 2),
            ("Sheriff Hale", 52, Occupation::Sheriff, 2),
            ("Jonas Reed", 44, Occupation::Publican, 0),
            ("Iris Bell", 27, Occupation::Doctor, 8),
        ];
        let mut agents = BTreeMap::new();
        let mut rng = StdRng::seed_from_u64(seed);
        for (index, (name, age, occupation, workplace)) in residents.into_iter().enumerate() {
            let id = AgentId(seeded_uuid(1, seed, index as u32));
            let home = location_ids[5];
            let personality_offset = index as f32 * 0.03;
            let mut vary = |base: f32| (base + rng.random_range(-0.15..=0.15)).clamp(0.0, 1.0);
            let personality = Personality {
                openness: vary(0.45 + personality_offset),
                agreeableness: vary(0.7 - personality_offset),
                neuroticism: vary(0.25 + personality_offset),
                honesty: vary(0.75 - personality_offset / 2.0),
                ambition: vary(0.4 + personality_offset),
                impulsiveness: vary(0.5 - personality_offset),
            };
            let mut vary_need = |base: f32| (base + rng.random_range(-0.08..=0.08)).clamp(0.0, 1.0);
            let agent = Agent {
                id,
                name: name.into(),
                age,
                occupation,
                home,
                workplace: Some(location_ids[workplace]),
                location: home,
                personality,
                needs: Needs {
                    money: vary_need(0.5),
                    food: vary_need(0.2),
                    companionship: vary_need(0.3),
                    safety: vary_need(0.15),
                    status: vary_need(0.35),
                    energy: vary_need(0.8),
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
                .ok_or(WorldError::UnknownLocation(home))?
                .agents
                .insert(id);
            agents.insert(id, agent);
        }

        let agent_ids: Vec<_> = agents.keys().copied().collect();
        for pair in agent_ids.windows(2) {
            agents
                .get_mut(&pair[0])
                .ok_or(WorldError::UnknownAgent(pair[0]))?
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

        let mut world = Self {
            name: "Briar Glen".into(),
            seed,
            tick: Tick(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES),
            agents,
            locations,
            active_town_event: TownEvent::scheduled(
                seed,
                Tick(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES),
            ),
            events: Vec::new(),
        };
        for agent in agent_ids {
            world.refresh_goals(agent);
        }
        world.validate()?;
        Ok(world)
    }
}
