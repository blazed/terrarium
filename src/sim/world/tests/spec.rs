use crate::sim::{AgentId, BRIAR_GLEN, LocationId, TownSpecError, World, seeded_uuid};

fn minimal_spec() -> String {
    r#"{
        "name": "Smoke Town",
        "locations": [
            { "name": "Riverside", "kind": "home" },
            { "name": "Market", "kind": "other", "business": { "offering": "supplies", "price": 3 } }
        ],
        "connections": [["Riverside", "Market"]],
        "residents": [
            {
                "name": "Test Person",
                "age": 30,
                "occupation": "Laborer",
                "home": "Riverside",
                "personality": {
                    "openness": { "base": 0.4, "spread": 0.15 },
                    "agreeableness": { "base": 0.5, "spread": 0.15 },
                    "neuroticism": { "base": 0.3, "spread": 0.15 },
                    "honesty": { "base": 0.6, "spread": 0.15 },
                    "ambition": { "base": 0.5, "spread": 0.15 },
                    "impulsiveness": { "base": 0.5, "spread": 0.15 }
                },
                "needs": {
                    "money": { "base": 0.5, "spread": 0.1 },
                    "food": { "base": 0.7, "spread": 0.1 },
                    "companionship": { "base": 0.3, "spread": 0.1 },
                    "safety": { "base": 0.6, "spread": 0.1 },
                    "status": { "base": 0.4, "spread": 0.1 },
                    "energy": { "base": 0.8, "spread": 0.1 }
                }
            }
        ]
    }"#
    .to_string()
}

#[test]
fn from_spec_builds_minimal_world() {
    let world = World::from_spec(&minimal_spec(), 42).expect("spec should be valid");
    assert_eq!(world.name, "Smoke Town");
    assert_eq!(world.seed, 42);
    assert_eq!(world.locations.len(), 2);
    assert_eq!(world.agents.len(), 1);
    let home_id = LocationId(seeded_uuid(2, 42, 0));
    let market_id = LocationId(seeded_uuid(2, 42, 1));
    assert!(world.locations.contains_key(&home_id));
    assert!(world.locations.contains_key(&market_id));
    assert!(world.locations[&home_id].connected.contains(&market_id));
    assert!(world.locations[&market_id].connected.contains(&home_id));
    let agent_id = AgentId(seeded_uuid(1, 42, 0));
    let agent = &world.agents[&agent_id];
    assert_eq!(agent.home, home_id);
    assert_eq!(agent.location, home_id);
    assert!(world.locations[&home_id].agents.contains(&agent_id));
}

/// Canonical, stable JSON of a freshly built world for golden regression.
/// Serialized as a tuple of the world's serializable components (never a
/// Serialize derive on World itself).
fn canonical_json(world: &World) -> String {
    serde_json::to_string(&(
        &world.name,
        &world.seed,
        &world.tick,
        &world.agents,
        &world.locations,
        &world.active_town_event,
        &world.events,
    ))
    .unwrap()
}

const GOLDEN_BRIAR_GLEN_SEED_0: &str = r###"["Briar Glen",0,84,{"01000000-0000-0000-0000-000000000000":{"id":"01000000-0000-0000-0000-000000000000","name":"Mara Quinn","age":41,"occupation":"Baker","home":"02000000-0000-0000-0000-00000000000a","workplace":"02000000-0000-0000-0000-000000000001","location":"02000000-0000-0000-0000-00000000000a","personality":{"openness":0.54043776,"agreeableness":0.76933396,"neuroticism":0.26658663,"honesty":0.83203804,"ambition":0.49280262,"impulsiveness":0.35775337},"needs":{"money":0.52743644,"food":0.21346548,"companionship":0.3593005,"safety":0.1119473,"status":0.28965273,"energy":0.8435179},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000001":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Bob Mercer","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000003"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Mara's Bakery","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000001":{"id":"01000000-0000-0000-0000-000000000001","name":"Elias Ward","age":46,"occupation":"Carpenter","home":"02000000-0000-0000-0000-00000000000b","workplace":"02000000-0000-0000-0000-000000000007","location":"02000000-0000-0000-0000-00000000000b","personality":{"openness":0.35110962,"agreeableness":0.5856091,"neuroticism":0.3667159,"honesty":0.82311404,"ambition":0.465218,"impulsiveness":0.54480153},"needs":{"money":0.50482476,"food":0.27503002,"companionship":0.35599732,"safety":0.22941077,"status":0.31519553,"energy":0.7423002},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000002":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Clara Voss","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000004"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Carpenter's Workshop","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000007"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000002":{"id":"01000000-0000-0000-0000-000000000002","name":"Alice Vale","age":35,"occupation":"Shopkeeper","home":"02000000-0000-0000-0000-00000000000c","workplace":"02000000-0000-0000-0000-000000000003","location":"02000000-0000-0000-0000-00000000000c","personality":{"openness":0.37386432,"agreeableness":0.5238595,"neuroticism":0.45717114,"honesty":0.8594507,"ambition":0.37179384,"impulsiveness":0.30482},"needs":{"money":0.48889607,"food":0.18811232,"companionship":0.3242998,"safety":0.14369275,"status":0.30058748,"energy":0.867882},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000003":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Sheriff Hale","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000005"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at General Store","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000003"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000003":{"id":"01000000-0000-0000-0000-000000000003","name":"Bob Mercer","age":29,"occupation":"Laborer","home":"02000000-0000-0000-0000-00000000000d","workplace":"02000000-0000-0000-0000-000000000006","location":"02000000-0000-0000-0000-00000000000d","personality":{"openness":0.6886326,"agreeableness":0.5281122,"neuroticism":0.30890918,"honesty":0.72279966,"ambition":0.54577124,"impulsiveness":0.30661672},"needs":{"money":0.53535795,"food":0.25367993,"companionship":0.29433426,"safety":0.10273202,"status":0.28210753,"energy":0.8005007},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000004":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Jonas Reed","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000006"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Abandoned Mill","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000006"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000004":{"id":"01000000-0000-0000-0000-000000000004","name":"Clara Voss","age":38,"occupation":"Teacher","home":"02000000-0000-0000-0000-00000000000e","workplace":"02000000-0000-0000-0000-000000000002","location":"02000000-0000-0000-0000-00000000000e","personality":{"openness":0.6841036,"agreeableness":0.6213248,"neuroticism":0.44422644,"honesty":0.6836548,"ambition":0.47332442,"impulsiveness":0.49282312},"needs":{"money":0.54697543,"food":0.25930566,"companionship":0.33711427,"safety":0.08011704,"status":0.41803867,"energy":0.8779025},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000005":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Iris Bell","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000007"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Town Hall","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000002"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000005":{"id":"01000000-0000-0000-0000-000000000005","name":"Sheriff Hale","age":52,"occupation":"Sheriff","home":"02000000-0000-0000-0000-00000000000f","workplace":"02000000-0000-0000-0000-000000000002","location":"02000000-0000-0000-0000-00000000000f","personality":{"openness":0.49846488,"agreeableness":0.46503925,"neuroticism":0.38108742,"honesty":0.818116,"ambition":0.48911652,"impulsiveness":0.47523317},"needs":{"money":0.48211578,"food":0.22835255,"companionship":0.28066018,"safety":0.07240401,"status":0.4009837,"energy":0.73107547},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000006":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Jonas Reed","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000006"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Town Hall","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000002"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000006":{"id":"01000000-0000-0000-0000-000000000006","name":"Jonas Reed","age":44,"occupation":"Publican","home":"02000000-0000-0000-0000-000000000010","workplace":"02000000-0000-0000-0000-000000000000","location":"02000000-0000-0000-0000-000000000010","personality":{"openness":0.7367779,"agreeableness":0.5554463,"neuroticism":0.490249,"honesty":0.6941308,"ambition":0.6314972,"impulsiveness":0.40557128},"needs":{"money":0.47199163,"food":0.22654217,"companionship":0.35834116,"safety":0.08350503,"status":0.40058553,"energy":0.7334343},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{"01000000-0000-0000-0000-000000000007":{"affection":0.2,"trust":0.1,"fear":0.0,"respect":0.1,"attraction":0.0,"suspicion":0.0}},"beliefs":{},"goals":[{"description":"Catch up with Mara Quinn","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000000"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at The Crooked Lantern","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000000"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]},"01000000-0000-0000-0000-000000000007":{"id":"01000000-0000-0000-0000-000000000007","name":"Iris Bell","age":27,"occupation":"Doctor","home":"02000000-0000-0000-0000-000000000011","workplace":"02000000-0000-0000-0000-000000000008","location":"02000000-0000-0000-0000-000000000011","personality":{"openness":0.79643065,"agreeableness":0.49561355,"neuroticism":0.56830454,"honesty":0.72088134,"ambition":0.53617764,"impulsiveness":0.4003323},"needs":{"money":0.4465215,"food":0.13401677,"companionship":0.25771916,"safety":0.12617953,"status":0.41366112,"energy":0.74997634},"health":1.0,"injury":false,"disease":{"stage":"susceptible"},"life":{"state":"alive"},"balance":20,"routing":{"budget_day":0,"llm_calls_today":0,"last_llm_attempt":null,"local_decisions":0,"llm_decisions":0,"llm_fallbacks":0,"llm_intentions_started":0,"llm_intention_steps":0,"llm_intentions_completed":0,"llm_intentions_interrupted":0},"inventory":{"meals":0,"supplies":0,"repair_kits":0,"medicine":0},"activity":null,"intention":null,"llm_intention":false,"mood":0.0,"relationships":{},"beliefs":{},"goals":[{"description":"Catch up with Alice Vale","kind":"community","target":{"type":"talk","resident":"01000000-0000-0000-0000-000000000002"},"progress":0,"required":1,"expires_at":372},{"description":"Complete two shifts at Briar Glen Clinic","kind":"livelihood","target":{"type":"work","workplace":"02000000-0000-0000-0000-000000000008"},"progress":0,"required":2,"expires_at":372},{"description":"Buy meal at Mara's Bakery","kind":"wellbeing","target":{"type":"purchase","location":"02000000-0000-0000-0000-000000000001"},"progress":0,"required":1,"expires_at":372}],"memories":[],"rumors":[]}},{"02000000-0000-0000-0000-000000000000":{"id":"02000000-0000-0000-0000-000000000000","name":"The Crooked Lantern","kind":"other","business":{"offering":"meal","price":5,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":12,"closes_at_hour":23},"connected":["02000000-0000-0000-0000-000000000001","02000000-0000-0000-0000-000000000002","02000000-0000-0000-0000-000000000005"],"agents":[]},"02000000-0000-0000-0000-000000000001":{"id":"02000000-0000-0000-0000-000000000001","name":"Mara's Bakery","kind":"other","business":{"offering":"meal","price":5,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":6,"closes_at_hour":14},"connected":["02000000-0000-0000-0000-000000000000","02000000-0000-0000-0000-000000000003","02000000-0000-0000-0000-000000000005"],"agents":[]},"02000000-0000-0000-0000-000000000002":{"id":"02000000-0000-0000-0000-000000000002","name":"Town Hall","kind":"other","business":{"offering":"civic_services","price":4,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":8,"closes_at_hour":18},"connected":["02000000-0000-0000-0000-000000000000","02000000-0000-0000-0000-000000000003","02000000-0000-0000-0000-000000000004","02000000-0000-0000-0000-000000000008","02000000-0000-0000-0000-000000000009"],"agents":[]},"02000000-0000-0000-0000-000000000003":{"id":"02000000-0000-0000-0000-000000000003","name":"General Store","kind":"other","business":{"offering":"supplies","price":6,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":8,"closes_at_hour":18},"connected":["02000000-0000-0000-0000-000000000001","02000000-0000-0000-0000-000000000002","02000000-0000-0000-0000-000000000005"],"agents":[]},"02000000-0000-0000-0000-000000000004":{"id":"02000000-0000-0000-0000-000000000004","name":"Old Chapel","kind":"other","business":null,"opening_hours":{"opens_at_hour":6,"closes_at_hour":20},"connected":["02000000-0000-0000-0000-000000000002","02000000-0000-0000-0000-000000000005","02000000-0000-0000-0000-000000000006"],"agents":[]},"02000000-0000-0000-0000-000000000005":{"id":"02000000-0000-0000-0000-000000000005","name":"Riverside Houses","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000000","02000000-0000-0000-0000-000000000001","02000000-0000-0000-0000-000000000003","02000000-0000-0000-0000-000000000004","02000000-0000-0000-0000-000000000006","02000000-0000-0000-0000-000000000007","02000000-0000-0000-0000-000000000008","02000000-0000-0000-0000-00000000000a","02000000-0000-0000-0000-00000000000b","02000000-0000-0000-0000-00000000000c","02000000-0000-0000-0000-00000000000d","02000000-0000-0000-0000-00000000000e","02000000-0000-0000-0000-00000000000f","02000000-0000-0000-0000-000000000010","02000000-0000-0000-0000-000000000011"],"agents":[]},"02000000-0000-0000-0000-000000000006":{"id":"02000000-0000-0000-0000-000000000006","name":"Abandoned Mill","kind":"other","business":{"offering":"supplies","price":6,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":8,"closes_at_hour":18},"connected":["02000000-0000-0000-0000-000000000004","02000000-0000-0000-0000-000000000005","02000000-0000-0000-0000-000000000007"],"agents":[]},"02000000-0000-0000-0000-000000000007":{"id":"02000000-0000-0000-0000-000000000007","name":"Carpenter's Workshop","kind":"other","business":{"offering":"repairs","price":8,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":8,"closes_at_hour":18},"connected":["02000000-0000-0000-0000-000000000005","02000000-0000-0000-0000-000000000006"],"agents":[]},"02000000-0000-0000-0000-000000000008":{"id":"02000000-0000-0000-0000-000000000008","name":"Briar Glen Clinic","kind":"other","business":{"offering":"medicine","price":12,"cash":100,"stock":20,"revenue":0,"wages_paid":0},"opening_hours":{"opens_at_hour":8,"closes_at_hour":20},"connected":["02000000-0000-0000-0000-000000000002","02000000-0000-0000-0000-000000000005"],"agents":[]},"02000000-0000-0000-0000-000000000009":{"id":"02000000-0000-0000-0000-000000000009","name":"Jail","kind":"other","business":null,"opening_hours":{"opens_at_hour":8,"closes_at_hour":18},"connected":["02000000-0000-0000-0000-000000000002"],"agents":[]},"02000000-0000-0000-0000-00000000000a":{"id":"02000000-0000-0000-0000-00000000000a","name":"Mara Quinn's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000000"]},"02000000-0000-0000-0000-00000000000b":{"id":"02000000-0000-0000-0000-00000000000b","name":"Elias Ward's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000001"]},"02000000-0000-0000-0000-00000000000c":{"id":"02000000-0000-0000-0000-00000000000c","name":"Alice Vale's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000002"]},"02000000-0000-0000-0000-00000000000d":{"id":"02000000-0000-0000-0000-00000000000d","name":"Bob Mercer's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000003"]},"02000000-0000-0000-0000-00000000000e":{"id":"02000000-0000-0000-0000-00000000000e","name":"Clara Voss's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000004"]},"02000000-0000-0000-0000-00000000000f":{"id":"02000000-0000-0000-0000-00000000000f","name":"Sheriff Hale's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000005"]},"02000000-0000-0000-0000-000000000010":{"id":"02000000-0000-0000-0000-000000000010","name":"Jonas Reed's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000006"]},"02000000-0000-0000-0000-000000000011":{"id":"02000000-0000-0000-0000-000000000011","name":"Iris Bell's Home","kind":"home","business":null,"opening_hours":null,"connected":["02000000-0000-0000-0000-000000000005"],"agents":["01000000-0000-0000-0000-000000000007"]}},null,[]]"###;

#[test]
fn briar_glen_matches_golden_fixture() {
    let world = World::from_spec(BRIAR_GLEN, 0).unwrap();
    assert_eq!(canonical_json(&world), GOLDEN_BRIAR_GLEN_SEED_0);
}

/// JSON fragments for building test towns.
const HOME: &str = r#"{"name":"Riverside","kind":"home"}"#;
const MARKET: &str =
    r#"{"name":"Market","kind":"other","business":{"offering":"supplies","price":3}}"#;
const NOBIZ: &str = r#"{"name":"Chapel","kind":"other"}"#;
const RESIDENT: &str = r#"{"name":"Test Person","age":30,"occupation":"Laborer","home":"Riverside","personality":{"openness":{"base":0.4,"spread":0.15},"agreeableness":{"base":0.5,"spread":0.15},"neuroticism":{"base":0.3,"spread":0.15},"honesty":{"base":0.6,"spread":0.15},"ambition":{"base":0.5,"spread":0.15},"impulsiveness":{"base":0.5,"spread":0.15}},"needs":{"money":{"base":0.5,"spread":0.1},"food":{"base":0.7,"spread":0.1},"companionship":{"base":0.3,"spread":0.1},"safety":{"base":0.6,"spread":0.1},"status":{"base":0.4,"spread":0.1},"energy":{"base":0.8,"spread":0.1}}}"#;

fn town(name: &str, locations: &str, connections: &str, residents: &str) -> String {
    format!(
        r#"{{"name":"{name}","locations":[{locations}],"connections":[{connections}],"residents":[{residents}]}}"#
    )
}

/// Short canonical name of a spec failure, asserted in `invalid_specs_are_rejected_with_typed_errors`.
fn failure_kind(error: &TownSpecError) -> &'static str {
    match error {
        TownSpecError::MalformedJson(_) => "MalformedJson",
        TownSpecError::EmptyTownName => "EmptyTownName",
        TownSpecError::NoLocations => "NoLocations",
        TownSpecError::NoResidents => "NoResidents",
        TownSpecError::EmptyLocationName => "EmptyLocationName",
        TownSpecError::EmptyResidentName => "EmptyResidentName",
        TownSpecError::DuplicateLocationName(_) => "DuplicateLocationName",
        TownSpecError::DuplicateResidentName(_) => "DuplicateResidentName",
        TownSpecError::UnknownConnectionEndpoint(_) => "UnknownConnectionEndpoint",
        TownSpecError::SelfConnection(_) => "SelfConnection",
        TownSpecError::DuplicateConnection(_, _) => "DuplicateConnection",
        TownSpecError::UnknownHome(_) => "UnknownHome",
        TownSpecError::HomeNotHome(_) => "HomeNotHome",
        TownSpecError::HomeWithOpeningHours(_) => "HomeWithOpeningHours",
        TownSpecError::UnknownWorkplace(_) => "UnknownWorkplace",
        TownSpecError::WorkplaceWithoutBusiness(_) => "WorkplaceWithoutBusiness",
        TownSpecError::InvalidOpeningHours(_) => "InvalidOpeningHours",
        TownSpecError::ZeroBusinessPrice(_) => "ZeroBusinessPrice",
        TownSpecError::InvalidRange(_) => "InvalidRange",
        TownSpecError::SheriffWithoutJail => "SheriffWithoutJail",
        TownSpecError::World(_) => "World",
    }
}

#[test]
fn invalid_specs_are_rejected_with_typed_errors() {
    let cases: Vec<(&str, String, &str)> = vec![
        ("malformed json", "{ not json".into(), "MalformedJson"),
        (
            "unknown location field",
            town(
                "T",
                &format!(r#"{{"name":"Riverside","kind":"home","bogus":1}},{MARKET}"#),
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "MalformedJson",
        ),
        (
            "empty town name",
            town("", HOME, r#"["Riverside","Market"]"#, RESIDENT),
            "EmptyTownName",
        ),
        (
            "no locations",
            town("T", "", r#"["Riverside","Market"]"#, RESIDENT),
            "NoLocations",
        ),
        (
            "no residents",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                "",
            ),
            "NoResidents",
        ),
        (
            "empty location name",
            town(
                "T",
                &format!(r#"{{"name":"","kind":"home"}},{MARKET}"#),
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "EmptyLocationName",
        ),
        (
            "empty resident name",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace("\"name\":\"Test Person\"", "\"name\":\"\""),
            ),
            "EmptyResidentName",
        ),
        (
            "duplicate location name",
            town(
                "T",
                &format!("{HOME},{HOME}"),
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "DuplicateLocationName",
        ),
        (
            "duplicate resident name",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &format!("{RESIDENT},{RESIDENT}"),
            ),
            "DuplicateResidentName",
        ),
        (
            "unknown connection endpoint",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Nope"]"#,
                RESIDENT,
            ),
            "UnknownConnectionEndpoint",
        ),
        (
            "self connection",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Riverside"]"#,
                RESIDENT,
            ),
            "SelfConnection",
        ),
        (
            "duplicate connection",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"],["Market","Riverside"]"#,
                RESIDENT,
            ),
            "DuplicateConnection",
        ),
        (
            "invalid opening hours",
            town(
                "T",
                r#"{"name":"Riverside","kind":"home"},{"name":"Market","kind":"other","opening_hours":{"opens_at_hour":10,"closes_at_hour":10}}"#,
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "InvalidOpeningHours",
        ),
        (
            "home with opening hours",
            town(
                "T",
                &format!(
                    r#"{{"name":"Riverside","kind":"home","opening_hours":{{"opens_at_hour":8,"closes_at_hour":18}}}},{MARKET}"#
                ),
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "HomeWithOpeningHours",
        ),
        (
            "zero business price",
            town(
                "T",
                r#"{"name":"Riverside","kind":"home"},{"name":"Market","kind":"other","business":{"offering":"supplies","price":0}}"#,
                r#"["Riverside","Market"]"#,
                RESIDENT,
            ),
            "ZeroBusinessPrice",
        ),
        (
            "unknown home",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace("\"home\":\"Riverside\"", "\"home\":\"Nope\""),
            ),
            "UnknownHome",
        ),
        (
            "home referencing non-home location",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace("\"home\":\"Riverside\"", "\"home\":\"Market\""),
            ),
            "HomeNotHome",
        ),
        (
            "unknown workplace",
            town(
                "T",
                &format!("{HOME},{MARKET},{NOBIZ}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace(
                    "\"home\":\"Riverside\"",
                    "\"home\":\"Riverside\",\"workplace\":\"Nope\"",
                ),
            ),
            "UnknownWorkplace",
        ),
        (
            "workplace without business",
            town(
                "T",
                &format!("{HOME},{MARKET},{NOBIZ}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace(
                    "\"home\":\"Riverside\"",
                    "\"home\":\"Riverside\",\"workplace\":\"Chapel\"",
                ),
            ),
            "WorkplaceWithoutBusiness",
        ),
        (
            "invalid personality range",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace(
                    r#"{"base":0.4,"spread":0.15}"#,
                    r#"{"base":-0.5,"spread":0.15}"#,
                ),
            ),
            "InvalidRange",
        ),
        (
            "invalid need range",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace(
                    r#"{"base":0.8,"spread":0.1}"#,
                    r#"{"base":0.9,"spread":0.2}"#,
                ),
            ),
            "InvalidRange",
        ),
        (
            "sheriff without jail",
            town(
                "T",
                &format!("{HOME},{MARKET}"),
                r#"["Riverside","Market"]"#,
                &RESIDENT.replace("\"occupation\":\"Laborer\"", "\"occupation\":\"Sheriff\""),
            ),
            "SheriffWithoutJail",
        ),
    ];
    for (label, json, expected) in cases {
        let err = World::from_spec(&json, 7).expect_err(&format!("{label}: expected an error"));
        assert_eq!(failure_kind(&err), expected, "{label}: got {err:?}",);
    }
}
